mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn escape_interrupts_a_blocked_large_image_send_and_restore_is_explicit() {
    blocked_send(false);
}

#[test]
fn escape_during_queued_image_send_retains_the_uncertain_submission() {
    blocked_send(true);
}

fn blocked_send(queued: bool) {
    let root = unique_temp_dir("hibiscus-blocked-send");
    let pi = root.join("pi");
    let image = root.join("image.png");
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.resize(2 * 1024 * 1024, 0);
    fs::write(&image, bytes).unwrap();
    for name in ["wl-paste", "xclip", "osascript", "powershell.exe"] {
        let helper = root.join(name);
        fs::write(&helper, "#!/bin/sh\nif [ \"$1\" = --list-types ]; then printf 'image/png\\n'; else cat \"$PRESSURE_IMAGE\"; fi\n").unwrap();
        fs::set_permissions(helper, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        &pi,
        r#"#!/usr/bin/env node
const fs = require('node:fs');
const reader = require('node:readline').createInterface({input:process.stdin});
reader.on('line', line => {
 const c = JSON.parse(line);
 fs.appendFileSync(process.env.PRESSURE_LOG, line+'\n');
 if(c.type === 'get_state') {
  console.log(JSON.stringify({type:'response',id:c.id,success:true,data:{}}));
  if(process.env.PRESSURE_QUEUED !== '1') { reader.pause(); setInterval(() => {}, 1000); }
 } else if(c.type === 'prompt') {
  console.log(JSON.stringify({type:'response',id:c.id,success:true}));
  console.log(JSON.stringify({type:'message_update',assistantMessageEvent:{type:'thinking_start'}}));
  reader.pause(); setInterval(() => {}, 1000);
 }
});
"#,
    )
    .unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1")
        .env("PRESSURE_LOG", root.join("commands"))
        .env("PRESSURE_IMAGE", &image)
        .env("PRESSURE_QUEUED", if queued { "1" } else { "0" })
        .env("WAYLAND_DISPLAY", "fixture")
        .env_remove("WSL_DISTRO_NAME")
        .env_remove("WSL_INTEROP")
        .env(
            "PATH",
            format!("{}:{}", root.display(), std::env::var("PATH").unwrap()),
        );
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    if queued {
        tty.send(b"first\r");
        tty.wait_text("Thinking…");
    }
    tty.send(b"large prompt\x16");
    tty.wait_text("1 image(s) attached");
    tty.send(b"\r");
    tty.wait_text(if queued {
        "Sending steering message…"
    } else {
        "Sending prompt…"
    });
    tty.send(b"\x1b");
    tty.wait_text("Pi disconnected");
    tty.send(if queued {
        b"\x03/restore\r"
    } else {
        b"/restore\r"
    });
    tty.wait_text("1 image(s) attached");
    tty.send(b"\x03/quit\r");
    let shown = tty.finish();
    let log = fs::read_to_string(root.join("commands")).unwrap();
    assert_eq!(
        log.lines().count(),
        if queued { 2 } else { 1 },
        "no completed prompt, abort appended to a partial record, or automatic replay"
    );
    assert!(!shown.contains("iVBORw0KGgo"));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn overload_during_modal_never_sends_approval_and_reconnect_does_not_replay() {
    let root = unique_temp_dir("hibiscus-rpc-pressure");
    let pi = root.join("pi");
    let log = root.join("commands");
    fs::write(&pi, r#"#!/usr/bin/env node
const fs = require('node:fs');
const send = record => process.stdout.write(JSON.stringify(record)+'\n');
process.stdout.on('error', () => fs.writeFileSync(process.env.PRESSURE_READY, 'reader closed'));
require('node:readline').createInterface({input:process.stdin}).on('line', line => {
 const c = JSON.parse(line);
 fs.appendFileSync(process.env.PRESSURE_LOG, line+'\n');
 if(c.type === 'get_state') {
  send({type:'response',id:c.id,success:true,data:{sessionFile:process.env.PRESSURE_SESSION}});
 } else if(c.type === 'prompt') {
  send({type:'response',id:c.id,success:true});
  send({type:'extension_ui_request',id:'approval',method:'confirm',title:'Pressure approval',message:'Never authorize after overload',timeout:10000});
  setTimeout(() => {
   for(let i=0;i<100;i++) send({type:'future_event',payload:'PRIVATE_PRESSURE_DATA'.repeat(5000)});
  },100);
 }
});
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1")
        .env("HIBISCUS_RPC_BUFFER_MIB", "1")
        .env("HIBISCUS_RPC_MAX_RECORDS", "4096")
        .env("PRESSURE_LOG", &log)
        .env("PRESSURE_SESSION", root.join("session.jsonl"))
        .env("PRESSURE_READY", root.join("ready"));
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"test pressure\r");
    tty.wait_text("Pressure approval");
    // EPIPE confirms the bounded reader invalidated the connection. The modal
    // deliberately keeps the consumer busy while the independent reader drains.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !root.join("ready").exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    tty.send(b"y\r");
    tty.wait_text("Pi disconnected");
    tty.send(b"/reconnect\r");
    tty.wait_text("Reconnected");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let commands: Vec<serde_json::Value> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        commands.iter().filter(|c| c["type"] == "prompt").count(),
        1,
        "no automatic replay"
    );
    assert!(!commands
        .iter()
        .any(|c| c["type"] == "extension_ui_response" && c["confirmed"] == true));
    assert!(!shown.contains("PRIVATE_PRESSURE_DATA"));
    assert!(commands.iter().filter(|c| c["type"] == "get_state").count() >= 2);
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
