use super::*;

fn round(text: &str, finish: &str, calls: Value, usage: Value) -> (u16, String) {
    (
        200,
        format!(
            "{}data: [DONE]\n\n",
            sse(
                json!({"choices":[{"delta":{"content":text,"tool_calls":calls},"finish_reason":finish}],"usage":usage})
            )
        ),
    )
}

fn saved(f: &Fixture) -> String {
    let path = fs::read_dir(f.root.join("app/sessions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::read_to_string(path).unwrap()
}

#[test]
fn length_continuation_keeps_text_usage_context_and_provider_credentials() {
    for anonymous in [false, true] {
        let f = Fixture::new(
            "chat_completions",
            vec![
                round(
                    "Hello, ",
                    "length",
                    json!([]),
                    json!({"prompt_tokens":10,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":1}}),
                ),
                round(
                    "world.",
                    "stop",
                    json!([]),
                    json!({"prompt_tokens":15,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":2}}),
                ),
            ],
        );
        if anonymous {
            setup(&f, json!([{"id":"free"}]));
        }
        good(&f.run("", &["hello"]));
        let requests = f.captured();
        assert_eq!(requests.len(), 2);
        assert!(requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| { m["role"] == "assistant" && m["content"] == "Hello, " }));
        assert!(requests[1]["messages"]
            .to_string()
            .contains("Continue exactly where you stopped"));
        let data = saved(&f);
        let header: Value = serde_json::from_str(data.lines().next().unwrap()).unwrap();
        assert_eq!(
            header["usage"],
            json!({"input":25,"output":5,"cacheRead":3})
        );
        assert_eq!(header["context"], 18);
        assert!(data.contains("Hello, world."));
        if anonymous {
            assert_metadata(&f);
        } else {
            for request in post_headers(&f) {
                let fields = request["headers"].as_array().unwrap();
                assert!(fields
                    .iter()
                    .any(|h| h[0] == "authorization" && h[1] == "Bearer SYNTHETIC-KEY"));
                assert!(!fields.iter().any(|h| h[0] == "x-rooxcode-client"));
            }
        }
    }
}

#[test]
fn cut_tool_is_reissued_once_and_only_the_complete_call_executes() {
    for repeated_cut in [false, true] {
        let partial = json!([{"index":0,"id":"cut-call","function":{"name":"write","arguments":"{\"path\":\"never.txt\""}}]);
        let mut replies = vec![round("Preparing. ", "length", partial.clone(), Value::Null)];
        if repeated_cut {
            replies.push(round("", "length", partial, Value::Null));
        } else {
            replies.push(round("", "tool_calls", json!([{"index":0,"id":"complete-call","function":{"name":"write","arguments":json!({"path":"complete.txt","content":"written once"}).to_string()}}]), Value::Null));
            replies.push(done("TOOL_CONTINUED"));
        }
        let f = Fixture::new("chat_completions", replies);
        setup(&f, json!([{"id":"free"}]));
        let output = f.run("", &["write the file"]);
        assert!(!f.root.join("never.txt").exists());
        if repeated_cut {
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("tool call exceeded"));
            assert!(!f.root.join("complete.txt").exists());
            assert_eq!(f.captured().len(), 2);
        } else {
            good(&output);
            assert_eq!(
                fs::read_to_string(f.root.join("complete.txt")).unwrap(),
                "written once"
            );
            assert_eq!(f.captured().len(), 3);
            assert!(f.captured()[2]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["role"] == "tool" && m["tool_call_id"] == "complete-call"));
        }
        assert!(!f.captured()[1]["messages"].to_string().contains("cut-call"));
        assert!(f.captured()[1]["messages"]
            .to_string()
            .contains("Re-issue the complete tool call"));
        assert_metadata(&f);
    }
}

#[test]
fn text_continuation_stops_at_the_cap_with_a_visible_notice() {
    let f = Fixture::new(
        "chat_completions",
        (0..4)
            .map(|n| round(&format!("part{n} "), "length", json!([]), Value::Null))
            .collect(),
    );
    setup(&f, json!([{"id":"free"}]));
    let output = f.run("", &["continue the reply"]);
    good(&output);
    assert_eq!(f.captured().len(), 4);
    assert!(saved(&f).contains("part0 part1 part2 part3 "));
    assert!(saved(&f).contains("Reply cut at the output limit"));
    assert_metadata(&f);
}

#[test]
fn failed_continuation_preserves_the_prompt_without_committing_partial_text() {
    let f = Fixture::new(
        "chat_completions",
        vec![
            round("PARTIAL_NOT_COMMITTED", "length", json!([]), Value::Null),
            (400, "PRIVATE_UPSTREAM_ERROR".into()),
        ],
    );
    setup(&f, json!([{"id":"free"}]));
    let output = f.run("", &["PRESERVED_PROMPT"]);
    assert!(!output.status.success());
    assert_eq!(f.captured().len(), 2);
    let data = saved(&f);
    assert!(data.contains("PRESERVED_PROMPT"));
    assert!(!data.contains("PARTIAL_NOT_COMMITTED"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_UPSTREAM_ERROR"));
    assert_metadata(&f);
}

#[test]
fn escape_cancels_a_length_continuation_without_replay() {
    let f = Fixture::new(
        "chat_completions",
        vec![
            round("FIRST_PART", "length", json!([]), Value::Null),
            (299, ": rooxcode waiting\n\n".into()),
            done("MANUAL_AFTER_CANCEL"),
        ],
    );
    setup(&f, json!([{"id":"free"}]));
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("Enter send");
    tty.send(b"continue me\r");
    tty.wait_text("Rooxcode is busy. Waiting");
    let start = Instant::now();
    tty.send(b"\x1b");
    tty.wait_text("Stopped");
    assert!(start.elapsed() < Duration::from_secs(2));
    tty.wait_text("Enter send");
    assert_eq!(f.captured().len(), 2);
    tty.send(b"manual next\r");
    tty.wait_text("MANUAL_AFTER_CANCEL");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(f.captured().len(), 3);
    assert_metadata(&f);
}

#[test]
fn capped_compaction_summary_cannot_replace_the_original_history() {
    let mut replies = vec![done("ORIGINAL_REPLY")];
    replies.extend((0..4).map(|_| round("TRUNCATED_SUMMARY", "length", json!([]), Value::Null)));
    replies.push(done("AFTER_FAILED_COMPACT"));
    let f = Fixture::new("chat_completions", replies);
    setup(&f, json!([{"id":"free"}]));
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("Enter send");
    tty.send(b"ORIGINAL_PROMPT\r");
    tty.wait_text("ORIGINAL_REPLY");
    tty.wait_text("Enter send");
    tty.send(b"/compact\r");
    tty.wait_text("history unchanged.");
    tty.wait_text("Enter send");
    assert!(saved(&f).contains("ORIGINAL_REPLY"));
    assert!(!saved(&f).contains("TRUNCATED_SUMMARY"));
    let archives = f.root.join("app/archives");
    assert!(!archives.exists() || fs::read_dir(archives).unwrap().next().is_none());
    tty.send(b"next\r");
    tty.wait_text("AFTER_FAILED_COMPACT");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(f.captured().len(), 6);
    assert!(f.captured()[5]["messages"]
        .to_string()
        .contains("ORIGINAL_REPLY"));
    assert_metadata(&f);
}
