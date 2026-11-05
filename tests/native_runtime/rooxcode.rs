use super::*;
#[path = "rooxcode/continuation.rs"]
mod continuation;
#[path = "rooxcode/queue.rs"]
mod queue;
#[path = "rooxcode/thinking.rs"]
mod thinking;

fn done(text: &str) -> (u16, String) {
    (
        200,
        format!(
            "{}{}data: [DONE]\n\n",
            sse(json!({"choices":[{"delta":{"content":text},"finish_reason":null}]})),
            sse(json!({"choices":[{"delta":{},"finish_reason":"stop"}]}))
        ),
    )
}
fn tool() -> (u16, String) {
    (
        200,
        format!(
            "{}{}data: [DONE]\n\n",
            sse(
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"roox-call","function":{"name":"write","arguments":json!({"path":"roox.txt","content":"verified tool"}).to_string()}}]},"finish_reason":null}]})
            ),
            sse(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}))
        ),
    )
}
fn setup(f: &Fixture, models: Value) -> Vec<u8> {
    let path = f.root.join("app/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let provider = config["providers"][1].clone();
    assert_eq!(provider["id"], "rooxcode");
    config["providers"] = json!([provider]);
    config.as_object_mut().unwrap().remove("model");
    fs::write(&path, config.to_string()).unwrap();
    fs::write(
        f.root.join("rooxcode-catalog.json"),
        json!({"status":200,"body":{"object":"list","data":models}}).to_string(),
    )
    .unwrap();
    fs::read(path).unwrap()
}
fn post_headers(f: &Fixture) -> Vec<Value> {
    f.headers
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r["route"].as_str().unwrap().starts_with("POST "))
        .cloned()
        .collect()
}
fn header<'a>(record: &'a Value, name: &str) -> &'a str {
    let values = record["headers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p[0] == name)
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 1, "missing/duplicated {name}");
    values[0][1].as_str().unwrap()
}
fn assert_metadata(f: &Fixture) -> String {
    let requests = post_headers(f);
    assert!(!requests.is_empty());
    let id = header(&requests[0], "x-rooxcode-session").to_owned();
    assert_eq!(id.len(), 36);
    assert_ne!(id, "00000000-0000-0000-0000-000000000000");
    for record in f.headers.lock().unwrap().iter() {
        assert!(!record["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p[0] == "authorization" || p[0] == "x-api-key" || p[0] == "cookie"));
    }
    for record in requests {
        assert_eq!(header(&record, "x-rooxcode-client"), "hibiscus");
        assert_eq!(
            header(&record, "x-rooxcode-version"),
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(header(&record, "x-rooxcode-session"), id);
        assert!(record["route"]
            .as_str()
            .unwrap()
            .starts_with("POST /v1/chat/completions "));
    }
    id
}

#[test]
fn rooxcode_no_key_discovery_streaming_tools_and_credentials_are_isolated() {
    let f = Fixture::new("chat_completions", vec![tool(), done("ROOX_COMPLETE")]);
    let path = f.root.join("app/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["codexContextWindow"] = json!(1_000_000);
    fs::write(&path, config.to_string()).unwrap();
    let before = setup(&f, json!([{"id":"dynamic-first"},{"id":"operator/second"}]));
    use std::os::unix::fs::PermissionsExt;
    let auth = f.root.join("app/auth.json");
    fs::write(
        &auth,
        json!({"rooxcode":"SYNTHETIC-ROOX-KEY","openai":"SYNTHETIC-OTHER-KEY"}).to_string(),
    )
    .unwrap();
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o600)).unwrap();
    let output = f
        .command()
        .env("OPENAI_API_KEY", "SYNTHETIC-ENV-KEY")
        .env("ANTHROPIC_API_KEY", "SYNTHETIC-ANTHROPIC")
        .arg("use tools")
        .output()
        .unwrap();
    good(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("ROOX_COMPLETE"));
    assert_eq!(
        fs::read_to_string(f.root.join("roox.txt")).unwrap(),
        "verified tool"
    );
    let requests = f.captured();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["model"], "dynamic-first");
    assert!(requests[0].get("max_tokens").is_none());
    assert!(requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "tool" && m["tool_call_id"] == "roox-call"));
    assert_metadata(&f);
    assert_eq!(fs::read(f.root.join("app/config.json")).unwrap(), before);
    let shown = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(!shown.contains("SYNTHETIC-"));
}

#[test]
fn rooxcode_session_uuid_is_reused_on_resume_and_changed_for_new_conversations() {
    let f = Fixture::new(
        "chat_completions",
        vec![done("ONE"), done("TWO"), done("THREE")],
    );
    setup(&f, json!([{"id":"live-model"}]));
    good(&f.run("", &["first"]));
    let first = assert_metadata(&f);
    good(&f.run("second\n/quit\n", &["--continue"]));
    assert_eq!(assert_metadata(&f), first);
    good(&f.run("", &["third"]));
    let records = post_headers(&f);
    assert_ne!(header(&records[2], "x-rooxcode-session"), first);
    for path in fs::read_dir(f.root.join("app/sessions")).unwrap() {
        let text = fs::read_to_string(path.unwrap().path()).unwrap();
        let head: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(head["rooxcodeSession"].as_str().unwrap().len(), 36);
    }
}

#[test]
fn rooxcode_multiple_turns_reuse_uuid_and_new_command_rotates_it() {
    let f = Fixture::new("chat_completions", vec![done("A"), done("B"), done("C")]);
    setup(&f, json!([{"id":"free"}]));
    good(&f.run("first\nsecond\n/new\nthird\n/quit\n", &["--cli"]));
    let records = post_headers(&f);
    assert_eq!(records.len(), 3);
    assert_eq!(
        header(&records[0], "x-rooxcode-session"),
        header(&records[1], "x-rooxcode-session")
    );
    assert_ne!(
        header(&records[1], "x-rooxcode-session"),
        header(&records[2], "x-rooxcode-session")
    );
}

#[test]
fn rooxcode_empty_catalog_and_service_errors_do_not_retry_or_fallback() {
    let empty = Fixture::new("chat_completions", vec![]);
    setup(&empty, json!([]));
    let output = empty.run("", &["hello"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no available models"));
    assert!(empty.captured().is_empty());
    for (status, code, expected) in [
        (403, "hibiscus_client_required", "metadata"),
        (404, "model_not_found", "selected model is unavailable"),
        (429, "network_limit", "Retry-After: 7"),
        (503, "inference_disabled", "disabled by the operator"),
        (502, "upstream_failure", "upstream provider is unavailable"),
    ] {
        let f = Fixture::new(
            "chat_completions",
            vec![(
                status,
                json!({"error":{"code":code,"message":"PRIVATE_ERROR_BODY"}}).to_string(),
            )],
        );
        setup(&f, json!([{"id":"live-model"}]));
        fs::write(f.root.join("rooxcode-retry-after"), "7").unwrap();
        let output = f.run("", &["hello"]);
        assert!(!output.status.success());
        let shown = String::from_utf8_lossy(&output.stderr).to_string()
            + &String::from_utf8_lossy(&output.stdout);
        assert!(shown.contains(expected), "{status}: {shown}");
        assert!(!shown.contains("PRIVATE_ERROR_BODY"));
        assert_eq!(f.captured().len(), 1);
        assert_metadata(&f);
    }
}

#[test]
fn rooxcode_picker_needs_no_login_and_cancellation_does_not_execute_partial_tools() {
    let (status, body) = tool();
    assert_eq!(status, 200);
    let partial = body.split("data: [DONE]").next().unwrap().to_owned();
    let prefix =
        sse(json!({"choices":[{"delta":{"content":"WAIT_FOR_CANCEL"},"finish_reason":null}]}));
    let f = Fixture::new(
        "chat_completions",
        vec![(299, prefix + &partial), done("AFTER_CANCEL")],
    );
    setup(&f, json!([{"id":"free"},{"id":"second"}]));
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("rooxcode/free");
    tty.wait_text("Enter send");
    tty.send(b"/models\r");
    tty.wait_text("Models");
    tty.wait_text("second");
    tty.send(b"\x1b");
    tty.wait_text("Enter send");
    tty.send(b"start stream\r");
    tty.wait_text("WAIT_FOR_CANCEL");
    tty.send(b"\x1b");
    tty.wait_text("Stopped");
    tty.wait_text("Enter send");
    assert!(!f.root.join("roox.txt").exists());
    tty.send(b"continue\r");
    tty.wait_text("AFTER_CANCEL");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(f.captured().len(), 2);
    assert_metadata(&f);
}

#[test]
fn rooxcode_discovery_failure_and_removed_saved_model_do_not_switch_providers() {
    for body in [
        json!({"data":null}),
        json!({"data":[{"id":"duplicate"},{"id":"duplicate"}]}),
        json!({"error":{"code":"upstream_failure","message":"PRIVATE_CATALOG"}}),
    ] {
        let f = Fixture::new("chat_completions", vec![]);
        setup(&f, json!([]));
        let status = if body.get("error").is_some() {
            503
        } else {
            200
        };
        fs::write(
            f.root.join("rooxcode-catalog.json"),
            json!({"status":status,"body":body}).to_string(),
        )
        .unwrap();
        let output = f.run("", &["hello"]);
        assert!(!output.status.success());
        assert!(f.captured().is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_CATALOG"));
    }
    let f = Fixture::new("chat_completions", vec![done("ORIGINAL")]);
    setup(&f, json!([{"id":"first"}]));
    good(&f.run("", &["hello"]));
    fs::write(
        f.root.join("rooxcode-catalog.json"),
        json!({"status":200,"body":{"data":[{"id":"replacement"}]}}).to_string(),
    )
    .unwrap();
    let output = f.run("try again\n", &["--continue"]);
    assert!(String::from_utf8_lossy(&output.stderr).contains("selected model is unavailable"));
    assert_eq!(f.captured().len(), 1);
}

#[test]
fn rooxcode_cli_access_legacy_resume_and_compaction_keep_the_session_uuid() {
    let f = Fixture::new(
        "chat_completions",
        vec![done("INITIAL"), done("SUMMARY"), done("CONTINUED")],
    );
    setup(&f, json!([{"id":"free"}]));
    good(&f.run("", &["hello"]));
    let path = fs::read_dir(f.root.join("app/sessions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = fs::read_to_string(&path).unwrap();
    let mut rows = text
        .lines()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    // Older Hibiscus sessions have a hexadecimal ID and no Rooxcode UUID.
    rows[0].as_object_mut().unwrap().remove("rooxcodeSession");
    fs::write(
        path,
        rows.iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    let mut command = f.command();
    command.args(["--cli", "--continue"]);
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("rooxcode/free");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/compact\r");
    tty.wait_text("Runtime compacted context");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"continue\r");
    tty.wait_text("CONTINUED");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/quit\r");
    tty.finish();
    let headers = post_headers(&f);
    assert_eq!(headers.len(), 3);
    assert_eq!(
        header(&headers[1], "x-rooxcode-session"),
        header(&headers[2], "x-rooxcode-session")
    );
    assert_ne!(
        header(&headers[0], "x-rooxcode-session"),
        header(&headers[1], "x-rooxcode-session")
    );
}

#[test]
fn existing_keyed_provider_keeps_its_selection_and_never_gets_rooxcode_headers() {
    let f = Fixture::new("responses", vec![response("EXISTING_PROVIDER")]);
    let before = fs::read(f.root.join("app/config.json")).unwrap();
    good(&f.run("", &["hello"]));
    assert_eq!(f.headers.lock().unwrap().len(), 1);
    let headers = post_headers(&f);
    assert_eq!(header(&headers[0], "authorization"), "Bearer SYNTHETIC-KEY");
    assert!(!headers[0]["headers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h[0].as_str().unwrap().starts_with("x-rooxcode-")));
    assert_eq!(f.captured()[0]["model"], "model");
    assert_eq!(fs::read(f.root.join("app/config.json")).unwrap(), before);
}

#[test]
fn default_rooxcode_never_implicitly_uses_a_configured_paid_provider_key() {
    for available in [true, false] {
        let f = Fixture::new("responses", vec![done("FREE_DEFAULT")]);
        let path = f.root.join("app/config.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config.as_object_mut().unwrap().remove("model");
        fs::write(path, config.to_string()).unwrap();
        fs::write(f.root.join("rooxcode-catalog.json"),json!({"status":200,"body":{"data":if available {json!([{"id":"free"}])} else {json!([])}}}).to_string()).unwrap();
        let output = f.run("", &["hello"]);
        if available {
            good(&output);
            assert_eq!(f.captured()[0]["model"], "free");
            assert_metadata(&f);
        } else {
            assert!(!output.status.success());
            assert!(f.captured().is_empty());
        }
    }
}

#[test]
fn rooxcode_mid_stream_errors_do_not_commit_or_execute_tool_calls() {
    let (_, body) = tool();
    let partial = body.split("data: [DONE]").next().unwrap();
    let f = Fixture::new(
        "chat_completions",
        vec![(
            200,
            partial.to_owned()
                + &sse(
                    json!({"error":{"code":"upstream_failure","message":"PRIVATE_STREAM_FAILURE"}}),
                ),
        )],
    );
    setup(&f, json!([{"id":"free"}]));
    let output = f.run("", &["hello"]);
    assert!(!output.status.success());
    assert!(!f.root.join("roox.txt").exists());
    assert_eq!(f.captured().len(), 1);
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_STREAM_FAILURE"));
}

#[test]
fn rooxcode_compacts_large_tool_history_in_bounded_requests_and_continues() {
    let read = (
        200,
        format!(
            "{}{}data: [DONE]\n\n",
            sse(
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"large-read","function":{"name":"read","arguments":json!({"path":"large.txt"}).to_string()}}]},"finish_reason":null}]})
            ),
            sse(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}))
        ),
    );
    let mut replies = vec![read];
    replies.extend((0..20).map(|_| {
        done(
            "Keep reading the requested project. The completed read is recorded. CONTEXT_CONTINUED",
        )
    }));
    let f = Fixture::new("chat_completions", replies);
    setup(
        &f,
        json!([{"id":"bounded-model","reasoning":true,"reasoning_efforts":["high"],"context_window":20000,"max_request_bytes":10485760,"max_messages":256}]),
    );
    fs::write(
        f.root.join("large.txt"),
        "verified file content\n".repeat(1400),
    )
    .unwrap();
    let output = f.run(
        "/thinking high\nRead large.txt and then continue.\n/quit\n",
        &["--cli"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("CONTEXT_CONTINUED"));
    let requests = f.requests.lock().unwrap();
    let summaries = requests
        .iter()
        .filter(|r| {
            r["messages"][0]["content"]
                .as_str()
                .is_some_and(|s| s.starts_with("Summarize this conversation"))
        })
        .collect::<Vec<_>>();
    assert!(
        summaries.len() > 1,
        "expected bounded summary chunks, got {}",
        summaries.len()
    );
    for request in &*requests {
        assert_eq!(request["reasoning_effort"], "high");
        assert!(serde_json::to_vec(request).unwrap().len() < 12723);
    }
    assert!(requests.last().unwrap()["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["content"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v["text"]
                .as_str()
                .is_some_and(|t| t.contains("Previous conversation summary"))))));
    drop(requests);
    assert_metadata(&f);
}

#[test]
fn rooxcode_failed_compaction_keeps_original_prompt_and_tool_history() {
    let read = (
        200,
        format!(
            "{}{}data: [DONE]\n\n",
            sse(
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"read-once","function":{"name":"read","arguments":json!({"path":"large.txt"}).to_string()}}]},"finish_reason":null}]})
            ),
            sse(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}))
        ),
    );
    let f = Fixture::new(
        "chat_completions",
        vec![
            read,
            (
                413,
                json!({"error":{"code":"request_too_large","message":"PRIVATE-BODY"}}).to_string(),
            ),
        ],
    );
    setup(
        &f,
        json!([{"id":"bounded-model","context_window":20000,"max_request_bytes":10485760}]),
    );
    fs::write(
        f.root.join("large.txt"),
        "original file content\n".repeat(1400),
    )
    .unwrap();
    let output = f.run("Read large.txt and continue.", &[]);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Compact") && !error.contains("PRIVATE-BODY"));
    assert_eq!(f.requests.lock().unwrap().len(), 2);
    let sessions = fs::read_dir(f.root.join("app/sessions"))
        .unwrap()
        .map(|p| fs::read_to_string(p.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].contains("Read large.txt and continue."));
    assert_eq!(
        sessions[0]
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line)
                .ok()
                .is_some_and(|v| v["message"]["role"] == "toolResult"))
            .count(),
        1
    );
    assert!(!sessions[0].contains("Previous conversation summary"));
}
