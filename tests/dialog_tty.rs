use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
mod support;
use std::time::{SystemTime, UNIX_EPOCH};
use support::Pty;

#[test]
fn confirm_can_be_approved_in_chat() {
    let root = std::env::temp_dir().join(format!(
        "hibiscus-dialog-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let script = root.join("pi");
    let log = root.join("reply");
    fs::write(&script, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_session_stats"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"sessionName":"test","model":{"provider":"demo","id":"small"}}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '{"type":"extension_ui_request","id":"ui-1","method":"confirm","title":"Approve tool","message":"Run it?"}\n'
   ;;
  *'"type":"extension_ui_response"'*)
   printf '%s\n' "$line" > "$HIBISCUS_TEST_REPLY"
   printf '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"ok"}}\n{"type":"agent_settled"}\n'
   ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &script)
        .env("HIBISCUS_TEST_REPLY", &log);
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"hello\r");
    tty.wait_text("Approve tool");
    tty.send(b"\x1b[B\r"); // Choose Confirm in the docked modal.
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    let reply: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&log).unwrap()).unwrap();
    assert_eq!(reply["type"], "extension_ui_response");
    assert_eq!(reply["id"], "ui-1");
    assert_eq!(reply["confirmed"], true);
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
