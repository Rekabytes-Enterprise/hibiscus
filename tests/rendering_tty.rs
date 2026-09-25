mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn burst_streaming_keeps_composer_steady_typing_immediate_and_resize_clean() {
    let root = unique_temp_dir("hibiscus-render");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/bin/sh
turn=0
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"prompt"'*)
   turn=$((turn+1))
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   if [ "$turn" -eq 1 ]; then
    i=0
    while [ "$i" -lt 200 ]; do
     printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"stream "}}'
     i=$((i+1))
    done
    printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"BURST_DONE"}}'
   else
    printf '%s\n' "$line" > "$RENDER_PROMPT"
    printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"SECOND_DONE"}}' '{"type":"agent_settled"}'
   fi ;;
  *'"type":"clear_queue"'*) printf '{"type":"response","id":"%s","success":true}\n' "$id" ;;
  *'"type":"abort"'*) printf '{"type":"response","id":"%s","success":true}\n' "$id"; printf '%s\n' '{"type":"agent_settled"}' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("RENDER_PROMPT", root.join("prompt"))
        .env("HIBISCUS_NO_UPDATE_CHECK", "1")
        .env_remove("HIBISCUS_NO_SYNC_UPDATE");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"begin\r");
    tty.wait_text("BURST_DONE");
    tty.send(b"draft\x1b[D!");
    tty.wait_text("draf!t");
    tty.send(b"\x1b");
    tty.wait_text("Stopped. You can keep chatting.");
    tty.wait_text("Enter send");
    tty.send(b"\x1b[F\none\ntwo\nthree\nfour\nfive");
    tty.wait_text("five");
    // Idle resize must repaint a five-row, scrolled composer without losing
    // its draft or requiring an extra keypress.
    tty.resize(60, 18);
    tty.wait_text("\x1b[18;1H");
    tty.resize(80, 24);
    tty.wait_text("\x1b[24;1H");
    tty.send(b"\r");
    tty.wait_text("SECOND_DONE");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let output = tty.finish();
    let frames: Vec<_> = output
        .split("\x1b[?2026h")
        .skip(1)
        .filter_map(|part| part.split_once("\x1b[?2026l").map(|(frame, _)| frame))
        .collect();
    let burst: Vec<_> = frames
        .iter()
        .take_while(|frame| !frame.contains("BURST_DONE"))
        .filter(|frame| frame.contains("stream "))
        .collect();
    assert!(
        burst.len() < 50,
        "200 streamed chunks must be coalesced, not painted individually: {}",
        burst.len()
    );
    for frame in &burst {
        assert!(
            !frame.contains("\x1b[21;1H"),
            "stream updates should not repaint unchanged input"
        );
        assert!(
            !frame.contains("?25"),
            "no visibility toggling during streaming"
        );
        assert!(
            frame.ends_with("\x1b[21;7H"),
            "park the cursor at the composer"
        );
    }
    let prompt: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("prompt")).unwrap()).unwrap();
    assert_eq!(prompt["message"], "draf!t\none\ntwo\nthree\nfour\nfive");
    assert!(output.ends_with("\x1b[0 q"));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
