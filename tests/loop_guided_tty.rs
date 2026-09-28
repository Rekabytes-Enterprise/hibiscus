mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

fn run(continue_once: bool) {
    let root = unique_temp_dir("hibiscus-guided-loop");
    let pi = root.join("pi");
    let log = root.join("commands.jsonl");
    fs::write(&pi, r#"#!/usr/bin/env python3
import json,sys,os
args=sys.argv[1:];flag=os.path.join(os.path.dirname(args[args.index('--extension')+1]),'loop-active')
rounds=0
def send(v): print(json.dumps(v),flush=True)
for line in sys.stdin:
 c=json.loads(line);kind=c['type'];ident=c.get('id')
 with open(os.environ['GUIDED_LOG'],'a') as f:f.write(json.dumps({'command':c,'bypass':os.path.exists(flag)})+'\n')
 if kind=='get_state':data={'model':{'provider':'fake','id':'test'}}
 elif kind=='get_session_stats':data={}
 elif kind=='prompt':
  send({'type':'response','id':ident,'success':True})
  loop=c['message'].startswith('Hibiscus unattended /loop')
  if loop:
   rounds+=1
   assert 'guided mode' in c['message']
   assert '--reporter=json' not in c['message']
   assert 'No special test names' in c['message']
   assert "screenshot: 'only-on-failure'" in c['message'] and "trace: 'retain-on-failure'" in c['message']
   if not os.environ.get('CONTINUE_ONCE') or rounds>1:
    send({'type':'tool_execution_end','toolCallId':'signal','toolName':'loop_status','isError':False,'result':{'details':{'hibiscusLoop':{'state':'candidate_complete'}}}})
   send({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'MODEL_REPORT: task complete' if rounds>1 else 'MODEL_REPORT: progress'}})
  else:send({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'NORMAL_CHAT_OK'}})
  send({'type':'agent_settled'})
  continue
 else:sys.exit(13) # guided mode must not ask for strict get_messages/reviewer work
 send({'type':'response','id':ident,'success':True,'data':data})
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("GUIDED_LOG", &log)
        .env("HIBISCUS_AUTH_SDK", root.join("missing-sdk.mjs"))
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    if continue_once {
        command.env("CONTINUE_ONCE", "1");
    } else {
        command.env_remove("CONTINUE_ONCE");
    }
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"/loop --strict\r");
    tty.wait_text("Usage: /loop [--strict]");
    tty.send(b"/loop exercise the app\r");
    tty.wait_text("Pi reports the requested work and tests complete");
    tty.send(b"after\r");
    tty.wait_text("NORMAL_CHAT_OK");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("browser UAT passed"));
    assert!(
        !shown.contains("UAT 0/"),
        "guided mode must not imply machine evidence checks"
    );
    assert!(
        !shown.contains("Loop blocked"),
        "missing reviewer SDK is not needed for guided mode"
    );
    let records: Vec<serde_json::Value> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let prompts: Vec<_> = records
        .iter()
        .filter(|r| r["command"]["type"] == "prompt")
        .collect();
    assert_eq!(prompts.len(), if continue_once { 3 } else { 2 });
    for record in &prompts[..prompts.len() - 1] {
        assert_eq!(record["bypass"], true);
    }
    assert_eq!(prompts.last().unwrap()["bypass"], false);
    assert!(!records
        .iter()
        .any(|r| r["command"]["type"] == "get_messages"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn guided_loop_finishes_on_pi_report_without_an_sdk_or_strict_artifacts() {
    run(false);
}
#[test]
fn guided_loop_continues_until_explicit_completion_then_restores_approvals() {
    run(true);
}
