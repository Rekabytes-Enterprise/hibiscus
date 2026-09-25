mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn wide_terminal_and_resizes_expand_composer_without_losing_draft() {
    let root = unique_temp_dir("hibiscus-wide-terminal");
    let pi = root.join("pi");
    let prompt = root.join("prompt");
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '%s\n' "$line" > "$WIDE_PROMPT"
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"Wide reply"}}' '{"type":"agent_settled"}' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("WIDE_PROMPT", &prompt)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    for columns in [160, 60, 200] {
        tty.resize(columns, 24);
        tty.wait_text("\x1b[24;1H");
    }
    let draft = "x".repeat(180);
    tty.send(draft.as_bytes());
    tty.wait_text(&"x".repeat(150));
    tty.send(b"\r");
    tty.wait_text("Wide reply");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let output = tty.finish();
    let frames = support::screen::snapshots(&output);
    for width in [76, 156, 56, 196] {
        let border = format!("╭{}╮", "─".repeat(width - 2));
        assert!(
            frames.iter().any(|view| view.contains(&border)),
            "no {width}-column composer border"
        );
    }
    let wide = frames
        .iter()
        .find(|view| view.contains(&format!("❯ {}", "x".repeat(150))))
        .unwrap();
    let input = wide
        .lines()
        .find(|row| row.contains(&format!("❯ {}", "x".repeat(150))))
        .unwrap();
    assert!(
        input.starts_with("  │ ❯ "),
        "composer should retain a two-column margin"
    );
    assert!(
        input.contains(&draft),
        "180 characters must fit in one row at 200 columns"
    );
    assert!(wide.contains(&format!("╭{}╮", "─".repeat(194))));
    let sent: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&prompt).unwrap()).unwrap();
    assert_eq!(sent["message"], draft);
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
