use super::*;

#[test]
fn native_api_key_login_and_logout_are_masked_and_local() {
    let f = Fixture::new("responses", vec![]);
    let mut command = f.command();
    command.env_remove("HIBISCUS_TEST_NATIVE_KEY");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("no active model");
    tty.wait_text("Enter send");
    tty.send(b"/login\r");
    tty.wait_text("Sign in to Hibiscus");
    tty.send(b"\x1b[B\r");
    tty.wait_text("API Key · provider");
    tty.send(b"\r");
    tty.wait_text("Provider API key");
    tty.send(b"FAKE-PRIVATE-CREDENTIAL\r");
    tty.wait_text("sign-in completed");
    tty.wait_text("test/model");
    tty.wait_text("Enter send");
    let auth = f.root.join("app/auth.json");
    assert!(fs::read_to_string(&auth)
        .unwrap()
        .contains("FAKE-PRIVATE-CREDENTIAL"));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
        0o600
    );
    tty.send(b"/logout\r");
    tty.wait_text("Stored credentials");
    tty.send(b"\r");
    tty.wait_text("Remove stored credential");
    tty.send(b"\x1b[B\r");
    tty.wait_text("Logout complete");
    tty.wait_text("no active model");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("FAKE-PRIVATE-CREDENTIAL"));
    let views = support::screen::snapshots(&shown);
    assert!(views.last().unwrap().contains("no active model"));
    assert!(!views.last().unwrap().contains("test/model"));
    assert_eq!(fs::read_to_string(&auth).unwrap(), "{}");
}

#[test]
fn first_launch_without_credentials_shows_welcome_and_empty_logout_keeps_no_active_model() {
    let f = Fixture::new("responses", vec![]);
    let config_path = f.root.join("app/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config.as_object_mut().unwrap().remove("model");
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut command = f.command();
    command.env_remove("HIBISCUS_TEST_NATIVE_KEY");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter to send · Esc to stop");
    tty.wait_text("no active model");
    tty.resize(42, 24);
    tty.send(b"a");
    tty.wait_text("/login");
    tty.send(b"\x7f/logout\r");
    tty.wait_text("No stored credentials");
    tty.wait_text("no active model");
    tty.send(b"/new\r");
    tty.wait_text("Enter to send · Esc to stop");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let views = support::screen::snapshots(&shown);
    assert!(views
        .iter()
        .any(|v| v.contains("Enter to send · Esc to stop")));
    assert!(!views.iter().any(|v| v.contains("test/model")));
    assert!(f.captured().is_empty());
    assert!(!f.root.join("app/auth.json").exists());
}

#[test]
fn empty_logout_keeps_environment_authenticated_model_available() {
    let f = Fixture::new("responses", vec![]);
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("test/model");
    tty.send(b"/logout\r");
    tty.wait_text("No stored credentials");
    tty.wait_text("test/model");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(support::screen::snapshots(&shown)
        .last()
        .unwrap()
        .contains("test/model"));
    assert!(f.captured().is_empty());
}
#[test]
fn fullscreen_model_picker_stays_searchable_without_chat_confirmation() {
    let f = Fixture::new("responses", vec![]);
    let mut command = f.command();
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"/models\r");
    tty.wait_text("Models · all providers");
    tty.send(b"absent");
    tty.wait_text("No matching models");
    tty.send(b"\x1b");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("Already using test/model"));
    assert!(!shown.contains("Selected test/model"));
}

#[test]
fn idle_tips_disappear_after_chat_while_the_hibi_header_stays_out_of_history() {
    let f = Fixture::new("responses", vec![response("WELCOME_CHAT_COMPLETE")]);
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("Enter to send · Esc to stop");
    tty.send(b"hello\r");
    tty.wait_text("WELCOME_CHAT_COMPLETE");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let views = support::screen::snapshots(&shown);
    let view = views.last().unwrap();
    assert!(!view.contains("Enter to send · Esc to stop"));
    let header = view.lines().take(8).collect::<Vec<_>>().join(" ");
    assert!(header.contains("Hibiscus"));
    assert!(header.contains("Thinking"));
    assert!(view.contains("WELCOME_CHAT_COMPLETE"));
    assert!(!view.contains("✿ hibiscus"));
    for file in fs::read_dir(f.root.join("app/sessions")).unwrap() {
        let history = fs::read_to_string(file.unwrap().path()).unwrap();
        assert!(history.contains("WELCOME_CHAT_COMPLETE"));
        assert!(!history.contains("Enter to send · Esc to stop"));
        assert!(!history.contains("Sign in /login"));
        assert!(!history.contains("› Hibiscus"));
        assert!(!history.contains("Thinking   off"));
    }
}

#[test]
fn browser_login_menu_cancels_locally_without_starting_auth() {
    let f = Fixture::new("responses", vec![]);
    let mut command = f.command();
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"/login\r");
    tty.wait_text("Sign in to Hibiscus");
    tty.send(b"\r");
    tty.wait_text("Codex · ChatGPT account");
    tty.send(b"\x1b");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let _ = tty.finish();
    assert!(!f.root.join("app/codex-auth.json").exists());
    assert!(f.captured().is_empty());
}

#[test]
fn queued_followup_is_delivered_once_before_settlement() {
    let (_, slow) = response("FIRST_STREAM");
    let f = Fixture::new("responses", vec![(298, slow), response("FOLLOWUP_DONE")]);
    let mut c = f.command();
    let mut tty = Pty::spawn(&mut c);
    tty.wait_text("Enter send");
    tty.send(b"first\r");
    tty.wait_text("FIRST_STREAM");
    tty.send(b"follow up\x11");
    tty.wait_text("FOLLOWUP_DONE");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    let r = f.captured();
    assert_eq!(r.len(), 2);
    assert_eq!(
        r[1]["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["role"] == "user" && v["content"].to_string().contains("follow up"))
            .count(),
        1
    );
}
#[test]
fn failed_run_returns_undelivered_queued_input_for_review() {
    let failed = format!(
        "{}{}",
        sse(json!({"type":"response.output_text.delta","delta":"WAIT_FOR_FAILURE"})),
        sse(json!({"type":"response.failed","response":{"status":"failed"}}))
    );
    let f = Fixture::new("responses", vec![(298, failed)]);
    let mut c = f.command();
    let mut tty = Pty::spawn(&mut c);
    tty.wait_text("Enter send");
    tty.send(b"initial\r");
    tty.wait_text("WAIT_FOR_FAILURE");
    tty.send(b"native queued text\x11");
    tty.wait_text("rejected queued");
    tty.wait_text("native queued text");
    tty.send(b"\x03/quit\r");
    tty.finish();
    assert_eq!(f.captured().len(), 1);
}

#[test]
fn persistent_header_tracks_thinking_across_chat_scroll_new_resume_and_small_model_picker() {
    for color in [false, true] {
        let reply = (0..60)
            .map(|row| format!("HEADER_ROW_{row:02}\n"))
            .collect::<String>()
            + "HEADER_CHAT_END";
        let f = Fixture::new("responses", vec![response(&reply)]);
        let path = f.root.join("app/config.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["providers"][0]["models"][0]["reasoning"] = json!(true);
        fs::write(path, config.to_string()).unwrap();
        let mut tty = Pty::spawn_with_size(&mut f.command(), color, 120, 32);
        tty.wait_text("Enter send");
        tty.send(b"/thinking high\r");
        tty.wait_text("Thinking   high");
        tty.wait_text("Enter send");
        tty.send(b"hello persistent header\r");
        tty.wait_text("HEADER_CHAT_END");
        tty.wait_text("Enter send");
        tty.send(b"\x1b[5~\x1b[5~\x1b[5~\x1b[5~");
        tty.wait_text("HEADER_ROW_00");
        tty.send(b"/new\r");
        tty.wait_text("Thinking   off");
        tty.wait_text("Enter to send · Esc to stop");
        tty.send(b"/continue\r");
        tty.wait_text("Thinking   high");
        tty.wait_text("HEADER_CHAT_END");
        tty.wait_text("Enter send");
        tty.resize(42, 14);
        tty.send(b"/models\r");
        tty.wait_text("Models · all providers");
        tty.wait_text("Search models");
        tty.wait_text("HIBI · Thinking high");
        tty.send(b"absent");
        tty.wait_text("No matching models");
        tty.send(b"\x1b");
        tty.wait_text("Enter send");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        let views = support::screen::snapshots(&shown);
        for marker in ["HEADER_CHAT_END", "HEADER_ROW_00"] {
            assert!(
                views.iter().any(|view| {
                    let header = view.lines().take(8).collect::<Vec<_>>().join(" ");
                    view.contains(marker)
                        && header.contains("Hibiscus")
                        && header
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .contains("Thinking high")
                }),
                "banner missing while showing {marker}"
            );
        }
        assert!(!views.iter().any(|view| view.contains("✿ hibiscus")));
        assert!(!shown.contains("(Pi)"));
        assert!(!views.iter().any(|view| view.contains("Thinking level:")));
        let requests = f.captured();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["reasoning"]["effort"], "high");
        for entry in fs::read_dir(f.root.join("app/sessions")).unwrap() {
            let history = fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(!history.contains("› Hibiscus"));
            assert!(!history.contains("Thinking   high"));
        }
    }
}

#[test]
fn compact_header_survives_a_five_line_draft_and_restores_the_large_banner() {
    for color in [false, true] {
        let f = Fixture::new("responses", vec![]);
        let mut tty = Pty::spawn_with_size(&mut f.command(), color, 42, 14);
        tty.wait_text("Enter send");
        tty.send(b"\x1b[200~draft one\ndraft two\ndraft three\ndraft four\ndraft five\x1b[201~");
        tty.wait_text("draft five");
        tty.wait_text("HIBI · Thinking off");
        tty.resize(120, 32);
        tty.wait_text("Thinking   off");
        tty.wait_text("draft five");
        // Ctrl+C clears the draft, then /quit exits without a provider call.
        tty.send(b"\x03");
        tty.wait_text("Enter send");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        let views = support::screen::snapshots(&shown);
        assert!(views.iter().any(|view| view
            .lines()
            .next()
            .is_some_and(|line| line.contains("HIBI · Thinking off"))
            && view.contains("draft five")));
        assert!(f.captured().is_empty());
    }
}
