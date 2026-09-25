mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn cold_session_scan_can_be_cancelled_without_losing_typed_draft_or_switching_pi() {
    let root = unique_temp_dir("hibiscus-session-scan");
    let cwd = root.join("project");
    let dir = root.join("sessions");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let slow = dir.join("older.jsonl");
    let latest = dir.join("latest.jsonl");
    let header = |id: &str| {
        format!("{{\"type\":\"session\",\"id\":\"{id}\",\"cwd\":\"{}\"}}\n{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"{id}\"}}}}\n", cwd.display())
    };
    // A valid, unrelated session entry forces a cold full-file scan without
    // requiring sleeps or tampering with a real Pi session directory.
    fs::write(
        &slow,
        format!(
            "{}{{\"type\":\"custom\",\"data\":\"{}\"}}\n",
            header("older"),
            "x".repeat(24 * 1024 * 1024)
        ),
    )
    .unwrap();
    fs::write(&latest, header("latest")).unwrap();
    let log = root.join("commands");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/usr/bin/env python3
import json,os,sys
current='none'
for line in sys.stdin:
 c=json.loads(line)
 if c['type']=='get_state':
  print(json.dumps({'type':'response','id':c['id'],'success':True,'data':{'sessionName':current}}),flush=True)
 elif c['type']=='switch_session':
  current='latest' if c['sessionPath'].endswith('latest.jsonl') else 'older'
  with open(os.environ['SCAN_LOG'],'a') as f:f.write(current+'\n')
  print(json.dumps({'type':'response','id':c['id'],'success':True,'data':{'cancelled':False}}),flush=True)
 elif c['type']=='get_messages':
  print(json.dumps({'type':'response','id':c['id'],'success':True,'data':{'messages':[{'role':'user','content':current},{'role':'assistant','content':[{'type':'text','text':current+' answer'}]}]}}),flush=True)
 elif c['type']=='prompt':
  with open(os.environ['SCAN_LOG'],'a') as f:f.write('UNEXPECTED_PROMPT\n')
  print(json.dumps({'type':'response','id':c['id'],'success':True}),flush=True)
  print('{"type":"agent_settled"}',flush=True)
 else:sys.exit(13)
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .current_dir(&cwd)
        .env("HIBISCUS_PI", &pi)
        .env("PI_CODING_AGENT_SESSION_DIR", &dir)
        .env("SCAN_LOG", &log)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"/sessions\r");
    tty.wait_text("Loading saved sessions");
    tty.send(b"\x1b");
    // On very fast release runners the scan can finish while the PTY driver
    // observes the loading frame. Esc cancels either the scan or its picker.
    tty.send(b"draft");
    tty.wait_text("draft");
    assert!(
        !log.exists(),
        "Esc must not switch a session while scanning"
    );
    tty.send(b"\x03/continue\r");
    tty.wait_text("Loading saved sessions");
    tty.send(b"keep");
    tty.wait_text("latest answer");
    tty.wait_text("keep");
    tty.send(b"\x03/sessions\r");
    tty.wait_text("Saved sessions");
    tty.send(b"\x1b[B\r");
    tty.wait_text("older answer");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert_eq!(fs::read_to_string(&log).unwrap(), "latest\nolder\n");
    assert!(!shown.contains("UNEXPECTED_PROMPT"));
    assert!(!support::screen::snapshots(&shown)
        .last()
        .unwrap()
        .contains("Loading saved sessions"));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
