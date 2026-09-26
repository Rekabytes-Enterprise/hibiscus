mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn pi_state_and_session_stats_drive_header_footer_and_goal_without_invented_context() {
    run(false);
}

#[test]
fn older_pi_without_session_stats_keeps_chat_and_does_not_retry_unknown_command() {
    run(true);
}

fn run(unsupported: bool) {
    let root = unique_temp_dir("hibiscus-pi-context");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/usr/bin/env python3
import json,os,sys,time
thinking='high';stage=0
for line in sys.stdin:
 c=json.loads(line);kind=c['type'];id=c['id']
 with open(os.environ['CONTEXT_LOG'],'a') as log:log.write(json.dumps(c)+'\n')
 def send(data): print(json.dumps(data),flush=True)
 def reply(data=None):send({'type':'response','id':id,'success':True,'data':data})
 if kind=='get_state':reply({'model':{'provider':'demo','id':'reasoner'},'thinkingLevel':thinking,'sessionName':'work'})
 elif kind=='get_session_stats':
  if os.environ['CONTEXT_STATS']=='unsupported':
   send({'type':'response','id':id,'success':False,'error':'Unknown command: get_session_stats'});continue
  if stage==0:ctx={'tokens':60000,'contextWindow':200000,'percent':30.0};tokens={'input':50000,'output':10000,'cacheRead':40000,'cacheWrite':5000};cost=0.45
  elif stage==1:ctx={'tokens':80000,'contextWindow':200000,'percent':40.0};tokens={'input':65000,'output':12000,'cacheRead':41000,'cacheWrite':5000};cost=0.55
  else:ctx={'tokens':None,'contextWindow':200000,'percent':None};tokens={'input':65000,'output':12000,'cacheRead':41000,'cacheWrite':5000};cost=0.55
  reply({'tokens':tokens,'cost':cost,'contextUsage':ctx})
 elif kind=='get_available_thinking_levels':reply({'levels':['off','high']})
 elif kind=='set_thinking_level':
  thinking=c['level'];reply()
 elif kind=='prompt':
  reply();send({'type':'tool_execution_end','toolCallId':'goal','toolName':'goal','isError':False,'result':{'details':{'hibiscusGoal':{'completed':2,'total':4}}}})
  send({'type':'message_update','assistantMessageEvent':{'type':'thinking_start'}})
  time.sleep(.25);send({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'CONTEXT_REPLY'}})
  stage=1;send({'type':'agent_settled'})
 elif kind=='compact':
  send({'type':'compaction_start','reason':'manual'})
  stage=2
  result={'summary':'private fixture summary','tokensBefore':80000,'estimatedTokensAfter':16000}
  send({'type':'compaction_end','reason':'manual','result':result,'aborted':False})
  reply(result)
 else:sys.exit(13)
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env(
            "CONTEXT_STATS",
            if unsupported {
                "unsupported"
            } else {
                "supported"
            },
        )
        .env("CONTEXT_LOG", root.join("commands"))
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Thinking high");
    if unsupported {
        tty.send(b"/thinking off\r");
        tty.wait_text("Thinking level: off (Pi).");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        assert!(!shown.contains("Ctx 60K/200K"));
        let commands = fs::read_to_string(root.join("commands")).unwrap();
        assert_eq!(
            commands
                .lines()
                .filter(|line| line.contains("\"type\": \"get_session_stats\""))
                .count(),
            1
        );
        drop(tty);
        fs::remove_dir_all(root).unwrap();
        return;
    }
    tty.wait_text("Ctx 60K/200K · 30%");
    tty.resize(130, 24);
    tty.wait_text("\x1b[24;1H");
    tty.send(b"first\r");
    tty.wait_text("Goal");
    tty.wait_text("CONTEXT_REPLY");
    tty.wait_text("Ctx 80K/200K · 40%");
    tty.send(b"/thinking off\r");
    tty.wait_text("Thinking level: off (Pi).");
    tty.wait_text("Thinking off");
    tty.send(b"/compact\r");
    tty.wait_text("Pi compacted context: 80K → ~16K tokens");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let frames = support::screen::snapshots(&shown);
    let goal = frames
        .iter()
        .find(|frame| {
            frame
                .lines()
                .nth(22)
                .is_some_and(|row| row.contains("Goal") && row.contains("Ctx 60K/200K"))
        })
        .unwrap();
    let footer = goal.lines().nth(22).unwrap();
    assert!(footer.find("Ctx 60K/200K").unwrap() < footer.find("Goal").unwrap());
    assert!(footer.contains("In 50K · Out 10K"));
    assert!(frames.iter().any(|frame| frame
        .lines()
        .nth(22)
        .is_some_and(|row| row.contains("Cache ~40K")
            && row.contains("Cost ~$0.45")
            && !row.contains("write 5K"))));
    let final_view = frames.last().unwrap();
    assert!(final_view.contains("Thinking off"));
    assert!(
        !final_view.contains("Ctx 80K/200K"),
        "Pi null context after compaction must not leave stale percentage"
    );
    assert!(final_view.contains("In 65K · Out 12K"));
    let commands = fs::read_to_string(root.join("commands")).unwrap();
    let commands: Vec<serde_json::Value> = commands
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let prompt = commands.iter().position(|c| c["type"] == "prompt").unwrap();
    assert_eq!(
        commands[prompt + 1]["type"],
        "get_state",
        "no mid-run stats request; refresh only after settlement"
    );
    assert!(!shown.contains("private fixture summary"));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
