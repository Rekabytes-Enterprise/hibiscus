mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn multiline_composer_sends_one_prompt_with_embedded_newlines() {
    let root = unique_temp_dir("hibiscus-multiline");
    let pi = root.join("pi");
    let log = root.join("prompts");
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*)
   printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '%s\n' "$line" >> "$HIBISCUS_TEST_PROMPTS"
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"MULTILINE_REPLY"}}\n'
   printf '{"type":"agent_settled"}\n' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_TEST_PROMPTS", &log);
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send · Ctrl+Enter");
    tty.send(b"one\x1b[13;5utwo\x1b[27;5;13~three\nfour\nfive\nsix");
    tty.wait_text("six");
    assert!(!log.exists(), "newlines must not submit the draft");
    tty.send(b"\x1b[A\x1b[B\r");
    tty.wait_text("MULTILINE_REPLY");
    tty.wait_text("Enter send · Ctrl+Enter");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let prompts = fs::read_to_string(&log).unwrap();
    let lines: Vec<_> = prompts.lines().collect();
    assert_eq!(lines.len(), 1);
    let prompt: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(prompt["message"], "one\ntwo\nthree\nfour\nfive\nsix");
    assert!(shown.contains("\x1b[>1u"));
    assert!(shown.contains("\x1b[<u"));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
