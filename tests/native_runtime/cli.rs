use super::*;

#[test]
fn explicit_cli_starts_inline_with_tui_hint_and_no_alternate_screen() {
    let f = Fixture::new("responses", vec![]);
    let mut command = f.command();
    command.arg("--cli").env_remove("HIBISCUS_TEST_NATIVE_KEY");
    let mut tty = Pty::spawn_with_color(&mut command, true);
    tty.wait_text(" · CLI");
    tty.wait_text("/tui");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("\x1b[?1049h"));
    assert!(shown.contains("\x1b[48;2;18;18;18m"));
    assert!(!shown.contains("\x1b[48;2;48;10;36m"));
    assert!(!shown.contains("\x1b]11;"));
    assert!(!shown.contains(['╭', '╮', '╰', '╯', '│', '█', '░']));
    assert!(shown.contains(&format!("{} · ", env!("CARGO_PKG_VERSION"))));
    assert!(shown.contains("no active model    /login"));
    assert!(shown.contains("/login"));
    assert!(f.captured().is_empty());
}

#[test]
fn cli_login_returns_inline_and_hides_login_tip_after_key_is_saved() {
    let f = Fixture::new("responses", vec![]);
    let mut command = f.command();
    command.arg("--cli").env_remove("HIBISCUS_TEST_NATIVE_KEY");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text(" · CLI");
    tty.send(b"/login\r");
    tty.wait_text("Sign in to Hibiscus");
    tty.send(b"\x1b[B\r");
    tty.wait_text("API Key · provider");
    tty.send(b"\r");
    tty.wait_text("Provider API key");
    tty.send(b"SYNTHETIC-CLI-SECRET\r");
    tty.wait_text(" · CLI");
    tty.wait_text("test/model");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let returned_cli = shown.rsplit("\x1b[?1049l").next().unwrap();
    assert!(returned_cli.contains("/tui"));
    assert!(!returned_cli.contains("/login"));
    assert!(!shown.contains("SYNTHETIC-CLI-SECRET"));
    assert!(f.captured().is_empty());
}

#[test]
fn dumb_cli_keeps_plain_input_without_editor_protocol_sequences() {
    let f = Fixture::new("responses", vec![]);
    let mut process = f.command();
    process.arg("--cli").env("TERM", "dumb");
    let mut tty = Pty::spawn(&mut process);
    tty.wait_text("/tui");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains('\x1b'));
    assert!(f.captured().is_empty());
}

#[test]
fn cli_multiline_keys_edit_one_draft_and_submit_one_prompt() {
    for color in [false, true] {
        let f = Fixture::new("responses", vec![response("MULTILINE_DONE")]);
        let mut process = f.command();
        process.arg("--cli");
        let mut tty = Pty::spawn_with_color(&mut process, color);
        tty.wait_text("\x1b[?2004h");
        tty.send(b"alpha\x1b\rsecond\nthird");
        tty.wait_text("third");
        assert!(f.captured().is_empty());
        tty.send(b"\x1b[A\x1b[F!\x1b[B\x1b[F\r");
        tty.wait_text("MULTILINE_DONE");
        tty.wait_text("\x1b[?2004h");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        let requests = f.captured();
        assert_eq!(requests.len(), 1);
        let users = requests[0]["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "user")
            .collect::<Vec<_>>();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0]["content"][0]["text"], "alpha\nsecond!\nthird");
        let views = support::screen::snapshots(&format!("{shown}\x1b[?2026l"));
        assert!(views
            .last()
            .unwrap()
            .contains("❯ alpha\n  second!\n  third"));
        assert!(!shown.contains("\x1b[?1049h"));
        if !color {
            assert!(!shown.contains("\x1b[38;") && !shown.contains("\x1b[48;"));
        }
    }
}

#[test]
fn cli_bracketed_paste_keeps_unicode_whitespace_and_embedded_commands_until_enter() {
    let f = Fixture::new("responses", vec![response("PASTE_DONE")]);
    let mut process = f.command();
    process.arg("--cli");
    let mut tty = Pty::spawn_with_size(&mut process, true, 24, 14);
    tty.wait_text("\x1b[?2004h");
    let raw = "\r\n  café 世界\r\n/quit\r\n\tkeep this line\r\n\r\n\r\n";
    tty.send(format!("\x1b[200~{raw}\x1b[201~").as_bytes());
    tty.wait_text("rows above");
    assert!(f.captured().is_empty());
    tty.send(b"\r");
    tty.wait_text("PASTE_DONE");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/quit\r");
    tty.finish();
    let requests = f.captured();
    assert_eq!(requests.len(), 1);
    let user = requests[0]["input"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .unwrap();
    assert_eq!(
        user["content"][0]["text"],
        "\n  café 世界\n/quit\n    keep this line\n\n\n"
    );
}

#[test]
fn cli_rejected_paste_keeps_the_existing_draft_and_never_submits() {
    let f = Fixture::new("responses", vec![response("KEPT_DRAFT")]);
    let mut process = f.command();
    process.arg("--cli");
    let mut tty = Pty::spawn(&mut process);
    tty.wait_text("\x1b[?2004h");
    tty.send(b"keep\x1b[200~bad\xff\n/quit\x1b[201~");
    tty.wait_text("Paste rejected");
    assert!(f.captured().is_empty());
    tty.send(b"\r");
    tty.wait_text("KEPT_DRAFT");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/quit\r");
    tty.finish();
    let requests = f.captured();
    assert_eq!(requests.len(), 1);
    let user = requests[0]["input"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .unwrap();
    assert_eq!(user["content"][0]["text"], "keep");
}

#[test]
fn cli_waiting_row_is_static_and_clears_when_a_run_is_cancelled() {
    let f = Fixture::new("responses", vec![(299, ": stream open\n\n".into())]);
    let mut process = f.command();
    process.arg("--cli");
    let mut tty = Pty::spawn_with_color(&mut process, true);
    tty.wait_text("/tui");
    tty.send(b"wait for the provider\r");
    tty.wait_text("Waiting for response · Esc to stop");
    let deadline = Instant::now() + Duration::from_secs(3);
    while f.captured().is_empty() && Instant::now() < deadline {
        tty.pump();
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(f.captured().len(), 1);
    tty.send(b"\x1b");
    tty.wait_text("Stopped");
    tty.wait_text("❯");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert_eq!(shown.matches("Waiting for response").count(), 1);
    let tail = shown.split("Waiting for response").nth(1).unwrap();
    assert!(tail.find("\r\x1b[2K").unwrap() < tail.find("Stopped").unwrap());
}

#[test]
fn cli_tool_rows_show_real_success_failure_and_commands_without_ansi_when_disabled() {
    for color in [true, false] {
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![
                    ("write", json!({"path":"one.txt","content":"one\n"})),
                    ("read", json!({"path":"one.txt"})),
                    ("read", json!({"path":"one.txt"})),
                    (
                        "bash",
                        json!({"command":"printf 'ACTUAL_FAILURE\\n============\\n\\n\\nFAILURE_CONTEXT\\n' >&2; exit 3"}),
                    ),
                    ("bash", json!({"command":"printf 'ACTUAL_SUCCESS\\n'"})),
                ]),
                response("The failed command returned 3; the next command succeeded. ROWS_DONE"),
            ],
        );
        let mut command = f.command();
        command.arg("--cli");
        let mut tty = Pty::spawn_with_color(&mut command, color);
        tty.wait_text(" · CLI");
        tty.send(b"run the local tools\r");
        tty.wait_text("ROWS_DONE");
        tty.wait_text("❯");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        assert!(shown.contains("Read  one.txt, one.txt"));
        assert!(shown.contains("printf 'ACTUAL_FAILURE"));
        assert!(shown.contains("      ACTUAL_FAILURE\r\n"));
        assert!(shown.contains("      ACTUAL_SUCCESS\r\n"));
        assert!(shown.contains("      FAILURE_CONTEXT\r\n"));
        assert!(!shown.contains("      ============\r\n"));
        assert!(shown.contains("Shell exited with exit status: 3"));
        assert!(shown.contains("exit 0 · ") && shown.contains("exit 3 · "));
        assert!(!shown.contains("done ·") && !shown.contains("↳"));
        assert_eq!(
            shown.matches("hibi\n").count()
                + shown.matches("hibi\r\n").count()
                + shown.matches("hibi\x1b[0m").count(),
            1
        );
        if color {
            assert!(shown.contains("\x1b[48;2;24;57;37m"));
            assert!(shown.contains("\x1b[48;2;72;28;34m"));
        } else {
            assert!(!shown
                .replace("\x1b[?2004h", "")
                .replace("\x1b[?2004l", "")
                .contains('\x1b'));
        }
        let request = f.captured();
        assert_eq!(request.len(), 2);
        let outputs = request[1]["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .collect::<Vec<_>>();
        assert_eq!(outputs.len(), 5);
        assert!(outputs[3]["output"]
            .as_str()
            .unwrap()
            .contains("ACTUAL_FAILURE"));
        assert!(outputs[3]["output"]
            .as_str()
            .unwrap()
            .contains("============\n\n\nFAILURE_CONTEXT"));
        let session = fs::read_dir(f.root.join("app/sessions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let records = fs::read_to_string(session)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let failure = records
            .iter()
            .find(|record| record["message"]["isError"] == true)
            .unwrap();
        assert_eq!(failure["message"]["details"]["exitCode"], 3);
    }
}

#[test]
fn cli_streamed_and_edit_diffs_use_red_green_and_respect_no_color() {
    let text = "Here is the patch:\n```diff\ndiff --git a/demo.txt b/demo.txt\n--- a/demo.txt\n+++ b/demo.txt\n@@ -1 +1 @@\n-old\n+new\n```\nPATCH_DONE";
    let mut body = text.as_bytes().chunks(3).map(|part| sse(json!({"type":"response.output_text.delta","delta":std::str::from_utf8(part).unwrap()}))).collect::<String>();
    body.push_str(&sse(json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}]}})));
    for color in [true, false] {
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![
                    ("write", json!({"path":"demo.txt","content":"old\n"})),
                    (
                        "edit",
                        json!({"path":"demo.txt","oldText":"old","newText":"new"}),
                    ),
                ]),
                (200, body.clone()),
            ],
        );
        let mut command = f.command();
        command.arg("--cli");
        let mut tty = Pty::spawn_with_color(&mut command, color);
        tty.wait_text(" · CLI");
        tty.send(b"apply the patch\r");
        tty.wait_text("PATCH_DONE");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        if color {
            assert!(shown.contains("\x1b[48;2;18;18;18m"));
            assert!(shown.contains("\x1b[38;2;110;210;135m+new"));
            assert!(shown.contains("\x1b[38;2;155;155;155m   1 \x1b[38;2;110;210;135m+new"));
            assert!(shown.contains("\x1b[38;2;245;110;115m-old"));
            assert!(shown.contains("\x1b[38;2;155;155;155m   1 \x1b[38;2;245;110;115m-old"));
        } else {
            assert!(!shown
                .replace("\x1b[?2004h", "")
                .replace("\x1b[?2004l", "")
                .contains('\x1b'));
        }
        assert_eq!(
            fs::read_to_string(f.root.join("demo.txt")).unwrap(),
            "new\n"
        );
        let session = fs::read_dir(f.root.join("app/sessions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(!fs::read_to_string(session).unwrap().contains("\\u001b"));
    }
}

#[test]
fn thinking_confirmation_uses_native_wording_without_provider_requests() {
    let f = Fixture::new("responses", vec![]);
    let path = f.root.join("app/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["providers"][0]["models"][0]["reasoning"] = json!(true);
    fs::write(path, config.to_string()).unwrap();
    let output = f.run("/thinking high\n/quit\n", &[]);
    good(&output);
    let shown = String::from_utf8_lossy(&output.stdout);
    assert!(shown.contains("Thinking level: high."));
    assert!(!shown.contains("(Pi)"));
    assert!(f.captured().is_empty());
}
