mod support;
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn explicit_checklist_updates_one_row_and_long_timeline_keeps_three_diffs() {
    let root = unique_temp_dir("hibiscus-goal-timeline");
    let pi = root.join("pi");
    let events = root.join("events");
    let mut lines = Vec::new();
    let steps = (1..=20)
        .map(|id| json!({"label":format!("Step {id}"),"done":id<=12}))
        .collect::<Vec<_>>();
    let goal = |done, steps: Vec<serde_json::Value>| json!({"type":"tool_execution_end","toolCallId":format!("g{done}"),"toolName":"goal","isError":false,"result":{"details":{"hibiscusGoal":{"completed":done,"total":20,"steps":steps}}}});
    lines.push(goal(12, steps.clone()));
    lines.push(json!({"type":"message_update","assistantMessageEvent":{"type":"thinking_start"}}));
    lines.push(json!({"type":"message_update","assistantMessageEvent":{"type":"thinking_end"}}));
    for id in 1..=10 {
        let path = format!("src/file_{id}.rs");
        lines.push(json!({"type":"tool_execution_start","toolCallId":format!("r{id}"),"toolName":"read","args":{"path":path}}));
        lines.push(json!({"type":"tool_execution_end","toolCallId":format!("r{id}"),"toolName":"read","isError":false}));
    }
    for id in 1..=3 {
        let path = format!("src/change_{id}.rs");
        lines.push(json!({"type":"tool_execution_start","toolCallId":format!("e{id}"),"toolName":"edit","args":{"path":path}}));
        lines.push(json!({"type":"tool_execution_end","toolCallId":format!("e{id}"),"toolName":"edit","isError":false,"result":{"details":{"diff":format!("-old_{id}\n+new_{id}")}}}));
        if id == 1 {
            lines.push(
                json!({"type":"message_update","assistantMessageEvent":{"type":"thinking_start"}}),
            );
            lines.push(
                json!({"type":"message_update","assistantMessageEvent":{"type":"thinking_end"}}),
            );
        }
    }
    lines.push(json!({"type":"tool_execution_start","toolCallId":"b1","toolName":"bash","args":{"command":"private command"}}));
    lines.push(
        json!({"type":"tool_execution_end","toolCallId":"b1","toolName":"bash","isError":false}),
    );
    let finished = (1..=20)
        .map(|id| json!({"label":format!("Step {id}"),"done":true}))
        .collect();
    lines.push(goal(20, finished));
    lines.push(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"Finished the planned work."}}));
    lines.push(json!({"type":"agent_settled"}));
    fs::write(
        &events,
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>(),
    )
    .unwrap();
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"model":{"provider":"demo","id":"small"}}}\n' "$id" ;;
  *'"type":"prompt"'*) printf '{"type":"response","id":"%s","success":true}\n' "$id"; while IFS= read -r record; do printf '%s\n' "$record"; done < "$GOAL_EVENTS" ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("GOAL_EVENTS", &events)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"a long task\r");
    tty.wait_text("20/20");
    tty.wait_text("Finished the planned work.");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let output = tty.finish();
    for expected in [
        "◐ Exploring · latest: src/file_1.rs",
        "◐ Exploring · latest: src/file_10.rs",
        "✓ Explored · latest: src/file_10.rs",
        "✓ Read · 10 files",
        "✓ Bash · 1 command",
        "diff · src/change_1.rs",
        "diff · src/change_2.rs",
        "diff · src/change_3.rs",
        "-old_1",
        "+new_2",
        "-old_3",
        "Goal  ████████████░░░░░░░░  12/20 · 60%",
        "Goal  ████████████████████  20/20 · 100%",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    assert!(!output.contains("private command"));
    assert!(
        !output.contains("latest: src/file_8.rs ·"),
        "latest row must not list multiple paths"
    );
    assert!(
        !output.contains("thinking · <1s"),
        "thinking belongs only in the live footer"
    );
    // A completed chat restores the normal hint instead of pinning the goal.
    let frames = support::screen::snapshots(&output);
    let final_frame = frames
        .iter()
        .rev()
        .find(|part| part.contains("Enter send"))
        .unwrap();
    assert!(!final_frame.contains("Goal  █"));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
