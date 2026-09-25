mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn new_session_is_blank_only_after_pi_confirms_creation() {
    for outcome in ["success", "cancel", "failure"] {
        let root = unique_temp_dir("hibiscus-blank-session");
        let saved = root.join("old.jsonl");
        fs::write(&saved, "saved history must stay intact\n").unwrap();
        let pi = root.join("pi");
        fs::write(&pi, r#"#!/bin/sh
printf 'start\n' >> "$FIXTURE/starts"
session=OLD_SESSION
while IFS= read -r line; do
 printf '%s\n' "$line" >> "$FIXTURE/commands"
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*)
   printf '{"type":"response","id":"%s","success":true,"data":{"sessionName":"%s","sessionFile":"%s/old.jsonl","model":{"provider":"test","id":"model"}}}\n' "$id" "$session" "$FIXTURE" ;;
  *'"type":"new_session"'*)
   case "$NEW_OUTCOME" in
    success) session=NEW_SESSION; printf '{"type":"response","id":"%s","success":true,"data":{"cancelled":false}}\n' "$id" ;;
    cancel) printf '{"type":"response","id":"%s","success":true,"data":{"cancelled":true}}\n' "$id" ;;
    failure) printf '{"type":"response","id":"%s","success":false,"error":"Could not create session"}\n' "$id" ;;
   esac ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   case "$line" in
    *FAILED_PROMPT*) printf '{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"subscription exhausted"}}\n' ;;
    *) printf '{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"OLD_REPLY"}]}}\n' ;;
   esac
   printf '{"type":"agent_settled"}\n' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
        fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
        command
            .env("HIBISCUS_PI", &pi)
            .env("FIXTURE", &root)
            .env("NEW_OUTCOME", outcome)
            .env("HIBISCUS_NO_UPDATE_CHECK", "1");
        let mut tty = Pty::spawn(&mut command);
        tty.wait_text("OLD_SESSION");
        tty.send(b"OLD_QUESTION\r");
        tty.wait_text("OLD_REPLY");
        tty.wait_text("Enter send");
        tty.send(b"FAILED_PROMPT\r");
        tty.wait_text("Use /restore to review");
        tty.wait_text("Enter send");
        tty.send(b"/new\r");
        if outcome == "success" {
            tty.wait_text("NEW_SESSION");
        } else if outcome == "cancel" {
            tty.wait_text("Cancelled new_session");
        } else {
            tty.wait_text("Could not create session");
        }
        tty.wait_text("Enter send");
        tty.send(b"/restore\r");
        if outcome == "success" {
            tty.wait_text("No failed prompt to restore.");
        } else {
            tty.wait_text("Restored failed prompt.");
        }
        tty.wait_text("Enter send");
        tty.send(b"\x03/quit\r");
        let shown = tty.finish();
        if outcome == "success" {
            let frames = support::screen::snapshots(&shown);
            let blank = frames
                .iter()
                .find(|frame| frame.contains("NEW_SESSION"))
                .unwrap();
            for old in [
                "OLD_QUESTION",
                "OLD_REPLY",
                "FAILED_PROMPT",
                "Started new session.",
                "/new",
            ] {
                assert!(!blank.contains(old), "new screen retained {old}: {blank}");
            }
            assert!(blank.contains("│ ❯ "));
            assert!(blank.contains("test/model"));
        } else {
            let error = if outcome == "cancel" {
                "Cancelled new_session"
            } else {
                "Could not create session"
            };
            let frames = support::screen::snapshots(&shown);
            let preserved = frames.iter().find(|frame| frame.contains(error)).unwrap();
            assert!(preserved.contains("OLD_REPLY"));
            assert!(preserved.contains("OLD_SESSION"));
            assert!(!shown.contains("NEW_SESSION"));
        }
        assert_eq!(
            fs::read_to_string(&saved).unwrap(),
            "saved history must stay intact\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("starts"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        let commands = fs::read_to_string(root.join("commands")).unwrap();
        assert_eq!(commands.matches("\"type\":\"new_session\"").count(), 1);
        drop(tty);
        fs::remove_dir_all(root).unwrap();
    }
}
