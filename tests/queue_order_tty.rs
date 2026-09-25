mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn accepted_steering_ack_after_final_settlement_does_not_hang_or_replay() {
    drive("ack_last");
}

#[test]
fn late_ack_still_waits_for_a_nonempty_pi_queue_to_drain() {
    drive("queue_update_last");
}

fn drive(scenario: &str) {
    let root = unique_temp_dir("hibiscus-late-queue-ack");
    let pi = root.join("pi");
    let log = root.join("prompts");
    fs::write(&pi, r#"#!/usr/bin/env python3
import json,os,sys,time
for line in sys.stdin:
 c=json.loads(line)
 kind=c['type'];id=c['id']
 def send(value): print(json.dumps(value),flush=True)
 if kind=='get_state':send({'type':'response','id':id,'success':True,'data':{}})
 elif kind=='prompt':
  with open(os.environ['QUEUE_ACK_LOG'],'a') as f:f.write(json.dumps(c)+'\n')
  if c.get('streamingBehavior')=='steer':
   # The queue has drained and the final settlement is already on the wire.
   # A delayed acceptance response is not a new agent run.
   send({'type':'message_start','message':{'role':'user','content':'steer now'}})
   send({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'STEER_DELIVERED'}})
   if os.environ['QUEUE_ACK_SCENARIO']=='queue_update_last':
    send({'type':'queue_update','steering':['steer now'],'followUp':[]})
    send({'type':'agent_settled'})
    send({'type':'response','id':id,'success':True})
    time.sleep(0.2)
    send({'type':'queue_update','steering':[],'followUp':[]})
   else:
    send({'type':'queue_update','steering':[],'followUp':[]})
    send({'type':'agent_settled'})
    send({'type':'response','id':id,'success':True})
  else:
   send({'type':'response','id':id,'success':True})
   if c['message']=='first':
    send({'type':'message_update','assistantMessageEvent':{'type':'thinking_start'}})
   else:
    send({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'NEXT_REPLY'}})
    send({'type':'agent_settled'})
 else:sys.exit(13)
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("QUEUE_ACK_LOG", &log)
        .env("QUEUE_ACK_SCENARIO", scenario)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"first\r");
    tty.wait_text("Thinking…");
    tty.send(b"steer now\r");
    tty.wait_text("STEER_DELIVERED");
    tty.wait_text("Enter send"); // before fix: late acknowledgement reset settled and waits forever
    tty.send(b"next\r");
    tty.wait_text("NEXT_REPLY");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let prompts: Vec<serde_json::Value> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(prompts.len(), 3);
    assert_eq!(prompts[0]["message"], "first");
    assert_eq!(prompts[1]["message"], "steer now");
    assert_eq!(prompts[1]["streamingBehavior"], "steer");
    assert_eq!(prompts[2]["message"], "next");
    let final_view = support::screen::snapshots(&shown).pop().unwrap();
    assert_eq!(final_view.matches("STEER_DELIVERED").count(), 1);
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
