mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

fn scenario(mode: &str) {
    let root = unique_temp_dir("hibiscus-loop-stop");
    let pi = root.join("pi");
    let log = root.join("marker-log");
    fs::write(&pi, r#"#!/usr/bin/env python3
import json,sys,os
args=sys.argv[1:];flag=os.path.join(os.path.dirname(args[args.index('--extension')+1]),'loop-active')
def send(v):print(json.dumps(v,separators=(',',':')),flush=True)
for line in sys.stdin:
 c=json.loads(line); kind=c['type']; ident=c.get('id')
 if kind=='get_state':data={'model':{'provider':'mock','id':'test'}}
 elif kind=='get_session_stats':data={}
 elif kind=='get_messages':data={'messages':[{'role':'assistant','content':[{'type':'text','text':'Missing browser prerequisites.'}]}]}
 elif kind=='prompt':
  loop=c['message'].startswith('Hibiscus unattended /loop')
  with open(os.environ['LOOP_LOG'],'a') as f:f.write('loop='+str(loop)+' marker='+str(os.path.exists(flag))+'\n')
  send({'type':'response','id':ident,'success':True})
  if not loop:send({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'BACK_TO_CHAT'}});send({'type':'agent_settled'})
  elif os.environ['LOOP_SCENARIO']=='blocked':
   send({'type':'tool_execution_end','toolCallId':'loopstatus','toolName':'loop_status','isError':False,'result':{'details':{'hibiscusLoop':{'state':'blocked','reason':'Missing browser prerequisites'}}}})
   send({'type':'message_end','message':{'role':'assistant','stopReason':'stop','content':[{'type':'text','text':'Missing browser prerequisites.'}]}})
   send({'type':'agent_settled'})
  continue
 elif kind in ('abort','clear_queue'):
  send({'type':'response','id':ident,'success':True})
  if kind=='abort':send({'type':'message_end','message':{'role':'assistant','stopReason':'aborted'}});send({'type':'agent_settled'})
  continue
 else:sys.exit(13)
 send({'type':'response','id':ident,'success':True,'data':data})
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut tty = Pty::spawn(
        Command::new(env!("CARGO_BIN_EXE_hibiscus"))
            .env("HIBISCUS_PI", &pi)
            .env("LOOP_LOG", &log)
            .env("LOOP_SCENARIO", mode)
            .env("HIBISCUS_NO_UPDATE_CHECK", "1"),
    );
    tty.wait_text("Enter send");
    tty.send(b"/loop test the UI\r");
    if mode == "blocked" {
        tty.wait_text("⚠ Loop blocked · Missing browser prerequisites");
    } else {
        tty.wait_text("Sending prompt");
        tty.send(b"\x1b");
        tty.wait_text("Loop stopped by Esc; approvals restored");
    }
    tty.send(b"after\r");
    tty.wait_text("BACK_TO_CHAT");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "loop=True marker=True\nloop=False marker=False\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn esc_stops_loop_and_restores_normal_approval_mode() {
    scenario("esc");
}
#[test]
fn reported_blocker_stops_loop_and_restores_normal_approval_mode() {
    scenario("blocked");
}
