mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn animated_work_flower_keeps_steady_cursor_visible_until_agent_settles() {
    let root = unique_temp_dir("hibiscus-working-cursor");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"thinking_start"}}'
   sleep 1
   printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"thinking_end"}}' '{"type":"agent_settled"}' ;;
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
    tty.send(b"think\r");
    tty.wait_text("✿ Thinking…");
    tty.wait_text("❀ Thinking…");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let frames = shown
        .split("\x1b[?2026h")
        .skip(1)
        .filter_map(|part| part.split_once("\x1b[?2026l").map(|(frame, _)| frame))
        .collect::<Vec<_>>();
    assert!(
        shown.contains("\x1b[?1049h\x1b[2 q"),
        "request a steady cursor in the alternate screen"
    );
    assert!(
        shown.contains("\x1b[?1049l\x1b[0 q"),
        "restore terminal default cursor style"
    );
    let thinking = frames
        .iter()
        .filter(|frame| frame.contains("Thinking…"))
        .collect::<Vec<_>>();
    assert!(thinking.len() >= 2);
    assert!(
        thinking.iter().all(|frame| frame.ends_with("\x1b[21;7H")
            && !frame.contains("?25")
            && !frame.contains("│ ❯")),
        "the cursor must stay parked and the unchanged composer must not repaint during work"
    );
    assert!(
        shown.contains("\x1b[?25h") && frames.iter().any(|frame| frame.contains("Enter send")),
        "cursor must return for idle typing"
    );
    assert!(
        !shown.contains("  · thinking ·"),
        "raw thinking is not a chat row"
    );
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
