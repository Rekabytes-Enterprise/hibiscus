mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn narrow_layout_keeps_header_goal_and_composer_readable_after_resize() {
    let root = unique_temp_dir("hibiscus-mobile");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/usr/bin/env python3
import json,sys
for line in sys.stdin:
  command=json.loads(line); kind=command['type']; ident=command.get('id')
  if kind=='get_state': data={'sessionName':'01a0e1abcdef','thinkingLevel':'medium','model':{'provider':'openai-codex','id':'gpt-6-sol'}}
  elif kind=='get_session_stats': data={'contextUsage':{'tokens':3900,'contextWindow':272000,'percent':1.4},'tokens':{'input':19000,'output':9000},'cost':0.62}
  elif kind=='prompt':
    print(json.dumps({'type':'response','id':ident,'success':True}),flush=True)
    print(json.dumps({'type':'tool_execution_end','toolCallId':'g1','toolName':'goal','isError':False,'result':{'details':{'hibiscusGoal':{'completed':2,'total':5}}}}),flush=True)
    print(json.dumps({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'ASSISTANT MOBILE RESPONSE WITH WRAPPED WORDS'}}),flush=True)
    print(json.dumps({'type':'agent_settled'}),flush=True)
    continue
  else: sys.exit(13)
  print(json.dumps({'type':'response','id':ident,'success':True,'data':data}),flush=True)
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut tty = Pty::spawn(
        Command::new(env!("CARGO_BIN_EXE_hibiscus"))
            .env("HIBISCUS_PI", &pi)
            .env("HIBISCUS_NO_UPDATE_CHECK", "1"),
    );
    tty.wait_text("gpt-6-sol");
    tty.resize(48, 24);
    tty.wait_text("hibiscus ·");
    tty.send(b"mobile question\r");
    tty.wait_text("ASSISTANT MOBILE RESPONSE");
    tty.wait_text("Enter send");
    tty.resize(80, 24);
    tty.wait_text("openai-codex/gpt-6-sol");
    tty.send(b"/quit\r");
    let output = tty.finish();
    let snapshots = support::screen::snapshots(&output);
    let mobile = snapshots
        .iter()
        .find(|view| {
            view.lines()
                .next()
                .is_some_and(|line| line.contains("hibiscus ·"))
        })
        .expect("compact header after resize");
    assert!(
        mobile.lines().next().unwrap().starts_with(" ✿ hibiscus ·"),
        "{mobile}"
    );
    assert!(
        mobile
            .lines()
            .nth(1)
            .unwrap()
            .contains("gpt-6-sol · medium"),
        "{mobile}"
    );
    assert!(
        !mobile.lines().nth(1).unwrap().contains("openai-codex/"),
        "{mobile}"
    );
    assert!(
        snapshots.iter().any(|view| view
            .lines()
            .any(|line| line.contains("Goal 2/5 · 40%") && line.contains('✿'))),
        "Goal must share the status row"
    );
    let reply = snapshots
        .iter()
        .rev()
        .find(|view| {
            view.contains("ASSISTANT MOBILE RESPONSE")
                && view
                    .lines()
                    .next()
                    .is_some_and(|line| line.contains("hibiscus ·"))
        })
        .expect("assistant reply");
    assert!(
        reply.contains(" hibi"),
        "retain one speaker label per turn: {reply}"
    );
    assert!(
        reply.contains(" ┃ mobile question"),
        "user text uses narrow margin: {reply}"
    );
    assert!(reply.contains(" ╭"), "composer uses narrow margin: {reply}");
    assert!(
        snapshots.iter().any(|view| view
            .lines()
            .any(|line| line.contains("Enter send · /help · Ctx 3.9K/272K · 1%"))),
        "idle stats must stay complete"
    );
    assert!(
        snapshots.iter().any(|view| view
            .lines()
            .nth(1)
            .is_some_and(|line| line.contains("openai-codex/gpt-6-sol"))),
        "desktop header must return after resizing wider"
    );
    fs::remove_dir_all(root).unwrap();
}
