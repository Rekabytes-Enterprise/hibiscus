use super::*;

fn models() -> Value {
    json!([
        {"id":"a","reasoning":true,"reasoning_efforts":["low","high"]},
        {"id":"b","reasoning":true,"reasoning_efforts":["none","high"]},
        {"id":"c","reasoning":true,"reasoning_efforts":["minimal","max"]}
    ])
}
fn catalog(f: &Fixture, status: u16, entries: Value) {
    fs::write(
        f.root.join("rooxcode-catalog.json"),
        json!({"status":status,"body":{"data":entries}}).to_string(),
    )
    .unwrap();
}
fn saved(f: &Fixture) -> Value {
    let entry = fs::read_dir(f.root.join("app/sessions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let data = fs::read_to_string(entry.path()).unwrap();
    assert!(!data.contains("Thinking reset"));
    serde_json::from_str(data.lines().next().unwrap()).unwrap()
}

#[test]
fn roox_thinking_missing_empty_and_populated_catalog_controls_wire_values() {
    for entry in [
        json!({"id":"a"}),
        json!({"id":"a","reasoning":true,"reasoning_efforts":[]}),
        models()[0].clone(),
    ] {
        let f = Fixture::new(
            "chat_completions",
            vec![done("DEFAULT"), done("SELECTED"), done("DEFAULT_AGAIN")],
        );
        let populated = entry
            .get("reasoning_efforts")
            .is_some_and(|v| v.as_array().unwrap().len() == 2);
        setup(&f, json!([entry]));
        let output = f.run(
            "first\n/thinking high\nsecond\n/thinking default\nthird\n/quit\n",
            &["--cli"],
        );
        good(&output);
        let requests = f.captured();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].get("reasoning_effort").is_none());
        assert_eq!(
            requests[1].get("reasoning_effort"),
            populated.then_some(&json!("high"))
        );
        assert!(requests[2].get("reasoning_effort").is_none());
        assert_eq!(saved(&f)["thinking"], "off");
        assert_metadata(&f);
    }
}

#[test]
fn roox_thinking_explicit_none_is_not_default_or_unadvertised() {
    let f = Fixture::new("chat_completions", vec![done("NONE"), done("DEFAULT")]);
    setup(&f, json!([models()[1].clone()]));
    good(&f.run(
        "/thinking none\nfirst\n/thinking default\nsecond\n/quit\n",
        &["--cli"],
    ));
    assert_eq!(f.captured()[0]["reasoning_effort"], "none");
    assert!(f.captured()[1].get("reasoning_effort").is_none());
    let f = Fixture::new("chat_completions", vec![done("NO_NONE")]);
    setup(&f, json!([models()[0].clone()]));
    let output = f.run("/thinking none\nfirst\n/quit\n", &["--cli"]);
    good(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Available: default, low, high"));
    assert!(f.captured()[0].get("reasoning_effort").is_none());
}

#[test]
fn roox_thinking_tools_compaction_and_resume_keep_effort_and_identity() {
    let f = Fixture::new(
        "chat_completions",
        vec![
            tool(),
            done("TOOLS_DONE"),
            done("SUMMARY"),
            done("AFTER_COMPACT"),
            done("RESUMED"),
        ],
    );
    setup(&f, json!([models()[0].clone()]));
    good(&f.run(
        "/thinking high\nwrite roox.txt\n/compact\nafter\n/quit\n",
        &["--cli"],
    ));
    assert_eq!(saved(&f)["thinking"], "high");
    good(&f.run("resume\n/quit\n", &["--cli", "--continue"]));
    let requests = f.captured();
    assert_eq!(requests.len(), 5);
    assert!(requests.iter().all(|r| r["reasoning_effort"] == "high"));
    assert!(requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "tool"));
    assert!(requests[2]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .starts_with("Summarize this conversation"));
    assert_eq!(
        fs::read_to_string(f.root.join("roox.txt")).unwrap(),
        "verified tool"
    );
    assert_metadata(&f);
}

#[test]
fn roox_thinking_resume_only_resets_after_authoritative_catalog_change() {
    for replacement in [
        json!([{"id":"a"}]),
        json!([{"id":"a","reasoning":true,"reasoning_efforts":["low"]}]),
        json!([]),
    ] {
        let f = Fixture::new("chat_completions", vec![done("SAVED")]);
        setup(&f, json!([models()[0].clone()]));
        good(&f.run("/thinking high\nseed conversation\n/quit\n", &["--cli"]));
        catalog(&f, 503, json!([]));
        good(&f.run("/quit\n", &["--cli", "--continue"]));
        assert_eq!(saved(&f)["thinking"], "high");
        catalog(&f, 200, replacement);
        let output = f.run("/quit\n", &["--cli", "--continue"]);
        good(&output);
        assert!(String::from_utf8_lossy(&output.stdout).contains("Thinking reset to Default"));
        assert_eq!(saved(&f)["thinking"], "off");
        let again = f.run("/quit\n", &["--cli", "--continue"]);
        good(&again);
        assert!(!String::from_utf8_lossy(&again.stdout).contains("Thinking reset"));
        assert_eq!(f.captured().len(), 1);
    }
}

#[test]
fn roox_thinking_actual_cli_selector_and_model_changes() {
    let f = Fixture::new("chat_completions", vec![done("HIGH_B"), done("DEFAULT_C")]);
    setup(&f, models());
    let mut tty = Pty::spawn(f.command().arg("--cli"));
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/thinking\r");
    tty.wait_text("3. high");
    tty.wait_text("Choose a number");
    tty.send(b"3\r");
    tty.wait_text("Thinking level: high.");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/models\r");
    tty.wait_text("Choose a number");
    tty.send(b"3\r");
    tty.wait_text("Selected rooxcode/b.");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"first\r");
    tty.wait_text("HIGH_B");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/models\r");
    tty.wait_text("Choose a number");
    tty.send(b"4\r");
    tty.wait_text("Thinking reset to Default");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/thinking\r");
    tty.wait_text("3. max");
    tty.wait_text("Choose a number");
    tty.send(b"\r");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"second\r");
    tty.wait_text("DEFAULT_C");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(shown.contains("1. Default  (current)"));
    let lists = shown
        .split("Thinking levels (current marked):")
        .skip(1)
        .collect::<Vec<_>>();
    assert_eq!(lists.len(), 2);
    assert!(!lists[0]
        .split("Choose a number")
        .next()
        .unwrap()
        .contains("none"));
    assert!(!lists[1]
        .split("Choose a number")
        .next()
        .unwrap()
        .contains("high"));
    assert_eq!(f.captured()[0]["reasoning_effort"], "high");
    assert!(f.captured()[1].get("reasoning_effort").is_none());
    assert_eq!(saved(&f)["thinking"], "off");
}

#[test]
fn roox_thinking_actual_tui_selector_and_refresh_reset_header() {
    let f = Fixture::new(
        "chat_completions",
        vec![done("TUI_HIGH"), done("TUI_DEFAULT")],
    );
    setup(&f, json!([models()[0].clone()]));
    let mut tty = Pty::spawn_with_size(&mut f.command(), true, 120, 32);
    tty.wait_text("Thinking   Default");
    tty.wait_text("Enter send");
    tty.send(b"/thinking\r");
    tty.wait_text("Thinking · Runtime model");
    tty.send(b"\x1b[B\x1b[B\r");
    tty.wait_text("Thinking   high");
    tty.wait_text("Enter send");
    tty.send(b"first\r");
    tty.wait_text("TUI_HIGH");
    tty.wait_text("Enter send");
    catalog(
        &f,
        200,
        json!([{"id":"a","reasoning":true,"reasoning_efforts":["low"]}]),
    );
    tty.send(b"/models\r");
    tty.wait_text("Models · all providers");
    tty.send(b"\x1b");
    tty.wait_text("Thinking   Default");
    tty.wait_text("Thinking reset to Default");
    tty.wait_text("Enter send");
    tty.send(b"second\r");
    tty.wait_text("TUI_DEFAULT");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let views = support::screen::snapshots(&shown);
    let picker = views
        .iter()
        .find(|v| v.contains("Thinking · Runtime model"))
        .unwrap();
    assert!(picker.contains("Default") && picker.contains("low") && picker.contains("high"));
    assert!(!picker.contains("medium") && !picker.contains("none"));
    assert_eq!(f.captured()[0]["reasoning_effort"], "high");
    assert!(f.captured()[1].get("reasoning_effort").is_none());
    assert_eq!(saved(&f)["thinking"], "off");
}

#[test]
fn roox_thinking_length_continuation_and_cancellation_preserve_selection() {
    let partial = (
        200,
        format!(
            "{}data: [DONE]\n\n",
            sse(json!({"choices":[{"delta":{"content":"PARTIAL"},"finish_reason":"length"}]}))
        ),
    );
    let f = Fixture::new(
        "chat_completions",
        vec![
            partial,
            (299, ": rooxcode waiting\n\n".into()),
            done("MANUAL_NEXT"),
        ],
    );
    setup(&f, json!([models()[0].clone()]));
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("Enter send");
    tty.send(b"/thinking high\r");
    tty.wait_text("Thinking   high");
    tty.wait_text("Enter send");
    tty.send(b"first\r");
    tty.wait_text("Rooxcode is busy. Waiting");
    let start = Instant::now();
    tty.send(b"\x1b");
    tty.wait_text("Stopped");
    assert!(start.elapsed() < Duration::from_secs(2));
    tty.wait_text("Enter send");
    assert_eq!(f.captured().len(), 2);
    tty.send(b"manual next\r");
    tty.wait_text("MANUAL_NEXT");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(f.captured().len(), 3);
    assert!(f.captured().iter().all(|r| r["reasoning_effort"] == "high"));
    assert_eq!(saved(&f)["thinking"], "high");
    assert_metadata(&f);
}

#[test]
fn non_roox_thinking_wire_behavior_is_unchanged() {
    for api in ["responses", "chat_completions", "anthropic"] {
        let reply = match api {
            "responses" => response("EXISTING"),
            "anthropic" => (
                200,
                format!(
                    "{}{}{}{}",
                    sse(
                        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})
                    ),
                    sse(
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"EXISTING"}})
                    ),
                    sse(
                        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}})
                    ),
                    sse(json!({"type":"message_stop"}))
                ),
            ),
            _ => done("EXISTING"),
        };
        let f = Fixture::new(api, vec![reply]);
        let path = f.root.join("app/config.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["providers"][0]["models"][0]["reasoning"] = json!(true);
        config["providers"][0]["models"][0]["reasoningEfforts"] = json!(["none", "max"]);
        fs::write(path, config.to_string()).unwrap();
        good(&f.run("/thinking high\nfirst\n/quit\n", &["--cli"]));
        let requests = f.captured();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].get("reasoning_effort").is_none());
        if api == "responses" {
            assert_eq!(requests[0]["reasoning"]["effort"], "high");
        } else {
            assert!(requests[0].get("reasoning").is_none());
        }
    }
}
