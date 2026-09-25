mod support;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use support::{unique_temp_dir, Pty};

/// Shadow every platform reader so tests never access a developer/CI desktop
/// clipboard. WAYLAND_DISPLAY does not override the compiled macOS backend.
fn install_clipboard_helpers(root: &Path) -> PathBuf {
    for name in ["wl-paste", "xclip", "osascript", "powershell.exe"] {
        let helper = root.join(name);
        fs::write(
            &helper,
            r#"#!/bin/sh
printf '%s\n' "${0##*/}" >> "$STEERING_CLIPBOARD_LOG"
for arg in "$@"; do
 case "$arg" in
  --list-types|TARGETS) printf 'image/png\n'; exit 0 ;;
 esac
done
cat "$STEERING_IMAGE"
"#,
        )
        .unwrap();
        fs::set_permissions(helper, fs::Permissions::from_mode(0o755)).unwrap();
    }
    root.join("clipboard-calls")
}

#[test]
fn clipboard_fixture_covers_all_platform_readers() {
    let root = unique_temp_dir("hibiscus-steering-clipboard");
    let log = install_clipboard_helpers(&root);
    let image = root.join("image.png");
    let bytes = b"\x89PNG\r\n\x1a\n";
    fs::write(&image, bytes).unwrap();
    for (name, args) in [
        ("osascript", vec!["-l", "JavaScript", "-e", "fixture-only"]),
        (
            "powershell.exe",
            vec![
                "-NoProfile",
                "-NonInteractive",
                "-STA",
                "-Command",
                "fixture-only",
            ],
        ),
        ("wl-paste", vec!["--type", "image/png", "--no-newline"]),
        (
            "xclip",
            vec!["-selection", "clipboard", "-t", "image/png", "-o"],
        ),
    ] {
        let result = Command::new(root.join(name))
            .args(args)
            .env("STEERING_IMAGE", &image)
            .env("STEERING_CLIPBOARD_LOG", &log)
            .output()
            .unwrap();
        assert!(result.status.success(), "{name}");
        assert_eq!(result.stdout, bytes, "{name} must return fixture bytes");
    }
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "osascript\npowershell.exe\nwl-paste\nxclip\n"
    );
    for (name, args) in [
        ("wl-paste", vec!["--list-types"]),
        (
            "xclip",
            vec!["-selection", "clipboard", "-t", "TARGETS", "-o"],
        ),
    ] {
        let result = Command::new(root.join(name))
            .args(args)
            .env("STEERING_IMAGE", &image)
            .env("STEERING_CLIPBOARD_LOG", &log)
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout, b"image/png\n");
    }
    fs::remove_dir_all(root).unwrap();
}

fn drive(scenario: &str) {
    let root = unique_temp_dir("hibiscus-steering");
    let pi = root.join("pi");
    let log = root.join("commands");
    let image = root.join("image.png");
    fs::write(&image, b"\x89PNG\r\n\x1a\n").unwrap();
    let clipboard_log = install_clipboard_helpers(&root);
    fs::write(&pi, r#"#!/bin/sh
turn=0
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '%s\n' "$line" >> "$STEERING_LOG"
   case "$line" in
    *'"streamingBehavior":"steer"'*)
     if [ "$STEERING_SCENARIO" = reject ] || [ "$STEERING_SCENARIO" = reject_late ]; then
      if [ "$STEERING_SCENARIO" = reject_late ]; then sleep 0.3; fi
      printf '{"type":"response","id":"%s","success":false,"error":"queue denied"}\n{"type":"agent_settled"}\n' "$id"
     else
      printf '{"type":"response","id":"%s","success":true}\n{"type":"queue_update","steering":["steer one"],"followUp":[]}\n' "$id"
      if [ "$STEERING_SCENARIO" = slow_start ]; then printf '%s\n' '{"type":"queue_update","steering":[],"followUp":[]}' '{"type":"agent_settled"}'; fi
      if [ "$STEERING_SCENARIO" = early_settled ]; then printf '%s\n' '{"type":"agent_settled"}'; fi
     fi ;;
    *'"streamingBehavior":"followUp"'*)
     printf '{"type":"response","id":"%s","success":true}\n{"type":"queue_update","steering":["steer one"],"followUp":["later"]}\n' "$id"
     sleep 1
     # A new queued run after an earlier settlement announces agent_start.
     if [ "$STEERING_SCENARIO" = early_settled ]; then printf '%s\n' '{"type":"agent_start"}'; fi
     printf '%s\n' '{"type":"queue_update","steering":[],"followUp":["later"]}'
     printf '%s\n' '{"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"steer one"},{"type":"image","data":"FAKE_PRIVATE_IMAGE","mimeType":"image/png"}]}}'
     printf '%s\n' '{"type":"queue_update","steering":[],"followUp":[]}'
     printf '%s\n' '{"type":"message_start","message":{"role":"user","content":"later"}}'
     printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"FIRST_RESPONSE"}}'
     if [ "$STEERING_SCENARIO" = settle_before_drain ]; then
      printf '%s\n' '{"type":"agent_settled"}' '{"type":"queue_update","steering":[],"followUp":[]}'
     else
      printf '%s\n' '{"type":"queue_update","steering":[],"followUp":[]}' '{"type":"agent_settled"}'
     fi ;;
    *)
     turn=$((turn+1))
     if [ "$turn" -eq 1 ] && [ "$STEERING_SCENARIO" = slow_start ]; then sleep 0.3; fi
     printf '{"type":"response","id":"%s","success":true}\n' "$id"
     if [ "$turn" -eq 1 ]; then
      printf '%s\n' '{"type":"message_start","message":{"role":"user","content":"first"}}' '{"type":"message_update","assistantMessageEvent":{"type":"thinking_start"}}'
     else
      printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"SECOND_RESPONSE"}}' '{"type":"agent_settled"}'
     fi ;;
   esac ;;
  *'"type":"clear_queue"'*)
   printf '{"type":"response","id":"%s","success":true,"data":{"steering":["steer one"],"followUp":[]}}\n' "$id"
   printf '%s\n' '{"type":"queue_update","steering":[],"followUp":[]}' ;;
  *'"type":"abort"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '%s\n' '{"type":"message_end","message":{"role":"assistant","stopReason":"aborted"}}' '{"type":"agent_settled"}' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("STEERING_LOG", &log)
        .env("STEERING_IMAGE", &image)
        .env("STEERING_CLIPBOARD_LOG", &clipboard_log)
        .env("STEERING_SCENARIO", scenario)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1")
        .env("WAYLAND_DISPLAY", "test")
        .env(
            "PATH",
            format!("{}:{}", root.display(), std::env::var("PATH").unwrap()),
        )
        .env_remove("WSL_DISTRO_NAME")
        .env_remove("WSL_INTEROP");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"first\r");
    if scenario == "slow_start" {
        tty.send(b"early\r");
        tty.wait_text("Pi is starting; press Enter again");
        tty.wait_text("Thinking…");
        tty.send(b"\r");
        tty.wait_text("steering accepted by Pi");
        tty.wait_text("Enter send");
        tty.send(b"/quit\r");
        tty.finish();
        let prompts: Vec<serde_json::Value> = fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[1]["message"], "early");
        assert_eq!(prompts[1]["streamingBehavior"], "steer");
        drop(tty);
        fs::remove_dir_all(root).unwrap();
        return;
    }
    tty.wait_text("Thinking…");
    tty.send(b"steer one\x16");
    tty.wait_text("1 image(s) attached");
    tty.send(b"\r");
    if matches!(scenario, "reject" | "reject_late") {
        if scenario == "reject_late" {
            tty.send(b"fresh");
        }
        tty.wait_text("Pi rejected queued message");
        tty.wait_text("Enter send");
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 2);
        tty.send(if scenario == "reject_late" {
            b" end\r"
        } else {
            b"\r"
        }); // explicit retry, never automatic
    } else {
        tty.wait_text("steering accepted by Pi");
        if scenario != "abort" {
            tty.wait_text("Waiting · Steering: steer one");
        }
        if scenario == "abort" {
            tty.send(b"keep\x1b");
            tty.wait_text("Stopped. You can keep chatting.");
            tty.wait_text("Enter send");
            tty.send(b" rest\r");
        } else {
            tty.send(b"later");
            tty.send(if scenario == "alt" {
                b"\x1b\r"
            } else {
                b"\x11"
            });
            tty.wait_text("follow-up accepted by Pi");
            tty.send(b"/new\r");
            tty.wait_text("Commands are available after Pi settles");
            tty.send(b"\x03kept");
            tty.wait_text("FIRST_RESPONSE");
            tty.wait_text("Enter send");
            tty.send(b" end\r");
        }
    }
    tty.wait_text("SECOND_RESPONSE");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let prompts: Vec<serde_json::Value> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(prompts[0]["message"], "first");
    assert_eq!(prompts[1]["message"], "steer one");
    assert_eq!(prompts[1]["streamingBehavior"], "steer");
    assert_eq!(prompts[1]["images"][0]["mimeType"], "image/png");
    assert_eq!(prompts[1]["images"][0]["data"], "iVBORw0KGgo=");
    let clipboard_calls = fs::read_to_string(&clipboard_log).unwrap();
    let expected_reader = if cfg!(target_os = "macos") {
        "osascript"
    } else {
        "wl-paste"
    };
    assert!(
        clipboard_calls.lines().any(|name| name == expected_reader),
        "paste must use the platform's mock reader, not the real clipboard"
    );
    assert!(
        clipboard_calls.lines().all(|name| name == expected_reader),
        "unexpected clipboard backend: {clipboard_calls}"
    );
    let ids = prompts
        .iter()
        .map(|prompt| prompt["id"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        ids.len(),
        prompts.len(),
        "queued commands must have distinct correlation IDs"
    );
    if matches!(
        scenario,
        "alt" | "ctrlq" | "early_settled" | "settle_before_drain"
    ) {
        assert_eq!(prompts.len(), 4);
        assert_eq!(prompts[2]["message"], "later");
        assert_eq!(prompts[2]["streamingBehavior"], "followUp");
        assert_eq!(prompts[3]["message"], "kept end");
        assert!(
            shown.contains("↪1 ▷1"),
            "show Pi's steering/follow-up queue counts"
        );
        assert!(prompts[3].get("images").is_none());
        assert!(shown.contains("Sending · Steering: steer one"));
        let frames = support::screen::snapshots(&shown);
        let waiting = frames
            .iter()
            .find(|frame| frame.contains("Waiting · Steering: steer one"))
            .unwrap();
        assert!(
            !waiting.contains("┃  steer one"),
            "queue acceptance must not add a user transcript row"
        );
        assert!(
            waiting.find("Waiting · Steering").unwrap() < waiting.rfind('╭').unwrap(),
            "pending message belongs above the composer"
        );
        assert!(
            shown.contains("┃  steer one"),
            "delivered steering message should become a normal user row"
        );
        assert!(
            shown.contains("┃  later"),
            "delivered follow-up should become a normal user row"
        );
        assert!(!shown.contains("[steering]"));
        assert!(!shown.contains("FAKE_PRIVATE_IMAGE"));
        assert!(!shown.contains("Started new session"));
    } else if scenario == "abort" {
        assert_eq!(prompts.len(), 3);
        assert_eq!(prompts[2]["message"], "keep rest");
    } else {
        assert_eq!(prompts.len(), 3);
        if scenario == "reject_late" {
            assert_eq!(
                prompts[2]["message"], "fresh end",
                "a rejected queue must not overwrite newer typing"
            );
            assert!(prompts[2].get("images").is_none());
        } else {
            assert_eq!(prompts[2]["message"], "steer one");
            assert_eq!(
                prompts[2]["images"], prompts[1]["images"],
                "explicit retry must keep the rejected image"
            );
        }
    }
    assert!(
        !shown.contains("iVBORw0KGgo"),
        "base64 never belongs in the transcript"
    );
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn steer_then_follow_up_with_ctrl_q_preserves_live_draft() {
    drive("ctrlq");
}
#[test]
fn alt_enter_queues_follow_up_without_aborting() {
    drive("alt");
}
#[test]
fn early_settled_event_does_not_finish_while_messages_are_queued() {
    drive("early_settled");
}
#[test]
fn last_queue_update_after_settled_allows_completion() {
    drive("settle_before_drain");
}
#[test]
fn queued_rejection_does_not_auto_retry() {
    drive("reject");
}
#[test]
fn queued_rejection_preserves_newer_live_typing() {
    drive("reject_late");
}
#[test]
fn early_steering_is_held_until_original_prompt_is_accepted() {
    drive("slow_start");
}
#[test]
fn esc_clears_queued_message_and_preserves_unsent_draft() {
    drive("abort");
}
