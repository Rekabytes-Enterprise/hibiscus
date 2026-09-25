#[allow(dead_code)]
mod support;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};
use support::{unique_temp_dir, Pty};

fn fullscreen(color: bool) {
    let root = unique_temp_dir("hibiscus-diagnostics");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/usr/bin/env python3
import json,os,sys,threading,time
root=os.environ['DIAG_ROOT']
lock=threading.Lock()
def send(value):
 with lock: print(json.dumps(value),flush=True)
def warning(text): sys.stderr.write(text+'\n');sys.stderr.flush()
def gate(name):
 end=time.monotonic()+12
 while not os.path.exists(root+'/'+name):
  if time.monotonic()>end:raise RuntimeError('fixture gate timeout')
  time.sleep(.005)
def background():
 gate('idle');warning('\x1b]0;PRIVATE_TITLE\x07IDLE_WARNING api_key=PRIVATE_KEY')
 gate('working');warning('WORK_WARNING {"type":"agent_settled"}')
 gate('dialog');send({'type':'extension_ui_request','id':'confirm','method':'confirm','title':'Diagnostic confirmation','message':'Cancel safely','timeout':10000})
 gate('modal');warning('MODAL_WARNING')
warning('\x1b[31mWarning: No models match pattern "minimax/*"\x1b[0m')
threading.Thread(target=background,daemon=True).start()
for line in sys.stdin:
 c=json.loads(line)
 with open(root+'/commands','a') as log:log.write(json.dumps(c)+'\n')
 if c['type']=='get_state':gate('state-ready')
 if c['type']=='extension_ui_response':
  send({'type':'agent_settled'});continue
 send({'type':'response','id':c['id'],'success':True,'data':{}})
 if c['type']=='prompt':send({'type':'message_update','assistantMessageEvent':{'type':'thinking_start'}})
 elif c['type']=='abort':send({'type':'agent_settled'})
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("DIAG_ROOT", &root)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn_with_color(&mut command, color);
    tty.wait_text("Pi: Warning: No models match pattern");
    fs::write(root.join("state-ready"), "").unwrap();
    tty.send(b"keep");
    tty.wait_text("keep");
    fs::write(root.join("idle"), "").unwrap();
    tty.wait_text("Pi: IDLE_WARNING");
    tty.send(b"\r");
    tty.wait_text("Thinking…");
    tty.send(b"next");
    tty.wait_text("next");
    fs::write(root.join("working"), "").unwrap();
    tty.wait_text("Pi: WORK_WARNING");
    tty.send(b"\x1b");
    tty.wait_text("Stopped. You can keep chatting.");
    tty.wait_text("Enter send");
    tty.send(b"\r");
    tty.wait_text("Thinking…");
    tty.send(b"draft preserved");
    tty.wait_text("draft preserved");
    fs::write(root.join("dialog"), "").unwrap();
    tty.wait_text("Diagnostic confirmation");
    fs::write(root.join("modal"), "").unwrap();
    tty.wait_text("Pi: MODAL_WARNING");
    tty.resize(100, 18);
    tty.wait_text("\x1b[18;1H");
    tty.resize(160, 24);
    tty.wait_text("\x1b[24;1H");
    tty.send(b"\x1b");
    tty.wait_text("Enter send");
    tty.wait_text("draft preserved");
    tty.send(b"\x03/quit\r");
    let shown = tty.finish();
    for secret in ["PRIVATE_KEY", "PRIVATE_TITLE", "\x1b[31m", "\x1b]0;"] {
        assert!(!shown.contains(secret));
    }
    let frames = support::screen::snapshots(&shown);
    for (marker, draft) in [
        ("Warning: No models", ""),
        ("IDLE_WARNING", "keep"),
        ("WORK_WARNING", "next"),
        ("MODAL_WARNING", "Respond in the dialog above"),
    ] {
        let frame = frames
            .iter()
            .find(|frame| {
                frame
                    .lines()
                    .nth(23)
                    .is_some_and(|row| row.contains(marker))
            })
            .unwrap();
        assert!(
            frame.lines().nth(20).unwrap().contains(draft),
            "diagnostics must not replace composer"
        );
        assert!(
            frame.lines().nth(21).unwrap().contains('╰'),
            "warning belongs below composer bottom"
        );
        assert!(
            !frame.lines().take(23).any(|row| row.contains(marker)),
            "warning must not escape its row"
        );
    }
    assert!(frames.iter().any(|frame| frame
        .lines()
        .nth(17)
        .is_some_and(|line| line.contains("Pi: MODAL_WARNING"))));
    let commands: Vec<serde_json::Value> = fs::read_to_string(root.join("commands"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let prompts = commands
        .iter()
        .filter(|c| c["type"] == "prompt")
        .map(|c| c["message"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(prompts, ["keep", "next"]);
    assert!(commands
        .iter()
        .any(|c| c["type"] == "extension_ui_response" && c["confirmed"] != true));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stderr_is_below_composer_during_startup_typing_streaming_and_modals() {
    fullscreen(true);
}
#[test]
fn stderr_warning_row_respects_no_color() {
    fullscreen(false);
}

#[test]
fn reconnect_keeps_diagnostics_routed_to_the_warning_row() {
    let root = unique_temp_dir("hibiscus-diagnostics-reconnect");
    let pi = root.join("pi");
    fs::write(
        &pi,
        r#"#!/usr/bin/env python3
import json,os,sys
marker=os.environ['DIAG_ROOT']+'/started'
second=os.path.exists(marker)
open(marker,'a').close()
sys.stderr.write(('RECONNECTED_WARNING' if second else 'FIRST_WARNING')+'\n');sys.stderr.flush()
for line in sys.stdin:
 c=json.loads(line)
 if c['type']=='prompt':sys.exit(1)
 print(json.dumps({'type':'response','id':c['id'],'success':True,'data':{}}),flush=True)
"#,
    )
    .unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("DIAG_ROOT", &root)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Pi: FIRST_WARNING");
    tty.send(b"break\r");
    tty.wait_text("Pi disconnected");
    tty.send(b"/reconnect\r");
    tty.wait_text("Pi: RECONNECTED_WARNING");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(support::screen::snapshots(&shown).iter().any(|frame| frame
        .lines()
        .nth(23)
        .is_some_and(|row| row.contains("Pi: RECONNECTED_WARNING"))));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn one_shot_and_piped_chat_keep_diagnostics_on_stderr_and_stdout_plain() {
    let root = unique_temp_dir("hibiscus-diagnostics-plain");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/usr/bin/env python3
import sys,json
sys.stderr.write('Warning: line-mode diagnostic\n');sys.stderr.flush()
for line in sys.stdin:
 c=json.loads(line)
 print(json.dumps({'type':'response','id':c['id'],'success':True}),flush=True)
 print(json.dumps({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'PLAIN_REPLY'}}),flush=True)
 print('{"type":"agent_settled"}',flush=True)
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    for piped in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
        command.env("HIBISCUS_PI", &pi);
        let output = if piped {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"hello\n/quit\n")
                .unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.arg("hello").output().unwrap()
        };
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "PLAIN_REPLY\n");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "Warning: line-mode diagnostic\n"
        );
    }
    fs::remove_dir_all(root).unwrap();
}
