use super::*;
use std::sync::Arc;

#[test]
fn connected_command_picker_hides_login_without_an_empty_panel() {
    let mut screen = Screen::new(false).unwrap();
    screen
        .status(&serde_json::json!({"hasCredentials":true}))
        .unwrap();
    screen.insert_draft("/login");
    screen.refresh_suggestions();
    assert!(screen.suggestions.is_none());
    screen.draft = "/cli".into();
    screen.draft_cursor = 4;
    screen.refresh_suggestions();
    assert!(screen.suggestions.is_some());
}

#[test]
fn sgr_drag_release_and_wheel_are_distinct_from_escape() {
    let (send, recv) = std::sync::mpsc::channel();
    for (bytes, expected) in [
        (
            b"[<0;6;8M".as_slice(),
            Navigation::Mouse(MouseEvent {
                button: 0,
                column: 5,
                row: 7,
                release: false,
            }),
        ),
        (
            b"[<32;14;8M".as_slice(),
            Navigation::Mouse(MouseEvent {
                button: 32,
                column: 13,
                row: 7,
                release: false,
            }),
        ),
        (
            b"[<0;14;8m".as_slice(),
            Navigation::Mouse(MouseEvent {
                button: 0,
                column: 13,
                row: 7,
                release: true,
            }),
        ),
        (b"[<64;10;10M".as_slice(), Navigation::MouseScroll(3, 10)),
        (b"[<0;0;8M".as_slice(), Navigation::Other),
    ] {
        for byte in bytes {
            send.send(*byte).unwrap();
        }
        assert_eq!(escape_key(&recv), expected);
    }
}

#[test]
fn bracketed_paste_is_literal_and_bounded_during_an_active_run() {
    let mut screen = Screen::new(false).unwrap();
    let (send, events) = std::sync::mpsc::channel();
    for byte in b"[200~" {
        send.send(*byte).unwrap();
    }
    assert_eq!(escape_key(&events), Navigation::PasteStart);
    screen.paste_start();
    for byte in b"a\r\nb\x03\x1b[106;5uZ\x1b[201~" {
        assert!(screen.paste_byte(*byte).unwrap());
    }
    assert_eq!(screen.draft, "a\nb\nZ");
    assert!(screen.paste.is_none());
    screen.paste_start();
    for byte in "x".repeat(8193).bytes().chain(b"\x1b[201~".iter().copied()) {
        screen.paste_byte(byte).unwrap();
    }
    assert_eq!(screen.draft, "a\nb\nZ");
    assert!(screen
        .input_notice
        .as_deref()
        .unwrap()
        .contains("too large"));
}

#[test]
fn selection_extracts_only_visible_transcript_and_not_chrome() {
    let mut screen = Screen::new(false).unwrap();
    screen.selectable_rows = vec!["first line".into(), "second line".into()];
    // At 80 columns the left margin and transcript marker occupy 5 cells.
    screen.selection = Some(TextSelection {
        anchor: (0, 5 + 6),
        focus: (1, 5 + 6),
        dragging: false,
    });
    assert_eq!(screen.selected_text(), "line\nsecond");
    assert_eq!(screen.selection.unwrap().columns(0, 10, 5), Some((6, 10)));
    screen.dirty_transcript();
    assert!(screen.selection.is_none());
}
#[test]
fn layout_reuses_rows_for_typing_and_coalesces_scrolled_changes() {
    let mut screen = Screen::new(false).unwrap();
    screen.transcript = "hibi › first\nsecond\nthird\n".as_bytes().to_vec();
    let original = screen.transcript_layout(73);
    screen.insert_draft("typing does not change transcript");
    assert!(std::rc::Rc::ptr_eq(
        &original,
        &screen.transcript_layout(73)
    ));
    screen.scroll = 2;
    for line in ["fourth\n", "fifth\n"] {
        screen.dirty_transcript();
        screen.transcript.extend_from_slice(line.as_bytes());
    }
    assert_eq!(screen.scroll, 2, "scroll counting is deferred until layout");
    assert_eq!(screen.transcript_layout(73).len(), original.len() + 2);
    assert_eq!(screen.scroll, 4);
    screen.transcript_layout(73);
    assert_eq!(screen.scroll, 4, "do not apply an anchor twice");
    screen.clear_session().unwrap();
    assert!(screen.transcript_layout(73).is_empty());
    assert_eq!(screen.scroll, 0);
}

#[test]
fn printable_batches_are_bounded_utf8_safe_and_stop_before_control_keys() {
    let mut screen = Screen::new(false).unwrap();
    let (send, events) = std::sync::mpsc::channel();
    let text = "é".repeat(70);
    for byte in text.as_bytes().iter().skip(1).chain(b"\rMORE") {
        send.send(*byte).unwrap();
    }
    let mut pending = Vec::new();
    screen.insert_input_batch(text.as_bytes()[0], &events, &mut pending);
    assert_eq!(screen.draft.chars().count(), 64);
    screen.insert_input_batch(events.recv().unwrap(), &events, &mut pending);
    assert_eq!(screen.draft, text);
    assert!(pending.is_empty());
    assert_eq!(screen.pending_input.take(), Some(b'\r'));
    assert_eq!(events.recv().unwrap(), b'M');
}

#[test]
fn compact_token_labels_round_at_unit_boundaries() {
    for (count, label) in [
        (0, "0"),
        (999, "999"),
        (1_000, "1K"),
        (1_250, "1.3K"),
        (200_000, "200K"),
        (999_950, "1M"),
        (1_000_000, "1M"),
        (1_500_000, "1.5M"),
        (1_000_000_000, "1B"),
        (1_000_000_000_000, "1T"),
    ] {
        assert_eq!(compact_tokens(count), label, "{count}");
    }
    assert!(compact_tokens(u64::MAX).ends_with('T'));
}

#[test]
fn displayed_cost_has_two_decimals_without_calling_a_small_charge_free() {
    for (cost, label) in [
        (0.0, "Cost ~$0.00"),
        (0.004, "Cost <$0.01"),
        (0.005, "Cost ~$0.01"),
        (0.45, "Cost ~$0.45"),
        (0.6634, "Cost ~$0.66"),
    ] {
        let usage = SessionUsage::from_runtime(&serde_json::json!({"cost":cost})).unwrap();
        assert_eq!(usage.parts(), [label]);
    }
}

#[test]
fn pi_stats_are_confirmed_not_inferred_and_goal_keeps_its_right_edge() {
    let mut screen = Screen::new(false).unwrap();
    screen.status(&serde_json::json!({"model":{"provider":"test","id":"reasoner"},"thinkingLevel":"high","sessionName":"work"})).unwrap();
    assert_eq!(screen.thinking.as_deref(), Some("high"));
    screen.session_usage(&serde_json::json!({"tokens":{"input":50000,"output":10000,"cacheRead":40000,"cacheWrite":5000},"cost":0.45,"contextUsage":{"tokens":60000,"contextWindow":200000,"percent":30.0}})).unwrap();
    let usage = screen.usage.as_ref().unwrap();
    let line = working_status(130, "✿", "Working…", 12, "", Some((2, 4)), Some(usage));
    assert!(line.starts_with("✿ Working… · 12s · Esc stop"));
    assert!(line.contains("Ctx 60K/200K · 30%"), "{line}");
    assert!(line.contains("In 50K · Out 10K"), "{line}");
    assert!(
        line.ends_with("Goal  ██████████░░░░░░░░░░  2/4 · 50%"),
        "{line}"
    );
    assert_eq!(line.chars().count(), 130);
    assert!(
        !working_status(40, "✿", "Working…", 1, "", Some((2, 4)), Some(usage)).contains("In 50K")
    );
    screen.session_usage(&serde_json::json!({"tokens":{"input":50000,"output":10000},"contextUsage":{"tokens":null,"contextWindow":200000,"percent":null}})).unwrap();
    assert!(
        screen.usage.as_ref().unwrap().context.is_none(),
        "null context after compaction is not zero"
    );
    screen.session_usage(&serde_json::json!({})).unwrap();
    assert!(
        screen.usage.is_none(),
        "missing Runtime stats must not invent values"
    );
    screen
        .status(&serde_json::json!({"model":null,"thinkingLevel":"high"}))
        .unwrap();
    assert!(
        screen.thinking.is_none(),
        "no model must not imply a supported thinking level"
    );
}

#[test]
fn narrow_footer_keeps_only_complete_fields() {
    let usage = SessionUsage::from_runtime(&serde_json::json!({
        "contextUsage":{"tokens":3900,"contextWindow":272000,"percent":1.4},
        "tokens":{"input":19000,"output":9000},"cost":0.62
    }))
    .unwrap();
    let narrow = working_status(42, "✿", "Thinking…", 34, "", None, Some(&usage));
    assert_eq!(narrow, "✿ Thinking… · 34s · Esc stop");
    assert!(!narrow.contains("Ctx"));
    let goal = working_status(42, "✿", "Thinking…", 34, "", Some((2, 5)), Some(&usage));
    assert!(goal.starts_with("✿ Thinking… · 34s"), "{goal}");
    assert!(goal.ends_with("Goal 2/5 · 40%"), "{goal}");
    assert_eq!(goal.chars().count(), 42);
    assert_eq!(
        with_usage("Enter send · /help".into(), 42, Some(&usage)),
        "Enter send · /help · Ctx 3.9K/272K · 1%"
    );
}

#[test]
fn wide_layout_uses_terminal_width_for_composer_and_cursor() {
    for (columns, expected) in [
        (40, 38),
        (59, 57),
        (60, 56),
        (80, 76),
        (160, 156),
        (200, 196),
    ] {
        assert_eq!(content_width(columns), expected);
    }
    let draft = "x".repeat(180);
    assert_eq!(
        draft_cursor_position(&draft, content_width(200) - 5, draft.len()),
        (0, 180)
    );
    assert!(draft_cursor_position(&draft, content_width(80) - 5, draft.len()).0 > 0);
}

#[test]
fn session_scan_typing_replays_before_new_keys_without_auto_submission() {
    let mut screen = Screen::new(false).unwrap();
    screen.defer_session_typing(b"old".iter().copied());
    let (send, events) = std::sync::mpsc::channel();
    for byte in b" new\r" {
        send.send(*byte).unwrap();
    }
    assert_eq!(screen.prompt(&events).unwrap().as_deref(), Some("old new"));
    assert!(screen.deferred_input.is_empty());
    screen.defer_session_typing(b"stale".iter().copied());
    screen.clear_session().unwrap();
    assert!(screen.deferred_input.is_empty());
}

#[test]
fn new_session_clears_transcript_and_transient_input_but_keeps_model() {
    let mut screen = Screen::new(false).unwrap();
    screen.transcript = b"old conversation".to_vec();
    screen.draft = "old draft".into();
    screen.restore_draft(
        "failed prompt".into(),
        vec![Arc::new(serde_json::json!({"type":"image"}))],
    );
    screen
        .images
        .push(Arc::new(serde_json::json!({"type":"image"})));
    screen.clipboard_notice = "old clipboard notice".into();
    screen.auth_url = Some("https://example.invalid".into());
    screen.scroll = 20;
    screen.draft_scroll = 3;
    screen.secret_input = true;
    screen.session = "old session".into();
    screen.model = "provider/model".into();
    screen.suggestions = Some(Picker::new("Commands".into(), vec!["/new".into()], None, 5));
    screen.clear_session().unwrap();
    assert!(screen.transcript.is_empty());
    assert!(screen.draft.is_empty());
    assert!(screen.restored_draft.is_none());
    assert!(screen.images.is_empty());
    assert!(screen.clipboard_notice.is_empty());
    assert!(screen.auth_url.is_none());
    assert!(screen.suggestions.is_none());
    assert!(!screen.secret_input);
    assert_eq!(screen.scroll, 0);
    assert_eq!(screen.draft_scroll, 0);
    assert_eq!(screen.session, "new chat");
    assert_eq!(screen.model, "provider/model");
}

#[test]
fn rejected_image_is_shared_between_live_draft_and_restore_slot() {
    let mut screen = Screen::new(false).unwrap();
    let image: SharedImage = Arc::new(serde_json::json!({
        "type": "image", "mimeType": "image/png", "data": "x".repeat(1024 * 1024)
    }));
    screen
        .rejected_queue(
            DraftSubmission {
                text: "review".into(),
                images: vec![image.clone()],
                mode: QueueMode::Steer,
            },
            "queue denied",
        )
        .unwrap();
    let (_, saved) = screen.take_rejected_queue().unwrap();
    assert!(Arc::ptr_eq(&image, &screen.images[0]));
    assert!(Arc::ptr_eq(&screen.images[0], &saved[0]));
    // Clearing live input must not discard the recovery copy.
    screen.clear_images();
    screen.draft.clear();
    assert!(screen.images.is_empty());
    screen.restore_draft("review".into(), saved);
    let (tx, keys) = std::sync::mpsc::channel();
    tx.send(b'\r').unwrap();
    assert_eq!(screen.prompt(&keys).unwrap().as_deref(), Some("review"));
    assert!(Arc::ptr_eq(&image, &screen.take_images()[0]));
    // An unrelated draft must not be overwritten by a rejected queue.
    screen.draft = "newer".into();
    screen
        .rejected_queue(
            DraftSubmission {
                text: "old".into(),
                images: vec![image.clone()],
                mode: QueueMode::FollowUp,
            },
            "queue denied",
        )
        .unwrap();
    assert_eq!(screen.draft, "newer");
    let (_, saved) = screen.take_rejected_queue().unwrap();
    assert!(Arc::ptr_eq(&image, &saved[0]));
}

#[test]
fn restored_failed_draft_keeps_images_and_waits_for_explicit_submission() {
    let mut screen = Screen::new(false).unwrap();
    let images = vec![Arc::new(
        serde_json::json!({"type":"image","data":"test","mimeType":"image/png"}),
    )];
    screen.restore_draft("original\ntext".into(), images.clone());
    let (send, recv) = std::sync::mpsc::channel();
    for byte in b" edited\r" {
        send.send(*byte).unwrap();
    }
    assert_eq!(
        screen.prompt(&recv).unwrap().as_deref(),
        Some("original\ntext edited")
    );
    assert_eq!(screen.take_images(), images);
    assert!(screen.restored_draft.is_none());
    screen.set_disconnected(true).unwrap();
    assert!(screen.disconnected);
    screen.set_disconnected(false).unwrap();
    assert!(!screen.disconnected);
}

#[test]
fn follow_up_shortcuts_are_distinct_from_ctrl_enter_newlines() {
    for sequence in [b"\r".as_slice(), b"[13;3u", b"[27;3;13~"] {
        let (send, rx) = std::sync::mpsc::channel();
        for byte in sequence {
            send.send(*byte).unwrap();
        }
        assert_eq!(escape_key(&rx), Navigation::FollowUp);
    }
    let (send, rx) = std::sync::mpsc::channel();
    for byte in b"[13;5u" {
        send.send(*byte).unwrap();
    }
    assert_eq!(escape_key(&rx), Navigation::Newline);
}

#[test]
fn image_paste_recognizes_alt_and_ctrl_terminal_encodings() {
    for sequence in [
        b"v".as_slice(),
        b"[118;3u",
        b"[118;5u",
        b"[27;3;118~",
        b"[27;5;118~",
        b"[118;3:1u",
    ] {
        let (send, recv) = std::sync::mpsc::channel();
        for byte in sequence {
            send.send(*byte).unwrap();
        }
        assert_eq!(escape_key(&recv), Navigation::PasteImage);
    }
    let (send, recv) = std::sync::mpsc::channel();
    for byte in b"[118;3:3u" {
        send.send(*byte).unwrap();
    }
    assert_eq!(escape_key(&recv), Navigation::Other); // key release must not paste again
}

#[test]
fn multiline_input_preserves_newlines_and_backspace() {
    for newline in [
        b"\x1b[13;2u".as_slice(),
        b"\x1b[27;2;13~",
        b"\x1b[13;5u",
        b"\x1b[27;5;13~",
        b"\n",
    ] {
        let (send, recv) = std::sync::mpsc::channel();
        for byte in b"first".iter().chain(newline).chain(b"second\x7f!\r") {
            send.send(*byte).unwrap();
        }
        let mut screen = Screen::new(false).unwrap();
        assert_eq!(
            screen.prompt(&recv).unwrap().as_deref(),
            Some("first\nsecon!")
        );
    }
}

#[test]
fn pi_queue_updates_wait_for_user_delivery_with_duplicate_text_and_late_acks() {
    let mut screen = Screen::new(false).unwrap();
    screen.full = true;
    screen.suspended = true;
    screen.user("initial").unwrap();
    screen.expect_initial_user("initial");
    assert!(!screen
        .delivered_user(&serde_json::json!({"content":"initial"}))
        .unwrap());
    screen
        .queue_update(&serde_json::json!({"steering":["same","same"],"followUp":["later"]}))
        .unwrap();
    assert_eq!(screen.pending_queue.len(), 3);
    assert!(screen.pending_queue.iter().all(|item| !item.sending));
    let first = DraftSubmission {
        text: "same".into(),
        images: vec![Arc::new(serde_json::json!({"type":"image"}))],
        mode: QueueMode::Steer,
    };
    screen.queued(&first).unwrap();
    assert_eq!(screen.pending_queue[0].images, 1);
    assert_eq!(
        String::from_utf8_lossy(&screen.transcript)
            .matches("same")
            .count(),
        0
    );
    screen
        .queue_update(&serde_json::json!({"steering":["same"],"followUp":["later"]}))
        .unwrap();
    assert_eq!(
        screen
            .pending_queue
            .iter()
            .filter(|item| item.text == "same" && item.sending)
            .count(),
        1
    );
    screen.delivered_user(&serde_json::json!({"content":[{"type":"text","text":"same"},{"type":"image","data":"private-base64"}]})).unwrap();
    assert_eq!(
        screen
            .pending_queue
            .iter()
            .filter(|item| item.text == "same")
            .count(),
        1
    );
    assert!(!String::from_utf8_lossy(&screen.transcript).contains("private-base64"));
    screen
        .queued(&DraftSubmission {
            text: "same".into(),
            images: vec![],
            mode: QueueMode::Steer,
        })
        .unwrap();
    screen
        .queue_update(&serde_json::json!({"steering":[],"followUp":["later"]}))
        .unwrap();
    screen
        .delivered_user(&serde_json::json!({"content":"same"}))
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&screen.transcript)
            .matches("you › same")
            .count(),
        2
    );
    screen
        .queue_update(&serde_json::json!({"steering":[],"followUp":[]}))
        .unwrap();
    screen
        .delivered_user(&serde_json::json!({"content":"later"}))
        .unwrap();
    screen
        .queued(&DraftSubmission {
            text: "later".into(),
            images: vec![],
            mode: QueueMode::FollowUp,
        })
        .unwrap();
    assert!(
        screen.pending_queue.is_empty(),
        "late acknowledgement must not recreate an already delivered message"
    );
    assert_eq!(
        String::from_utf8_lossy(&screen.transcript)
            .matches("you › initial")
            .count(),
        1
    );
    assert_eq!(
        String::from_utf8_lossy(&screen.transcript)
            .matches("you › later")
            .count(),
        1
    );
}

#[test]
fn live_composer_keeps_editing_and_stages_only_explicit_queue_submissions() {
    let mut screen = Screen::new(false).unwrap();
    screen.full = true;
    screen.suspended = true;
    screen.start_work().unwrap();
    let (_send, events) = std::sync::mpsc::channel();
    for byte in b"hello" {
        assert!(matches!(
            screen.active_key(*byte, &events).unwrap(),
            ActiveAction::None
        ));
    }
    screen.input_navigation(Navigation::Left).unwrap();
    screen.active_key(b'!', &events).unwrap();
    screen.images.push(Arc::new(
        serde_json::json!({"type":"image","mimeType":"image/png","data":"test"}),
    ));
    let ActiveAction::Submit(submission) = screen.active_key(17, &events).unwrap() else {
        panic!("Ctrl+Q should queue a follow-up");
    };
    assert_eq!(submission.text, "hell!o");
    assert_eq!(submission.mode, QueueMode::FollowUp);
    assert_eq!(submission.images.len(), 1);
    assert!(screen.draft.is_empty() && screen.images.is_empty());
    screen.active_key(b'k', &events).unwrap();
    screen.rejected_queue(submission, "queue denied").unwrap();
    assert_eq!(screen.draft, "k", "a newer draft must not be overwritten");
    assert_eq!(screen.take_rejected_queue().unwrap().0, "hell!o");
    screen.active_key(3, &events).unwrap();
    for byte in b"/new" {
        screen.active_key(*byte, &events).unwrap();
    }
    assert!(matches!(
        screen.active_key(b'\r', &events).unwrap(),
        ActiveAction::None
    ));
    assert_eq!(
        screen.draft, "/new",
        "local commands cannot be sent as steering"
    );
}

#[test]
fn composer_edits_mid_string_with_arrows_and_utf8_boundaries() {
    fn type_keys(bytes: &[u8]) -> String {
        let (send, recv) = std::sync::mpsc::channel();
        for byte in bytes {
            send.send(*byte).unwrap();
        }
        Screen::new(false).unwrap().prompt(&recv).unwrap().unwrap()
    }
    assert_eq!(type_keys(b"abCD\x1b[D\x1b[D-\x1b[C\x1b[3~\r"), "ab-C");
    assert_eq!(type_keys("aé文\x1b[D\x7fß\r".as_bytes()), "aß文");
    assert_eq!(type_keys(b"hello\x01Hi \x05!\r"), "Hi hello!");
    assert_eq!(
        type_keys(b"one\ntwo\x1b[Hstart \x1b[F end\r"),
        "one\nstart two end"
    );
    assert_eq!(type_keys(b"abc\x1b[D\x04\r"), "ab");
}

#[test]
fn cursor_follows_wrapping_and_multiline_navigation_without_animation() {
    assert_eq!(draft_cursor_position("abcdef", 3, 3), (1, 0));
    assert_eq!(draft_cursor_position("abc\ndef", 3, 4), (2, 0));
    assert_eq!(draft_index_on_row("abcdef", 3, 1, 2), Some(5));
    let mut screen = Screen::new(false).unwrap();
    screen.draft = "one\ntwo\nthree\nfour\nfive\nsix\nseven".into();
    screen.draft_cursor = screen.draft.len();
    for _ in 0..5 {
        screen.input_navigation(Navigation::Up).unwrap();
    }
    assert_eq!(
        draft_cursor_position(&screen.draft, 75, screen.draft_cursor).0,
        1
    );
    assert!(screen.draft_scroll > 0);
    screen.input_navigation(Navigation::Right).unwrap();
    assert_eq!(screen.preferred_column, None);
    screen.input_navigation(Navigation::End).unwrap();
    assert!(screen.draft.is_char_boundary(screen.draft_cursor));
}

#[test]
fn attachment_cancellation_limits_and_command_guard_preserve_drafts() {
    let mut screen = Screen::new(false).unwrap();
    let image = Arc::new(serde_json::json!({"type":"image","mimeType":"image/png","data":"test"}));
    screen.images = vec![image.clone(); 4];
    screen.paste_image().unwrap();
    assert!(screen.clipboard.is_none());
    assert!(screen.clipboard_notice.contains("Maximum 4"));
    screen.clear_images();
    screen.poll_clipboard().unwrap();
    assert!(screen.images.is_empty());

    screen.images.push(image.clone());
    let (send, recv) = std::sync::mpsc::channel();
    for byte in b"/new\r\x18\r" {
        send.send(*byte).unwrap();
    }
    assert_eq!(screen.prompt(&recv).unwrap().as_deref(), Some("/new"));
    assert!(screen.images.is_empty());

    screen.images.push(image);
    let (send, recv) = std::sync::mpsc::channel();
    for byte in b"\x03kept\r" {
        send.send(*byte).unwrap();
    }
    assert_eq!(screen.prompt(&recv).unwrap().as_deref(), Some("kept"));
    assert!(screen.images.is_empty());
}

#[test]
fn composer_wraps_grows_to_five_rows_and_scrolls_independently() {
    assert_eq!(draft_lines("abc\ndef", 4), ["abc", "def"]);
    assert_eq!(draft_lines("abcdef", 3), ["abc", "def", ""]);
    let mut screen = Screen::new(false).unwrap();
    screen.draft = "1\n2\n3\n4\n5\n6\n7".into();
    screen.draft_cursor = screen.draft.len();
    screen.input_navigation(Navigation::Up).unwrap();
    assert_eq!(screen.draft_scroll, 0);
    assert_eq!(
        draft_cursor_position(&screen.draft, 75, screen.draft_cursor).0,
        5
    );
    screen
        .input_navigation(Navigation::MouseScroll(3, 19))
        .unwrap();
    assert_eq!(screen.draft_scroll, 2);
    assert_eq!(screen.scroll, 0);
    screen
        .input_navigation(Navigation::MouseScroll(3, 5))
        .unwrap();
    assert_eq!(screen.scroll, 3);
    screen.input_navigation(Navigation::Down).unwrap();
    assert_eq!(screen.draft_scroll, 0);
    screen.input_navigation(Navigation::Newline).unwrap();
    assert_eq!(screen.draft_scroll, 0);
    assert!(screen.draft.ends_with('\n'));
}

#[test]
fn next_user_turn_has_exactly_one_blank_separator() {
    let mut screen = Screen::new(false).unwrap();
    screen.full = true;
    screen.suspended = true;
    screen.transcript = "hibi › answer\n\n\n".as_bytes().to_vec();
    screen.user("next").unwrap();
    assert_eq!(
        String::from_utf8(screen.transcript.clone()).unwrap(),
        "hibi › answer\n\nyou › next\n"
    );
}

#[test]
fn transcript_scroll_stops_at_a_full_page() {
    assert_eq!(
        transcript_viewport(120, 40, 500),
        Viewport {
            start: 0,
            end: 40,
            scroll: 80
        }
    );
    assert_eq!(
        transcript_viewport(120, 40, 30),
        Viewport {
            start: 50,
            end: 90,
            scroll: 30
        }
    );
    assert_eq!(
        transcript_viewport(40, 40, 1),
        Viewport {
            start: 0,
            end: 40,
            scroll: 0
        }
    );
    assert_eq!(
        transcript_viewport(6, 40, 500),
        Viewport {
            start: 0,
            end: 6,
            scroll: 0
        }
    );
    assert_eq!(
        transcript_viewport(120, 0, 500),
        Viewport {
            start: 0,
            end: 0,
            scroll: 0
        }
    );
}

#[test]
fn working_flower_animates_while_explore_and_elapsed_status_refresh() {
    let mut screen = Screen::new(false).unwrap();
    screen.full = true;
    screen.suspended = true; // test the state machine without writing ANSI to stdout
    screen.start_work().unwrap();
    assert_eq!(screen.activity.as_ref().unwrap().phase, "Working…");
    let read = super::tool::Call {
        name: "read".into(),
        target: "src/main.rs".into(),
    };
    screen.tool_start("r", &read).unwrap();
    screen
        .tool_end(
            "r",
            &read,
            &super::tool::Outcome {
                failed: false,
                diff: None,
                output: "",
                exit_code: None,
                changes: None,
                elapsed: None,
            },
        )
        .unwrap();
    assert!(screen.rendered_transcript().contains("◐ Exploring"));
    screen.activity.as_mut().unwrap().last_spinner -= Duration::from_millis(400);
    screen.tick_work().unwrap();
    assert!(screen.rendered_transcript().contains("◓ Exploring"));
    assert_eq!(screen.activity.as_ref().unwrap().flower_frame, 1);
    screen.activity.as_mut().unwrap().last_refresh -= Duration::from_secs(2);
    screen.tick_work().unwrap();
    assert!(screen.activity.as_ref().unwrap().last_refresh.elapsed() < Duration::from_secs(1));
    screen
        .tool_start(
            "e",
            &super::tool::Call {
                name: "edit".into(),
                target: "src/main.rs".into(),
            },
        )
        .unwrap();
    let settled = screen.rendered_transcript();
    assert!(settled.contains("✓ Explored"));
    screen.activity.as_mut().unwrap().last_spinner -= Duration::from_millis(400);
    screen.tick_work().unwrap();
    assert_eq!(screen.rendered_transcript(), settled);
    assert_eq!(screen.activity.as_ref().unwrap().flower_frame, 2);
    screen.set_work("Running read…").unwrap();
    assert_eq!(screen.activity.as_ref().unwrap().phase, "Running read…");
    screen.stop_work().unwrap();
    assert!(screen.activity.is_none());
}

#[test]
fn clips_long_status_and_wraps_transcript() {
    assert_eq!(clip("abcdefgh", 5), "abcd…");
    assert_eq!(markdown::format("abcdef", 3).len(), 2);
    assert_eq!(strip_ansi("\x1b[35mhello\x1b[0m"), "hello");
}

#[test]
fn unnamed_session_shows_short_id_instead_of_long_filename() {
    let mut screen = Screen::new(false).unwrap();
    screen
        .status(&serde_json::json!({
            "model": {"provider":"openai-codex","id":"gpt-5.5"},
            "sessionFile":"/tmp/2026-09-24T04-02-39-644Z_01a0d19f-699b-72d7-88e2-4.jsonl",
            "sessionId":"01a0d19f-699b-72d7-88e2-4"
        }))
        .unwrap();
    assert_eq!(screen.session, "01a0d19f");
    assert_eq!(screen.model, "openai-codex/gpt-5.5");
}

#[test]
fn enter_on_file_picker_during_a_run_does_not_queue_a_prompt() {
    let mut screen = Screen::new(false).unwrap();
    screen.draft = "check @rea".into();
    screen.draft_cursor = screen.draft.len();
    screen.file_prefix = file_mentions::prefix(&screen.draft, screen.draft_cursor);
    screen.file_matches = vec![file_mentions::Entry {
        path: "README.md".into(),
        directory: false,
    }];
    screen.suggestions = Some(Picker::new(
        "Files".into(),
        vec!["README.md".into()],
        None,
        5,
    ));
    let (_sender, events) = std::sync::mpsc::channel();
    assert!(matches!(
        screen.active_key(b'\r', &events).unwrap(),
        ActiveAction::None
    ));
    assert_eq!(screen.draft, "check @README.md ");
    assert!(screen.file_matches.is_empty());
}

#[test]
fn file_completion_replaces_only_the_cursor_token_and_keeps_suffix() {
    let mut screen = Screen::new(false).unwrap();
    screen.draft = "inspect @REA and continue".into();
    screen.draft_cursor = "inspect @REA".len();
    screen.file_prefix = file_mentions::prefix(&screen.draft, screen.draft_cursor);
    screen.file_matches = vec![file_mentions::Entry {
        path: "README.md".into(),
        directory: false,
    }];
    screen.suggestions = Some(Picker::new(
        "Files".into(),
        vec!["README.md".into()],
        None,
        5,
    ));
    assert!(screen.complete_file().unwrap());
    assert_eq!(screen.draft, "inspect @README.md and continue");
    assert_eq!(screen.draft_cursor, "inspect @README.md".len());
    assert!(screen.suggestions.is_none());

    screen.draft = "see @\"my dir/\" later".into();
    screen.draft_cursor = "see @\"my dir/".len();
    screen.file_prefix = file_mentions::prefix(&screen.draft, screen.draft_cursor);
    screen.file_matches = vec![file_mentions::Entry {
        path: "my dir/sub".into(),
        directory: true,
    }];
    screen.suggestions = Some(Picker::new("Files".into(), vec!["sub/".into()], None, 5));
    assert!(screen.complete_file().unwrap());
    assert_eq!(screen.draft, "see @\"my dir/sub/\" later");
    assert_eq!(screen.draft_cursor, "see @\"my dir/sub/".len());
}

#[test]
fn slash_suggestions_complete_and_follow_arrows() {
    fn input(bytes: &[u8]) -> String {
        let (send, recv) = std::sync::mpsc::channel();
        for key in bytes {
            send.send(*key).unwrap();
        }
        Screen::new(false).unwrap().prompt(&recv).unwrap().unwrap()
    }
    assert_eq!(input(b"/m\r"), "/models");
    assert_eq!(input(b"/lo\x1b[B\r"), "/logout");
    assert_eq!(input(b"/mo\t\r"), "/models");
    assert_eq!(input(b"/mx\x7f\r"), "/models");
    assert_eq!(input(b"hello\r"), "hello");
}

#[test]
fn escape_dismisses_suggestions_without_discarding_input() {
    let (quick, quick_events) = std::sync::mpsc::channel();
    for key in b"/m\x1b\r" {
        quick.send(*key).unwrap();
    }
    assert_eq!(
        Screen::new(false)
            .unwrap()
            .prompt(&quick_events)
            .unwrap()
            .as_deref(),
        Some("/m")
    );
    let (send, recv) = std::sync::mpsc::channel();
    for key in b"/m\x1b" {
        send.send(*key).unwrap();
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        send.send(b'\r').unwrap();
    });
    let mut screen = Screen::new(false).unwrap();
    assert_eq!(screen.prompt(&recv).unwrap().as_deref(), Some("/m"));
}

#[test]
fn update_notice_waits_for_a_started_draft_to_finish() {
    let (keys, events) = std::sync::mpsc::channel();
    let (notify, updates) = std::sync::mpsc::channel();
    let mut screen = Screen::new(false).unwrap();
    screen.draft = "half-written".into();
    notify.send(Some("v0.2.0".into())).unwrap();
    keys.send(b'\r').unwrap();
    assert_eq!(
        screen.read_prompt(&events, Some(&updates)).unwrap(),
        PromptAction::Text(Some("half-written".into()))
    );
    assert_eq!(
        screen.prompt_with_update(&events, Some(&updates)).unwrap(),
        PromptAction::Update("v0.2.0".into())
    );
}

#[test]
fn page_keys_do_not_appear_in_prompt() {
    let (send, recv) = std::sync::mpsc::channel();
    for key in b"\x1b[5~\x1b[<64;10;10Mhello\r" {
        send.send(*key).unwrap();
    }
    let mut screen = Screen::new(false).unwrap();
    assert_eq!(screen.prompt(&recv).unwrap().as_deref(), Some("hello"));
    assert!(screen.scroll > 0);
}

#[test]
fn oauth_url_is_short_clickable_label_and_clipboard_encoding_is_complete() {
    assert_eq!(base64(b"a"), "YQ==");
    assert_eq!(base64(b"ab"), "YWI=");
    assert_eq!(base64(b"abc"), "YWJj");
    let mut screen = Screen::new(false).unwrap();
    assert!(!screen
        .set_auth_url("https://bad.example/\u{1b}]52;")
        .unwrap());
    screen
        .set_auth_url("https://auth.openai.com/oauth/authorize?state=test")
        .unwrap();
    let line = &markdown::format("  · Open sign-in ↗ (Ctrl+Y to copy)", 80)[0];
    let styled = screen.styled_line(line, 80);
    assert!(styled.contains("\x1b]8;;https://auth.openai.com/oauth/authorize?state=test\x1b\\Open sign-in ↗\x1b]8;;\x1b\\"));
    assert!(!screen
        .transcript
        .windows(b"state=test".len())
        .any(|part| part == b"state=test"));
    let unrelated = &markdown::format("hibi › Open sign-in ↗", 80)[1];
    assert!(!screen.styled_line(unrelated, 80).contains("\x1b]8;;"));
    screen.clear_auth_url().unwrap();
    assert!(!screen.styled_line(line, 80).contains("\x1b]8;;"));
}

#[test]
fn activity_and_diff_use_theme_colors_when_enabled() {
    let mut screen = Screen::new(false).unwrap();
    screen.color = true;
    screen.full = true;
    screen.suspended = true;
    screen.start_work().unwrap();
    assert!(screen
        .accent(screen.status_style(), "✾ Thinking…")
        .contains("\x1b[1;38;2;236;74;125m"));
    let rows = markdown::format(
        "  · reasoning · 3s\n  · diff · updated.md\n    -12 old\n    +12 new",
        80,
    );
    assert!(screen
        .styled_line(&rows[0], 80)
        .contains("\x1b[1;38;2;236;74;125m"));
    assert!(screen.styled_line(&rows[2], 80).contains("48;2;65;35;49"));
    assert!(screen.styled_line(&rows[3], 80).contains("48;2;36;55;44"));
    screen.stop_work().unwrap();
    assert_eq!(screen.status_style(), "2");
}

#[test]
fn picker_fits_narrow_terminal_and_marks_active_model() {
    let mut screen = Screen::new(false).unwrap();
    let items = (0..8)
        .map(|n| format!("very-long-model-identifier-{n}"))
        .collect();
    screen.picker = Some(Picker::new("Models · demo".into(), items, Some(6), 5));
    let (top, rows) = screen.picker_rows(36, 12, "  ").unwrap();
    assert_eq!(top, 4); // panel docks to the bottom, not over the middle of chat
    assert_eq!(rows.len(), 8);
    assert!(rows
        .iter()
        .all(|line| line.starts_with("  ") && line.chars().count() <= 38));
    assert!(rows.iter().any(|line| line.contains("● current")));
    assert!(rows.iter().any(|line| line.contains('█')));
}

#[test]
fn working_footer_puts_status_left_and_goal_right_only_while_active() {
    let line = working_status(76, "✿", "Thinking…", 49, "", Some((2, 4)), None);
    assert!(line.starts_with("✿ Thinking… · 49s · Esc stop"));
    assert!(
        line.ends_with("Goal  ██████████░░░░░░░░░░  2/4 · 50%"),
        "{line}"
    );
    assert_eq!(line.chars().count(), 76);
    let narrow = working_status(40, "✿", "Reading…", 7, "", Some((2, 4)), None);
    assert!(narrow.contains("Goal 2/4 · 50%"));
    assert!(narrow.chars().count() <= 40);
    assert!(!working_status(76, "✿", "Working…", 3, "", None, None).contains("Goal"));
}

#[test]
fn goal_row_is_one_line_and_absent_without_an_explicit_checklist() {
    assert_eq!(
        goal_line(40, 12, 20),
        "Goal  ████████████░░░░░░░░  12/20 · 60%"
    );
    assert_eq!(
        goal_line(40, 20, 20),
        "Goal  ████████████████████  20/20 · 100%"
    );
    assert!(goal_line(18, 3, 20).chars().count() <= 18);
    let mut screen = Screen::new(false).unwrap();
    screen.set_goal(&serde_json::json!({"hibiscusGoal":{"completed":3,"total":20}}));
    assert_eq!(screen.goal, Some((3, 20)));
    screen.set_goal(
        &serde_json::json!({"hibiscusGoal":{"completed":20,"total":20,"error":"invalid"}}),
    );
    assert_eq!(screen.goal, Some((3, 20)));
    screen.clear_session().unwrap();
    assert_eq!(screen.goal, None);
    screen.restore_goal(&[
            serde_json::json!({"role":"toolResult","toolName":"goal","details":{"hibiscusGoal":{"completed":12,"total":20}}}),
            serde_json::json!({"role":"toolResult","toolName":"read","details":{"hibiscusGoal":{"completed":20,"total":20}}}),
        ]).unwrap();
    assert_eq!(screen.goal, Some((12, 20)));
    screen.restore_goal(&[]).unwrap();
    assert_eq!(screen.goal, None);
}

#[test]
fn background_covers_erased_space_and_survives_style_resets_only_with_color() {
    let original = "\x1b[Hleft\x1b[1;38;2;236;74;125m✿\x1b[0m\x1b[K\r\n\x1b[K\r\n\x1b[48;2;57;30;46muser\x1b[0m\x1b[K\r\n\x1b[J";
    let painted = paint_background(original, true);
    assert!(painted.starts_with(BASE_BACKGROUND));
    assert!(painted.contains(&format!("✿\x1b[0m{BASE_BACKGROUND}\x1b[K")));
    assert!(painted.contains(&format!("user\x1b[0m{BASE_BACKGROUND}\x1b[K")));
    assert!(painted.ends_with("\x1b[J"));
    assert_eq!(paint_background(original, false), original);
}

#[test]
fn user_tint_covers_wrapped_rows_and_margins_without_leaking() {
    let mut screen = Screen::new(false).unwrap();
    screen.color = true;
    let rows = markdown::format(
        "you › a long user message\nsecond line\nhibi › **reply**",
        10,
    );
    for row in rows.iter().filter(|row| row.role == Role::User) {
        let rendered = screen.transcript_line(row, 13, 19, "   ");
        assert!(rendered.starts_with("\x1b[48;2;57;30;46m   "));
        assert!(rendered.ends_with("\x1b[0m"));
        assert_eq!(strip_ansi(&rendered).chars().count(), 19);
    }
    for row in rows.iter().filter(|row| row.role == Role::Assistant) {
        assert!(!screen.transcript_line(row, 13, 19, "   ").contains("48;2"));
    }
    assert!(rows
        .iter()
        .any(|row| screen.styled_line(row, 10).contains("hibi")));
    screen.color = false;
    assert!(!screen
        .transcript_line(&rows[0], 13, 19, "   ")
        .contains('\x1b'));
}

#[test]
fn table_borders_use_raspberry_and_respect_no_color() {
    let rows = markdown::format("hibi › A | B\none | two\nthree | four", 80);
    let mut screen = Screen::new(false).unwrap();
    let plain = screen.styled_line(&rows[1], 80);
    assert!(plain.starts_with('╭'));
    assert!(!plain.contains('\x1b'));
    screen.color = true;
    assert!(screen
        .styled_line(&rows[1], 80)
        .contains("\x1b[38;2;184;57;101m"));
    assert!(screen
        .styled_line(&rows[3], 80)
        .contains("\x1b[38;2;184;57;101m"));
}

#[test]
fn pink_highlights_markdown_but_no_color_keeps_clean_text() {
    let line = &markdown::format("hibi › **pink** `code`", 80)[1];
    let mut screen = Screen::new(false).unwrap();
    assert_eq!(screen.styled_line(line, 80), "pink code");
    screen.color = true;
    assert!(screen
        .styled_line(line, 80)
        .contains("\x1b[1;38;2;255;220;230m"));
    assert!(screen
        .styled_line(line, 80)
        .contains("\x1b[38;2;255;173;197m"));
}
