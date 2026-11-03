use super::*;

const WAITING: &str = "Rooxcode is busy. Waiting…";
const KEEPALIVE: &str = ": rooxcode waiting\n\n";

fn queued(reply: (u16, String)) -> (u16, String) {
    (298, KEEPALIVE.to_owned() + &reply.1)
}

fn service_error(code: &str, status: u16) -> String {
    sse(
        json!({"error":{"code":code,"status":status,"retry_after":5,"message":"PRIVATE_UPSTREAM_BODY"}}),
    )
}

#[test]
fn waiting_comments_show_capacity_status_until_output_in_both_views() {
    for cli in [false, true] {
        let f = Fixture::new("chat_completions", vec![queued(done("CAPACITY_READY"))]);
        let before = setup(&f, json!([{"id":"free"}]));
        let mut command = f.command();
        if cli {
            command.arg("--cli");
        }
        let mut tty = Pty::spawn_with_color(&mut command, true);
        tty.wait_text(if cli { "/tui" } else { "Enter send" });
        tty.send(b"hello\r");
        tty.wait_text(WAITING);
        tty.wait_text("CAPACITY_READY");
        if !cli {
            tty.wait_text("Enter send");
        }
        tty.send(b"/quit\r");
        let shown = tty.finish();
        // CLI uses ordinary scrollback rather than synchronized TUI frames.
        let views = support::screen::snapshots(&(shown.clone() + "\x1b[?2026l"));
        if !cli {
            let mut flowers = std::collections::HashSet::new();
            for view in &views {
                let lines: Vec<_> = view.lines().collect();
                if let Some(status) = lines.iter().position(|line| line.contains(WAITING)) {
                    let composer = lines.iter().rposition(|line| line.contains('╰')).unwrap();
                    assert_eq!(status, composer + 1, "{view}");
                    flowers.insert(lines[status].trim_start().chars().next().unwrap());
                }
            }
            assert!(flowers.len() >= 2, "Waiting indicator did not animate");
        }
        let final_view = views
            .iter()
            .rev()
            .find(|v| v.contains("CAPACITY_READY"))
            .unwrap();
        assert!(!final_view.contains(WAITING), "{final_view}");
        assert!(!shown.contains(": rooxcode waiting"));
        assert_eq!(f.captured().len(), 1);
        assert_eq!(fs::read(f.root.join("app/config.json")).unwrap(), before);
        assert_metadata(&f);
        let session = fs::read_dir(f.root.join("app/sessions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(!fs::read_to_string(session)
            .unwrap()
            .contains("rooxcode waiting"));
    }
}

#[test]
fn queued_tools_and_continuation_keep_the_conversation_and_credentials_isolated() {
    let f = Fixture::new(
        "chat_completions",
        vec![queued(tool()), queued(done("TOOLS_DONE"))],
    );
    setup(&f, json!([{"id":"free"}]));
    good(&f.run("", &["do the task"]));
    assert_eq!(
        fs::read_to_string(f.root.join("roox.txt")).unwrap(),
        "verified tool"
    );
    let requests = f.captured();
    assert_eq!(requests.len(), 2);
    assert!(requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "tool" && m["tool_call_id"] == "roox-call"));
    assert_metadata(&f);
}

#[test]
fn escape_cancels_queued_http_and_settles_without_replay() {
    let f = Fixture::new(
        "chat_completions",
        vec![(299, KEEPALIVE.into()), done("MANUAL_NEXT")],
    );
    setup(&f, json!([{"id":"free"}]));
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("Enter send");
    tty.send(b"queued prompt\r");
    tty.wait_text(WAITING);
    let start = Instant::now();
    tty.send(b"\x1b");
    tty.wait_text("Stopped");
    assert!(start.elapsed() < Duration::from_secs(2));
    let deadline = Instant::now() + Duration::from_secs(2);
    while !f.root.join("stream-disconnected").exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(f.root.join("stream-disconnected").exists());
    assert_eq!(f.captured().len(), 1);
    tty.send(b"manual next\r");
    tty.wait_text("MANUAL_NEXT");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(f.captured().len(), 2);
    assert_metadata(&f);
}

#[test]
fn http_and_sse_queue_errors_and_terminal_allowances_do_not_retry_or_leak() {
    for (code, status, expected) in [
        ("queue_full", 429, "shared waiting queue is full"),
        (
            "network_queue_full",
            429,
            "maximum number of waiting requests",
        ),
        ("queue_timeout", 429, "maximum queue wait elapsed"),
        ("queue_disabled", 503, "disabled by the operator"),
        (
            "network_daily_quota_exhausted",
            429,
            "network's daily quota",
        ),
        ("shared_quota_exhausted", 429, "shared daily quota"),
        (
            "upstream_quota_exhausted",
            429,
            "upstream free request quota",
        ),
        ("network_work_budget_exhausted", 429, "daily work budget"),
    ] {
        for stream in [false, true] {
            let reply = if stream {
                (200, KEEPALIVE.to_owned() + &service_error(code, status))
            } else {
                (
                    status,
                    json!({"error":{"code":code,"message":"PRIVATE_UPSTREAM_BODY"}}).to_string(),
                )
            };
            let f = Fixture::new("chat_completions", vec![reply]);
            setup(&f, json!([{"id":"free"}]));
            fs::write(f.root.join("rooxcode-retry-after"), "5").unwrap();
            let result = f.run("", &["hello"]);
            assert!(!result.status.success());
            let shown = String::from_utf8_lossy(&result.stderr);
            assert!(
                shown.contains(expected) && shown.contains("Retry-After: 5"),
                "{code}: {shown}"
            );
            assert!(!shown.contains("PRIVATE_UPSTREAM_BODY"));
            if stream {
                assert!(shown.contains(&format!("stream error {status}")));
            }
            assert_eq!(f.captured().len(), 1);
            assert_metadata(&f);
        }
    }
}

#[test]
fn queued_failure_restores_for_review_and_does_not_replay_completed_tools() {
    let call = sse(
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"once","function":{"name":"bash","arguments":json!({"command":"printf once >> completed.txt"}).to_string()}}]},"finish_reason":"tool_calls"}]}),
    ) + "data: [DONE]\n\n";
    let f = Fixture::new(
        "chat_completions",
        vec![
            queued((200, call)),
            queued((200, service_error("queue_timeout", 429))),
            done("MANUAL_RECOVERY"),
        ],
    );
    setup(&f, json!([{"id":"free"}]));
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("Enter send");
    tty.send(b"original task\r");
    tty.wait_text("Rooxcode stream error 429");
    tty.wait_text("Use /restore");
    assert_eq!(f.captured().len(), 2);
    assert_eq!(
        fs::read_to_string(f.root.join("completed.txt")).unwrap(),
        "once"
    );
    tty.send(b"/restore\r");
    tty.wait_text("Restored failed prompt");
    assert_eq!(f.captured().len(), 2);
    tty.send(b"\x03");
    tty.send(b"manual continue\r");
    tty.wait_text("MANUAL_RECOVERY");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(
        fs::read_to_string(f.root.join("completed.txt")).unwrap(),
        "once"
    );
    assert_eq!(f.captured().len(), 3);
    assert_metadata(&f);
}

#[test]
fn interrupted_queued_stream_never_executes_partial_tools() {
    let (_, body) = tool();
    let f = Fixture::new(
        "chat_completions",
        vec![(
            200,
            KEEPALIVE.to_owned() + body.split("data: [DONE]").next().unwrap(),
        )],
    );
    setup(&f, json!([{"id":"free"}]));
    let result = f.run("", &["hello"]);
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("partial output was not committed")
            || String::from_utf8_lossy(&result.stderr).contains("before its completion event")
    );
    assert!(!f.root.join("roox.txt").exists());
    assert_eq!(f.captured().len(), 1);
}

#[test]
fn other_providers_ignore_rooxcode_comments_without_a_queue_status() {
    for api in ["responses", "chat_completions"] {
        let body = if api == "responses" {
            response("OTHER_PROVIDER").1
        } else {
            done("OTHER_PROVIDER").1
        };
        let f = Fixture::new(api, vec![(298, KEEPALIVE.to_owned() + &body)]);
        let mut tty = Pty::spawn(&mut f.command());
        tty.wait_text("Enter send");
        tty.send(b"hello\r");
        tty.wait_text("OTHER_PROVIDER");
        tty.wait_text("Enter send");
        tty.send(b"/quit\r");
        assert!(!tty.finish().contains(WAITING));
        assert_eq!(
            header(&post_headers(&f)[0], "authorization"),
            "Bearer SYNTHETIC-KEY"
        );
    }
}
