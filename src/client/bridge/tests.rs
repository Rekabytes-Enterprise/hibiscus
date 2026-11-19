use super::*;

#[test]
fn newest_unacknowledged_queue_is_preserved_for_explicit_review() {
    let mut screen = crate::ui::Frontend::new(false).unwrap();
    let mut pending = HashMap::new();
    for n in [2, 10] {
        pending.insert(
            format!("hibiscus-1-queued-{n}"),
            DraftSubmission {
                text: format!("draft {n}"),
                images: vec![json!({"type":"image","data":"synthetic"}).into()],
                mode: QueueMode::Steer,
            },
        );
    }
    preserve_uncertain_queue(&mut screen, &mut pending).unwrap();
    let (text, images) = screen.take_rejected_queue().unwrap();
    assert_eq!(text, "draft 10");
    assert_eq!(images[0]["data"], "synthetic");
    assert_eq!(pending.len(), 1);
}

#[test]
fn borrowed_images_serialize_as_one_jsonl_record_and_preserve_originals() {
    let command = json!({"id":"request","type":"prompt","message":"line one\nline two"});
    let images: Vec<SharedImage> =
        vec![json!({"type":"image","mimeType":"image/png","data":"a".repeat(1024 * 1024)}).into()];
    let pointer = images[0]["data"].as_str().unwrap().as_ptr();
    let recovery = images.clone();
    assert!(std::sync::Arc::ptr_eq(&images[0], &recovery[0]));
    let mut wire = Vec::new();
    write_command(&mut wire, &command, &images).unwrap();
    assert_eq!(wire.iter().filter(|&&byte| byte == b'\n').count(), 1);
    let decoded: Value = serde_json::from_slice(&wire).unwrap();
    assert_eq!(decoded["images"], json!(images));
    assert_eq!(decoded["message"], command["message"]);
    assert_eq!(images[0]["data"].as_str().unwrap().as_ptr(), pointer);
    assert!(command.get("images").is_none());
    wire.clear();
    write_command(&mut wire, &command, &[]).unwrap();
    assert!(serde_json::from_slice::<Value>(&wire)
        .unwrap()
        .get("images")
        .is_none());
}

#[test]
fn buffered_command_does_not_retry_a_failed_write_on_drop() {
    struct FailsOnce(usize);
    impl Write for FailsOnce {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 += 1;
            if self.0 == 1 {
                Err(io::Error::other("uncertain write"))
            } else {
                Ok(bytes.len())
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = FailsOnce(0);
    assert!(write_command(&mut writer, &json!({"type":"prompt","message":"test"}), &[]).is_err());
    assert_eq!(writer.0, 1, "never implicitly retry a buffered RPC command");
}

#[test]
fn edit_preview_uses_result_details_and_limits_output() {
    let diff = (0..45)
        .map(|n| format!("+{n} line"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Vec::new();
    show_tool_diff(
        &Ui::new(false),
        &mut output,
        "edit · file.rs",
        &json!({"details":{"diff":diff}}),
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("diff · file.rs"));
    assert!(output.contains("+39 line"));
    assert!(!output.contains("+40 line"));
    assert!(output.contains("5 more diff lines"));
    let mut empty = Vec::new();
    show_tool_diff(
        &Ui::new(false),
        &mut empty,
        "edit · file.rs",
        &json!({"details":{}}),
    )
    .unwrap();
    assert!(empty.is_empty());
}

#[test]
fn tool_labels_include_file_paths_but_not_shell_commands_or_controls() {
    assert_eq!(
        tool_label("read", &json!({"path":"src/README.md"})),
        "read · src/README.md"
    );
    assert_eq!(
        tool_label("write", &json!({"path":"bad\u{1b}[2J\nname"})),
        "write · bad[2Jname"
    );
    assert_eq!(
        tool_label("bash", &json!({"command":"echo SECRET"})),
        "bash"
    );
}

#[test]
fn mouse_wheel_escape_does_not_abort() {
    struct ScrollSpy(Vec<Navigation>);
    impl Write for ScrollSpy {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl ScrollDisplay for ScrollSpy {
        fn scroll(&mut self, key: Navigation) -> io::Result<()> {
            self.0.push(key);
            Ok(())
        }
    }
    let (tx, rx) = std::sync::mpsc::channel();
    for byte in b"\x1b[<64;10;10M\x1b[5~" {
        tx.send(*byte).unwrap();
    }
    let mut display = ScrollSpy(Vec::new());
    assert!(!active_keys(&rx, &mut display).unwrap());
    assert_eq!(
        display.0,
        [Navigation::MouseScroll(3, 10), Navigation::ScrollPage(1)]
    );
    tx.send(27).unwrap();
    assert!(active_keys(&rx, &mut display).unwrap());
}

fn records(text: &str) -> Receiver<Result<Value>> {
    let (tx, rx) = mpsc::channel();
    for line in text.lines() {
        tx.send(Ok(serde_json::from_str(line).unwrap())).unwrap();
    }
    rx
}

#[test]
fn assistant_labels_wait_for_visible_text_without_losing_indentation() {
    let mut leading = String::new();
    assert!(assistant_chunk("", false, true, &mut leading).is_none());
    assert!(assistant_chunk("\n    ", false, true, &mut leading).is_none());
    assert_eq!(
        assistant_chunk("indented", false, true, &mut leading).unwrap(),
        "\n    indented"
    );
    assert!(leading.is_empty());
    assert_eq!(
        assistant_chunk("\n", true, true, &mut leading).unwrap(),
        "\n"
    );
    assert_eq!(
        assistant_chunk("  ", false, false, &mut leading).unwrap(),
        "  "
    );
    assert!(assistant_chunk(&"\u{2003}".repeat(9000), false, true, &mut leading).is_none());
    assert!(leading.len() <= 8192);
}

fn run_exchange(
    text: &str,
    id: &str,
    kind: &str,
    sent: &mut Vec<u8>,
    shown: &mut Vec<u8>,
) -> Result<bool> {
    exchange(
        sent,
        &records(text),
        shown,
        &mut io::Cursor::new(Vec::<u8>::new()),
        &mut Dialogs,
        None,
        id,
        kind,
        &mut None,
        false,
    )
}

#[test]
fn prompt_stream_waits_for_settled_and_supports_next_command() {
    let events = concat!(
        "{\"type\":\"response\",\"id\":\"other\",\"success\":true}\n",
        "{\"type\":\"response\",\"id\":\"hibiscus-1\",\"success\":true}\n",
        "{\"type\":\"message_update\",\"assistantMessageEvent\":{\"type\":\"text_delta\",\"delta\":\"hello\"}}\n",
        "{\"type\":\"agent_end\"}\n",
        "{\"type\":\"message_update\",\"assistantMessageEvent\":{\"type\":\"text_delta\",\"delta\":\" world\"}}\n",
        "{\"type\":\"agent_settled\"}\n",
        "{\"type\":\"response\",\"id\":\"hibiscus-2\",\"success\":true,\"data\":{\"cancelled\":false}}\n"
    );
    let mut sent = Vec::new();
    let mut shown = Vec::new();
    let rx = records(events);
    let mut dialogs = Dialogs;
    let mut fallback = io::Cursor::new(Vec::<u8>::new());
    exchange(
        &mut sent,
        &rx,
        &mut shown,
        &mut fallback,
        &mut dialogs,
        None,
        "hibiscus-1",
        "prompt",
        &mut None,
        false,
    )
    .unwrap();
    assert_eq!(shown, b"hello world\n");
    exchange(
        &mut sent,
        &rx,
        &mut shown,
        &mut fallback,
        &mut dialogs,
        None,
        "hibiscus-2",
        "new_session",
        &mut None,
        false,
    )
    .unwrap();
    assert!(rx.try_recv().is_err());
}

#[test]
fn rejected_prompt_and_failed_agent_are_errors() {
    let rejected =
        b"{\"type\":\"response\",\"id\":\"hibiscus-1\",\"success\":false,\"error\":\"no model\"}\n";
    assert!(run_exchange(
        std::str::from_utf8(rejected).unwrap(),
        "hibiscus-1",
        "prompt",
        &mut Vec::new(),
        &mut Vec::new()
    )
    .unwrap_err()
    .to_string()
    .contains("no model"));
    let failed = concat!(
        "{\"type\":\"response\",\"id\":\"hibiscus-1\",\"success\":true}\n",
        "{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"error\"}}\n",
        "{\"type\":\"agent_settled\"}\n"
    );
    assert!(run_exchange(
        failed,
        "hibiscus-1",
        "prompt",
        &mut Vec::new(),
        &mut Vec::new()
    )
    .is_err());
}

#[test]
fn extension_dialog_is_cancelled_without_terminal() {
    let events = concat!(
        "{\"type\":\"extension_ui_request\",\"id\":\"dialog-1\",\"method\":\"confirm\"}\n",
        "{\"type\":\"response\",\"id\":\"hibiscus-1\",\"success\":true}\n",
        "{\"type\":\"agent_settled\"}\n"
    );
    let mut sent = Vec::new();
    run_exchange(events, "hibiscus-1", "prompt", &mut sent, &mut Vec::new()).unwrap();
    let response: Value = serde_json::from_slice(&sent).unwrap();
    assert_eq!(response["id"], "dialog-1");
    assert_eq!(response["cancelled"], true);
}

#[test]
fn escape_sends_clear_then_abort_and_preserves_session() {
    let events = concat!(
        "{\"type\":\"response\",\"id\":\"hibiscus-1\",\"success\":true}\n",
        "{\"type\":\"response\",\"id\":\"hibiscus-1-clear\",\"success\":true}\n",
        "{\"type\":\"response\",\"id\":\"hibiscus-1-abort\",\"success\":true}\n",
        "{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"aborted\"}}\n",
        "{\"type\":\"agent_settled\"}\n",
        "{\"type\":\"response\",\"id\":\"hibiscus-2\",\"success\":true,\"data\":{\"cancelled\":false}}\n"
    );
    let rx = records(events);
    let (tx, keys) = mpsc::channel();
    tx.send(27).unwrap();
    let mut sent = Vec::new();
    let mut shown = Vec::new();
    let mut fallback = io::Cursor::new(Vec::<u8>::new());
    let mut dialogs = Dialogs;
    assert!(exchange(
        &mut sent,
        &rx,
        &mut shown,
        &mut fallback,
        &mut dialogs,
        Some(&keys),
        "hibiscus-1",
        "prompt",
        &mut None,
        false
    )
    .unwrap());
    assert!(!exchange(
        &mut sent,
        &rx,
        &mut shown,
        &mut fallback,
        &mut dialogs,
        None,
        "hibiscus-2",
        "new_session",
        &mut None,
        false
    )
    .unwrap());
    let commands: Vec<Value> = sent
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(
        commands
            .iter()
            .map(|cmd| cmd["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["clear_queue", "abort"]
    );
}

#[test]
fn rejected_preflight_drains_already_sent_cancellation_before_returning() {
    let rx = records(concat!(
        "{\"type\":\"response\",\"id\":\"p\",\"success\":false,\"error\":\"no model\"}\n",
        "{\"type\":\"response\",\"id\":\"p-clear\",\"success\":true}\n",
        "{\"type\":\"response\",\"id\":\"p-abort\",\"success\":true}\n",
        "{\"type\":\"response\",\"id\":\"next\",\"success\":true}\n"
    ));
    let (sender, keys) = mpsc::channel();
    sender.send(27).unwrap();
    assert!(exchange(
        &mut Vec::new(),
        &rx,
        &mut Vec::new(),
        &mut io::Cursor::new(Vec::<u8>::new()),
        &mut Dialogs,
        Some(&keys),
        "p",
        "prompt",
        &mut None,
        false
    )
    .is_err());
    assert_eq!(rx.recv().unwrap().unwrap()["id"], "next");
}

#[test]
fn silent_command_deadline_and_invalid_records_require_reconnection() {
    let (_sender, receiver) = mpsc::channel();
    let error = receive_record(&receiver, Instant::now(), &mut Vec::new()).unwrap_err();
    assert_eq!(
        error.downcast_ref::<ChatError>().unwrap().recovery(),
        super::error::RecoveryAction::Reconnect
    );
    for record in [
        json!({}),
        json!({"type":"response","success":"yes"}),
        json!({"type":"response","success":false,"command":"parse"}),
    ] {
        assert!(check_record(&record).is_err());
    }
    assert!(check_record(&json!({"type":"future_event"})).is_ok());
    let error = read_record(&mut b"private invalid payload\n".as_slice()).unwrap_err();
    assert!(!error.to_string().contains("private invalid payload"));
}

#[test]
fn strict_lf_framing_accepts_unicode_separators() {
    let mut stream =
        "{\"type\":\"message_update\",\"text\":\"a\u{2028}b\"}\n{\"type\":\"agent_settled\"}\n"
            .as_bytes();
    assert_eq!(
        read_record(&mut stream).unwrap().unwrap()["text"],
        "a\u{2028}b"
    );
    assert_eq!(
        read_record(&mut stream).unwrap().unwrap()["type"],
        "agent_settled"
    );
}
