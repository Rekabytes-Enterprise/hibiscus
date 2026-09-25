mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn fullscreen_paints_aubergine_restores_terminal_and_respects_no_color() {
    let root = unique_temp_dir("hibiscus-background");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"model":{"provider":"demo","id":"small"}}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"A reply"}}\n{"type":"agent_settled"}\n' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    for color in [true, false] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
        command
            .env("HIBISCUS_PI", &pi)
            .env("HIBISCUS_NO_UPDATE_CHECK", "1");
        let mut tty = if color {
            Pty::spawn_with_color(&mut command, true)
        } else {
            Pty::spawn(&mut command)
        };
        tty.wait_text("Enter send");
        tty.send(b"hello\r");
        tty.wait_text("A reply");
        tty.wait_text("Enter send");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        if color {
            assert!(
                shown.contains("\x1b[?1049h\x1b[2 q\x1b[>1u\x1b[48;2;48;10;36m\x1b[2J"),
                "background must precede clear"
            );
            assert!(
                shown.contains("\x1b[48;2;57;30;46m"),
                "user row keeps its lighter tint"
            );
            assert!(
                shown.contains("\x1b[0m\x1b[48;2;48;10;36m\x1b[K"),
                "erased space uses base after style reset"
            );
            assert!(
                shown.contains("\x1b[0m\x1b[?25h\x1b[?1049l"),
                "restore terminal style on exit"
            );
            assert!(
                shown.contains("\x1b[?1049l\x1b[0 q"),
                "restore default cursor style on exit"
            );
        } else {
            assert!(!shown.contains("\x1b[48;2;48;10;36m"));
            assert!(!shown.contains("\x1b[48;2;57;30;46m"));
        }
        drop(tty);
    }
    fs::remove_dir_all(root).unwrap();
}
