mod support;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};
use support::{unique_temp_dir, Pty};

fn run(scenario: &str) {
    let root = unique_temp_dir("hibiscus-pi-commands");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/usr/bin/env python3
import json,os,sys,threading,time
root=os.environ['COMMAND_ROOT'];scenario=os.environ['COMMAND_SCENARIO']
lock=threading.Lock();stop=threading.Event();level='off' if scenario=='no_thinking' else 'low';compact=None

def send(data):
 with lock:print(json.dumps(data),flush=True)
def response(c,success=True,data=None,error=None):
 r={'type':'response','id':c['id'],'success':success}
 if data is not None:r['data']=data
 if error:r['error']=error
 send(r)
def work(c):
 global compact
 end=time.monotonic()+10
 while not os.path.exists(root+'/finish') and not stop.is_set():
  if time.monotonic()>end:raise RuntimeError('fixture did not release compaction')
  time.sleep(.005)
 if stop.is_set():return
 result={'summary':'PRIVATE_SUMMARY','tokensBefore':100,'estimatedTokensAfter':40}
 send({'type':'compaction_end','reason':'manual','result':result,'aborted':False})
 response(c, data=result)
 compact=None
for line in sys.stdin:
 c=json.loads(line);kind=c['type']
 with open(root+'/commands','a') as log:log.write(json.dumps(c)+'\n')
 if kind=='get_state':response(c,data={'model':None if scenario=='no_model' else {'provider':'test','id':'reasoner'},'thinkingLevel':level})
 elif kind=='get_available_thinking_levels':response(c,data={'levels':['off'] if scenario=='no_thinking' else ['off','low','high']})
 elif kind=='set_thinking_level':
  level=c['level'];send({'type':'thinking_level_changed','level':level});response(c)
 elif kind=='compact':
  if scenario=='no_model': response(c,success=False,error='No model selected')
  elif scenario in ('success','plain') and compact is None:
   compact=c;send({'type':'compaction_start','reason':'manual'})
   threading.Thread(target=work,args=(c,),daemon=True).start()
  elif scenario=='cancel':
   compact=c;send({'type':'compaction_start','reason':'manual'})
  else:response(c,success=False,error='Nothing to compact (session too small)')
 elif kind=='abort':
  stop.set();send({'type':'compaction_end','reason':'manual','aborted':True})
  response(compact,success=False,error='Compaction cancelled')
  time.sleep(.15) # compact response before correlated abort acknowledgement
  response(c)
 elif kind=='prompt':
  response(c);send({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'NEXT_REPLY'}})
  send({'type':'agent_settled'})
 else:sys.exit(13)
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("COMMAND_ROOT", &root)
        .env("COMMAND_SCENARIO", scenario)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    if scenario == "plain" {
        fs::write(root.join("finish"), "").unwrap();
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
            .write_all(b"/thinking high\n/compact\n/quit\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("Thinking level: high (Pi)."));
        assert!(text.contains("Pi compacted context: 100 → ~40 tokens."));
        assert!(!text.contains("PRIVATE_SUMMARY") && !text.contains('\x1b'));
        let commands = fs::read_to_string(root.join("commands")).unwrap();
        assert!(!commands.contains("\"type\": \"prompt\""));
        fs::remove_dir_all(root).unwrap();
        return;
    }
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    if scenario == "no_thinking" {
        tty.send(b"/thinking\r");
        tty.wait_text("Thinking · Pi model");
        tty.send(b"\r");
        tty.wait_text("Already using off thinking");
        tty.send(b"/thinking high\r");
        tty.wait_text("Unknown thinking level. Available: off");
    } else if scenario == "no_model" {
        tty.send(b"/thinking\r");
        tty.wait_text("Choose a model with /models");
        tty.send(b"/compact\r");
        tty.wait_text("No model selected");
    } else {
        tty.send(b"/thinking\r");
        tty.wait_text("Thinking · Pi model");
        tty.send(b"\x1b");
        tty.wait_text("Enter send");
        tty.send(b"/thinking bogus\r");
        tty.wait_text("Unknown thinking level. Available: off, low, high");
        tty.send(b"/thinking high\r");
        tty.wait_text("Thinking level: high (Pi).");
        tty.send(b"/compact Keep the edits\r");
        tty.wait_text("Compacting context");
        tty.send(b"draft\r");
        tty.wait_text("draft kept");
        if scenario == "cancel" {
            tty.send(b"\x1b");
            tty.wait_text("Cancelled compact");
        } else {
            fs::write(root.join("finish"), "").unwrap();
            tty.wait_text("Pi compacted context: 100 → ~40 tokens");
        }
        tty.wait_text("Enter send");
        tty.send(b"\r"); // send the preserved draft explicitly, not before
        tty.wait_text("NEXT_REPLY");
        tty.wait_text("Enter send");
    }
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let commands: Vec<serde_json::Value> = fs::read_to_string(root.join("commands"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let kinds: Vec<&str> = commands
        .iter()
        .map(|c| c["type"].as_str().unwrap())
        .collect();
    if matches!(scenario, "no_model" | "no_thinking") {
        assert!(!kinds.contains(&"set_thinking_level"));
        assert_eq!(
            kinds.contains(&"get_available_thinking_levels"),
            scenario == "no_thinking"
        );
        assert!(!kinds.contains(&"prompt"));
    } else {
        assert_eq!(
            commands
                .iter()
                .filter(|c| c["type"] == "set_thinking_level")
                .count(),
            1
        );
        assert_eq!(
            commands
                .iter()
                .find(|c| c["type"] == "set_thinking_level")
                .unwrap()["level"],
            "high"
        );
        assert_eq!(
            commands.iter().find(|c| c["type"] == "compact").unwrap()["customInstructions"],
            "Keep the edits"
        );
        assert_eq!(
            commands.iter().filter(|c| c["type"] == "prompt").count(),
            1,
            "draft must not auto-submit"
        );
        assert_eq!(
            commands.iter().find(|c| c["type"] == "prompt").unwrap()["message"],
            "draft"
        );
        assert!(!shown.contains("PRIVATE_SUMMARY"));
        assert_eq!(kinds.contains(&"abort"), scenario == "cancel");
    }
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn thinking_picker_direct_level_and_compaction_keep_draft() {
    run("success");
}
#[test]
fn compact_escape_waits_for_abort_ack_and_keeps_draft() {
    run("cancel");
}
#[test]
fn thinking_requires_a_model_and_failed_compact_keeps_chat() {
    run("no_model");
}
#[test]
fn model_without_reasoning_exposes_only_off() {
    run("no_thinking");
}
#[test]
fn piped_commands_use_pi_rpc_without_sending_slash_prompts() {
    run("plain");
}
