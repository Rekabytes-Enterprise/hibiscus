use super::*;
pub(super) const PLACEHOLDER: &str = "__FIXTURE_CHECKPOINT__";
/// The synthetic model takes the ID from the real history tool output, just as
/// a provider would. It never invents IDs or reads the app's checkpoint store.
pub(super) fn id_from_request(value: &Value) -> Option<String> {
    if let Some(id) = value["checkpoints"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|c| c["checkpoint"].as_str())
    {
        return Some(id.to_owned());
    }
    match value {
        Value::Object(object) => object.values().find_map(id_from_request),
        Value::Array(array) => array.iter().find_map(id_from_request),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|v| id_from_request(&v)),
        _ => None,
    }
}
fn records(f: &Fixture) -> Vec<Value> {
    let root = f.root.join("app/checkpoints");
    let mut result = Vec::new();
    if !root.exists() {
        return result;
    }
    for workspace in fs::read_dir(root).unwrap() {
        let requests = workspace.unwrap().path().join("requests");
        for request in fs::read_dir(requests).unwrap() {
            let bytes = fs::read(request.unwrap().path().join("record.json")).unwrap();
            result.push(serde_json::from_slice(&bytes).unwrap());
        }
    }
    result.sort_by(|a, b| a["createdAt"].as_str().cmp(&b["createdAt"].as_str()));
    result
}
fn undo_replies() -> Vec<(u16, String)> {
    vec![
        calls(vec![("checkpoint_history", json!({}))]),
        calls(vec![("checkpoint_diff", json!({"checkpoint":PLACEHOLDER}))]),
        calls(vec![(
            "checkpoint_restore",
            json!({"checkpoint":PLACEHOLDER}),
        )]),
        response("CHECKPOINT_UNDO_DONE"),
    ]
}
#[test]
fn native_edit_history_diff_restore_flow_runs_in_both_views_and_preserves_user_work() {
    for cli in [false, true] {
        for color in [false, true] {
            let mut replies = vec![
                calls(vec![
                    (
                        "edit",
                        json!({"path":"greeting.txt","oldText":"old greeting","newText":"new greeting"}),
                    ),
                    (
                        "write",
                        json!({"path":"generated.txt","content":"generated content\n"}),
                    ),
                ]),
                response("CHECKPOINT_EDIT_DONE"),
            ];
            replies.extend(undo_replies());
            let f = Fixture::new("responses", replies);
            let original = "# user's uncommitted note\r\nold greeting\r\n";
            fs::write(f.root.join("greeting.txt"), original).unwrap();
            // This fixture deliberately has no .git directory.
            let mut command = f.command();
            command.env("PATH", "/not-installed");
            if cli {
                command.arg("--cli");
            }
            let mut tty = Pty::spawn_with_size(&mut command, color, 110, 36);
            tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
            tty.send(b"change the greeting and create a generated file\r");
            tty.wait_text("CHECKPOINT_EDIT_DONE");
            tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
            assert_eq!(
                fs::read_to_string(f.root.join("greeting.txt")).unwrap(),
                original.replace("old greeting", "new greeting")
            );
            assert!(f.root.join("generated.txt").exists());
            let snapshots = records(&f);
            assert_eq!(snapshots.len(), 1);
            assert_eq!(snapshots[0]["files"].as_object().unwrap().len(), 2);
            tty.send(b"undo your last file changes and preserve my note\r");
            tty.wait_text("CHECKPOINT_UNDO_DONE");
            tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
            assert_eq!(
                fs::read(f.root.join("greeting.txt")).unwrap(),
                original.as_bytes()
            );
            assert!(!f.root.join("generated.txt").exists());
            tty.send(b"/quit\r");
            let shown = tty.finish();
            assert!(shown.contains("checkpoint_restore"));
            assert!(
                shown.contains("before/greeting.txt"),
                "checkpoint diff was not shown"
            );
            if cli && color {
                assert!(shown.contains("\x1b[48;2;24;57;37m"));
            }
            let requests = f.captured();
            assert_eq!(requests.len(), 6);
            assert!(requests[1].to_string().contains("Checkpoint "));
            assert!(requests[5].to_string().contains("greeting.txt"));
            for name in [
                "checkpoint_history",
                "checkpoint_diff",
                "checkpoint_restore",
                "checkpoint_prune",
            ] {
                assert!(requests[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["name"] == name));
            }
            let snapshots = records(&f);
            for entry in snapshots[0]["files"].as_object().unwrap().values() {
                assert_eq!(entry["phase"]["status"], "restored");
            }
        }
    }
}
#[test]
fn restored_session_reports_later_manual_conflict_without_touching_other_files() {
    let mut replies = vec![
        calls(vec![
            (
                "edit",
                json!({"path":"a.txt","oldText":"before","newText":"after"}),
            ),
            ("write", json!({"path":"new.txt","content":"generated"})),
        ]),
        response("EDIT_READY"),
    ];
    let mut undo = undo_replies();
    undo.pop();
    undo.push(response("CONFLICT_REPORTED"));
    replies.extend(undo);
    let f = Fixture::new("responses", replies);
    fs::write(f.root.join("a.txt"), "user note\nbefore").unwrap();
    good(&f.run("", &["make the native edits"]));
    fs::write(f.root.join("a.txt"), "user's later manual edit").unwrap();
    good(&f.run("undo last file changes\n/quit\n", &["--cli", "--continue"]));
    assert_eq!(
        fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "user's later manual edit"
    );
    assert_eq!(
        fs::read_to_string(f.root.join("new.txt")).unwrap(),
        "generated"
    );
    assert!(f
        .captured()
        .last()
        .unwrap()
        .to_string()
        .contains("Restore conflict; no selected files were changed"));
    let session = fs::read_dir(f.root.join("app/sessions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let history = fs::read_to_string(session).unwrap();
    assert!(history.contains("\"isError\":true"));
    assert!(history.contains("checkpoint_restore"));
    assert_eq!(records(&f).len(), 1);
}
#[test]
fn shell_effects_and_invalid_edits_are_not_falsely_checkpointed() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![
                ("bash", json!({"command":"printf external > shell.txt"})),
                (
                    "edit",
                    json!({"path":"existing.txt","oldText":"ambiguous","newText":"replacement"}),
                ),
                ("checkpoint_history", json!({})),
            ]),
            response("COVERAGE_REPORTED"),
        ],
    );
    fs::write(f.root.join("existing.txt"), "ambiguous ambiguous").unwrap();
    good(&f.run("", &["exercise coverage"]));
    assert_eq!(
        fs::read_to_string(f.root.join("shell.txt")).unwrap(),
        "external"
    );
    assert!(records(&f).is_empty());
    assert!(f.captured()[1]
        .to_string()
        .contains("native write/edit only"));
    assert_eq!(
        fs::read_to_string(f.root.join("existing.txt")).unwrap(),
        "ambiguous ambiguous"
    );
}
#[test]
fn checkpoints_remain_empty_after_an_incomplete_native_write_stream() {
    let status = 200;
    let body = sse(
        json!({"type":"response.output_item.done","item":{"type":"function_call","call_id":"tool-0","name":"write","arguments":json!({"path":"not-executed.txt","content":"no"}).to_string()}}),
    );
    let f = Fixture::new("responses", vec![(status, body)]);
    assert!(!f
        .run("", &["do not execute an incomplete call"])
        .status
        .success());
    assert!(!f.root.join("not-executed.txt").exists());
    assert!(records(&f).is_empty());
}

#[test]
fn queued_followups_and_steering_each_start_a_separate_checkpoint_group() {
    for follow_up in [false, true] {
        let mut replies = vec![calls(vec![
            (
                "write",
                json!({"path":"first.txt","content":"first request"}),
            ),
            ("bash", json!({"command":"touch queue-ready; sleep 1"})),
        ])];
        if follow_up {
            replies.push(response("FIRST_REQUEST_DONE"));
        }
        replies.push(calls(vec![(
            "write",
            json!({"path":"queued.txt","content":"queued request"}),
        )]));
        replies.push(response("QUEUED_REQUEST_DONE"));
        let f = Fixture::new("responses", replies);
        let mut tty = Pty::spawn(&mut f.command());
        tty.wait_text("Enter send");
        tty.send(b"first request\r");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f.root.join("queue-ready").exists() {
            tty.pump();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        tty.send(if follow_up {
            b"queued followup\x11"
        } else {
            b"steering request\r"
        });
        tty.wait_text("QUEUED_REQUEST_DONE");
        tty.wait_text("Enter send");
        tty.send(b"/quit\r");
        tty.finish();
        let records = records(&f);
        assert_eq!(records.len(), 2);
        assert_ne!(records[0]["id"], records[1]["id"]);
        assert_eq!(records[0]["sessionId"], records[1]["sessionId"]);
        assert!(records[0]["files"].get("first.txt").is_some());
        assert!(records[1]["files"].get("queued.txt").is_some());
    }
}

#[test]
fn native_restore_preserves_real_git_index_head_and_existing_uncommitted_content() {
    let mut replies = vec![
        calls(vec![(
            "edit",
            json!({"path":"tracked.txt","oldText":"old greeting","newText":"new greeting"}),
        )]),
        response("EDIT_READY"),
    ];
    replies.extend(undo_replies());
    let f = Fixture::new("responses", replies);
    fn git(f: &Fixture, args: &[&str]) -> Vec<u8> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&f.root)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TEMPLATE_DIR", "/dev/null")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
    git(&f, &["init", "-q"]);
    fs::write(f.root.join("tracked.txt"), "committed content\n").unwrap();
    git(&f, &["add", "tracked.txt"]);
    git(&f, &["commit", "-qm", "fixture baseline"]);
    fs::write(f.root.join("staged.txt"), "user staged work").unwrap();
    git(&f, &["add", "staged.txt"]);
    let original = "user uncommitted note\nold greeting\n";
    fs::write(f.root.join("tracked.txt"), original).unwrap();
    let index = fs::read(f.root.join(".git/index")).unwrap();
    let head = git(&f, &["rev-parse", "HEAD"]);
    let branch = git(&f, &["symbolic-ref", "HEAD"]);
    good(&f.run(
        "change the greeting\nundo the previous change\n/quit\n",
        &[],
    ));
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
    assert_eq!(git(&f, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&f, &["symbolic-ref", "HEAD"]), branch);
    assert_eq!(
        fs::read_to_string(f.root.join("tracked.txt")).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(f.root.join("staged.txt")).unwrap(),
        "user staged work"
    );
}

#[test]
fn history_defaults_to_this_session_and_explicitly_can_include_other_conversations() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![(
                "write",
                json!({"path":"first-session.txt","content":"first"}),
            )]),
            response("FIRST_DONE"),
            calls(vec![("checkpoint_history", json!({}))]),
            calls(vec![("checkpoint_history", json!({"allSessions":true}))]),
            response("HISTORY_DONE"),
        ],
    );
    good(&f.run("", &["first conversation"]));
    good(&f.run("", &["new conversation inspect history"]));
    let requests = f.captured();
    assert!(id_from_request(&requests[3]).is_none());
    assert_eq!(
        id_from_request(&requests[4]).unwrap(),
        records(&f)[0]["id"].as_str().unwrap()
    );
}
