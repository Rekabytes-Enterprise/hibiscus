mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn explore_spinner_moves_even_when_pi_is_silent_and_stops_on_edit() {
    let root = unique_temp_dir("hibiscus-explore-spinner");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '%s\n' '{"type":"tool_execution_start","toolCallId":"r1","toolName":"read","args":{"path":"src/main.rs"}}'
   printf '%s\n' '{"type":"tool_execution_end","toolCallId":"r1","toolName":"read","isError":false}'
   sleep 1
   printf '%s\n' '{"type":"tool_execution_start","toolCallId":"e1","toolName":"edit","args":{"path":"src/main.rs"}}'
   printf '%s\n' '{"type":"tool_execution_end","toolCallId":"e1","toolName":"edit","isError":false,"result":{"details":{"diff":"-old\n+new"}}}'
   printf '%s\n' '{"type":"agent_settled"}' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"review\r");
    tty.wait_text("◐ Exploring · latest: src/main.rs");
    tty.wait_text("◓ Exploring · latest: src/main.rs");
    tty.wait_text("✓ Explored · latest: src/main.rs");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(shown.contains("✓ Read · 1 file"));
    assert!(shown.contains("diff · src/main.rs"));
    let frames = support::screen::snapshots(&shown);
    let final_frame = frames
        .iter()
        .rev()
        .find(|part| part.contains("Enter send"))
        .unwrap();
    assert!(!final_frame.contains("◓ Exploring") && !final_frame.contains("◐ Exploring"));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
