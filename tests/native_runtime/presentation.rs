use super::*;

#[test]
fn tui_cli_tui_switch_keeps_one_session_and_never_replays_prompts() {
    let f = Fixture::new(
        "responses",
        vec![response("BEFORE_SWITCH"), response("AFTER_SWITCH")],
    );
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("Enter send");
    tty.send(b"first message\r");
    tty.wait_text("BEFORE_SWITCH");
    tty.wait_text("Enter send");
    tty.send(b"/cli\r");
    tty.wait_text(" · CLI");
    tty.wait_text("/tui");
    tty.wait_text("BEFORE_SWITCH");
    tty.wait_text("❯ ");
    tty.send(b"second message\r");
    tty.wait_text("AFTER_SWITCH");
    tty.wait_text("❯ ");
    tty.send(b"/tui\r");
    tty.wait_text("BEFORE_SWITCH");
    tty.wait_text("AFTER_SWITCH");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(shown.contains("\x1b[?1049l"));
    assert_eq!(shown.matches("\x1b[?1049h").count(), 2);
    assert_eq!(f.captured().len(), 2);
    let files = fs::read_dir(f.root.join("app/sessions"))
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1);
    let history = fs::read_to_string(files.into_iter().next().unwrap().unwrap().path()).unwrap();
    assert!(!history.contains("/cli") && !history.contains("/tui"));
    assert_eq!(history.matches("first message").count(), 1);
    assert_eq!(history.matches("second message").count(), 1);
}

#[test]
fn saved_output_commands_are_local_complete_resumable_and_session_scoped() {
    for color in [false, true] {
        let command = format!(
            "printf '%s\\n' {}",
            (0..30)
                .map(|i| format!("'SAVED_LINE_{i:02}_payload'"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        assert!(command.len() > 400);
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![
                    ("write", json!({"path":"saved.txt","content":"old\n"})),
                    (
                        "edit",
                        json!({"path":"saved.txt","oldText":"old","newText":"new"}),
                    ),
                    ("bash", json!({"command":command})),
                ]),
                response("OUTPUT_READY"),
            ],
        );
        let mut process = f.command();
        process.arg("--cli");
        let mut tty = Pty::spawn_with_color(&mut process, color);
        tty.wait_text("/tui");
        tty.send(b"run the saved-output fixture\r");
        tty.wait_text("OUTPUT_READY");
        tty.wait_text("❯");
        tty.send(b"/output list\r");
        tty.wait_text("Recent tool results");
        tty.wait_text("Use /output <number>");
        tty.wait_text("❯");
        tty.send(b"/output\r");
        tty.wait_text("Saved output #3");
        tty.wait_text("      SAVED_LINE_15_payload");
        tty.wait_text("❯");
        tty.send(b"/output 2\r");
        tty.wait_text("Saved output #2");
        tty.wait_text("+new");
        tty.wait_text("❯");
        tty.send(b"/output 0\r");
        tty.wait_text("positive number");
        tty.wait_text("❯");
        tty.send(b"/output 999\r");
        tty.wait_text("No saved tool output matches");
        tty.wait_text("❯");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        assert!(shown.contains("Saved output: /output list"));
        let views = support::screen::snapshots(&format!("{shown}\x1b[?2026l"));
        let visible = views.last().unwrap().split_whitespace().collect::<String>();
        assert!(visible.contains("SAVED_LINE_29_payload'")); // command survived wrapping and the former 400-character cap
        assert!(!shown.contains("\n      exit 0"));
        assert_eq!(f.captured().len(), 2);
        let session = fs::read_dir(f.root.join("app/sessions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let history = fs::read_to_string(session).unwrap();
        assert!(!history.contains("/output") && !history.contains("Waiting for response"));

        let mut process = f.command();
        process.args(["--cli", "--continue"]);
        let mut tty = Pty::spawn_with_color(&mut process, color);
        tty.wait_text("OUTPUT_READY");
        tty.wait_text("❯");
        tty.send(b"/output 3\r");
        tty.wait_text("Saved output #3");
        tty.wait_text("      SAVED_LINE_15_payload");
        tty.wait_text("❯");
        tty.send(b"/new\r");
        tty.wait_text("❯");
        tty.send(b"/output\r");
        tty.wait_text("No saved tool output matches");
        tty.wait_text("❯");
        tty.send(b"/quit\r");
        tty.finish();
        assert_eq!(f.captured().len(), 2);
        assert_eq!(
            fs::read_to_string(f.root.join("saved.txt")).unwrap(),
            "new\n"
        );
    }
}

#[test]
fn shell_runs_in_both_views_and_ignores_obsolete_settings() {
    for mode in ["--tui", "--cli"] {
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![
                    ("bash", json!({"command":"printf first > first-file"})),
                    ("bash", json!({"command":"printf second > second-file"})),
                ]),
                response("SHELLS_DONE"),
            ],
        );
        let settings = f.root.join("app/approval.json");
        fs::write(&settings, "obsolete settings are not parsed").unwrap();
        let mut command = f.command();
        command.arg(mode);
        let mut tty = Pty::spawn(&mut command);
        tty.wait_text(if mode == "--tui" {
            "Enter send"
        } else {
            " · CLI"
        });
        tty.send(b"run both shell commands\r");
        tty.wait_text("SHELLS_DONE");
        tty.wait_text(if mode == "--tui" {
            "Enter send"
        } else {
            "❯ "
        });
        tty.send(b"/quit\r");
        let shown = tty.finish();
        assert!(!shown.contains("Approval") && !shown.contains("Allow once"));
        assert_eq!(
            fs::read_to_string(f.root.join("first-file")).unwrap(),
            "first"
        );
        assert_eq!(
            fs::read_to_string(f.root.join("second-file")).unwrap(),
            "second"
        );
        assert_eq!(
            fs::read_to_string(settings).unwrap(),
            "obsolete settings are not parsed"
        );
        assert_eq!(f.captured().len(), 2);
    }
}

#[test]
fn repeated_view_handover_keeps_paste_atomic_and_restores_terminal_modes() {
    for color in [false, true] {
        let f = Fixture::new("responses", vec![response("HANDOVER_DONE")]);
        let mut command = f.command();
        command.arg("--cli");
        let mut tty = Pty::spawn_with_color(&mut command, color);
        tty.wait_text("\x1b[?2004h");
        for _ in 0..2 {
            tty.send(b"/tui\r");
            tty.wait_text("Enter send");
            tty.send(b"/cli\r");
            tty.wait_text(" · CLI");
            tty.wait_text("\x1b[?2004h");
        }
        assert!(f.captured().is_empty());
        tty.send(b"\x1b[200~after switching\n/quit\x1b[201~");
        tty.wait_text("/quit");
        assert!(f.captured().is_empty());
        tty.send(b"\r");
        tty.wait_text("HANDOVER_DONE");
        tty.wait_text("\x1b[?2004h");
        tty.send(b"/tui\r");
        tty.wait_text("HANDOVER_DONE");
        tty.wait_text("Enter send");
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
        assert_eq!(users[0]["content"][0]["text"], "after switching\n/quit");
        assert_eq!(
            fs::read_dir(f.root.join("app/sessions")).unwrap().count(),
            1
        );
        assert_eq!(shown.matches("\x1b[?1049h").count(), 3);
        assert_eq!(shown.matches("\x1b[?1049l").count(), 3);
        for mode in ["2004", "1000", "1002", "1006"] {
            let enabled = shown.rfind(&format!("\x1b[?{mode}h")).unwrap();
            let disabled = shown.rfind(&format!("\x1b[?{mode}l")).unwrap();
            assert!(disabled > enabled, "terminal mode {mode} leaked after exit");
        }
    }
}
