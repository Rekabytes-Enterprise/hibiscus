#[allow(dead_code)]
mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command, time::Instant};
use support::{unique_temp_dir, Pty};

#[test]
fn large_history_paste_preserves_input_and_uses_batched_frames() {
    let root = unique_temp_dir("hibiscus-large-history");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/usr/bin/env node
const fs = require('node:fs');
const send = data => console.log(JSON.stringify(data));
let turn = 0;
require('node:readline').createInterface({input:process.stdin}).on('line', line => {
 const c = JSON.parse(line);
 send({type:'response',id:c.id,success:true,data:{}});
 if(c.type === 'prompt') {
  if(++turn === 1) {
   send({type:'message_update',assistantMessageEvent:{type:'text_delta',delta:'Some ordinary **bold** prose with `code` and words for the terminal.\n'.repeat(3900)+'HISTORY_DONE'}});
  } else {
   fs.writeFileSync(process.env.PERF_PROMPT, c.message);
   send({type:'message_update',assistantMessageEvent:{type:'text_delta',delta:'PASTE_SENT'}});
  }
  send({type:'agent_settled'});
 }
});
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1")
        .env("PERF_PROMPT", root.join("prompt"));
    let mut tty = Pty::spawn_with_color(&mut command, true);
    tty.wait_text("Enter send");
    tty.send(b"go\r");
    tty.wait_text("HISTORY_DONE");
    tty.wait_text("Enter send");
    let draft = format!("{}PASTE_DONE", "x".repeat(512));
    let started = Instant::now();
    tty.send(draft.as_bytes());
    tty.wait_text("PASTE_DONE");
    eprintln!(
        "256 KiB history, 521-byte paste: {:?} (PTY harness polling included)",
        started.elapsed()
    );
    tty.send(b"\r");
    tty.wait_text("PASTE_SENT");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let output = tty.finish();
    assert_eq!(fs::read_to_string(root.join("prompt")).unwrap(), draft);
    let frames = output
        .split("\x1b[?2026h")
        .filter(|frame| frame.contains("❯ x") || frame.contains("│   x"))
        .count();
    assert!(
        frames < 100,
        "521 bytes should not produce one frame per character: {frames}"
    );
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
