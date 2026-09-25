mod support;
use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

fn run(event: Value, keys: &[u8], expected: Value, keep_draft: bool) -> String {
    let root = unique_temp_dir("hibiscus-modal");
    fs::write(root.join("event"), format!("{event}\n")).unwrap();
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"BEFORE_MODAL"}}'
   if [ "$KEEP_DRAFT" = yes ]; then sleep 0.4; fi
   cat "$MODAL_ROOT/event" ;;
  *'"type":"extension_ui_response"'*)
   printf '%s\n' "$line" > "$MODAL_ROOT/response"
   printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"AFTER_MODAL"}}' '{"type":"agent_settled"}' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("MODAL_ROOT", &root)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1")
        .env("KEEP_DRAFT", if keep_draft { "yes" } else { "no" });
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"start\r");
    if keep_draft {
        tty.wait_text("BEFORE_MODAL");
        tty.send(b"keep this draft");
        tty.wait_text("keep this draft");
    }
    let heading = format!(
        "╭─ ✿ {}",
        event["title"].as_str().unwrap().lines().next().unwrap()
    );
    tty.wait_text(&heading);
    if !keys.is_empty() {
        tty.send(keys);
    }
    tty.wait_text("AFTER_MODAL");
    tty.wait_text("Enter send");
    if keep_draft {
        tty.wait_text("keep this draft");
        tty.send(b"\x03");
    }
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let response: Value =
        serde_json::from_str(&fs::read_to_string(root.join("response")).unwrap()).unwrap();
    assert_eq!(response["id"], "fixture-dialog");
    for (field, value) in expected.as_object().unwrap() {
        assert_eq!(&response[field], value);
    }
    assert!(!shown.contains("Choose a number"));
    assert!(!shown.contains("[pi] Modal fixture"));
    let views = support::screen::snapshots(&shown);
    let modal = views.iter().find(|view| view.contains(&heading)).unwrap();
    assert!(modal.contains("Respond in the dialog above"));
    assert!(
        modal.contains("  ╭─ ✿"),
        "panel must align with the composer margin"
    );
    drop(tty);
    fs::remove_dir_all(root).unwrap();
    shown
}

fn event(method: &str) -> Value {
    json!({"type":"extension_ui_request","id":"fixture-dialog","method":method,"title":"Modal fixture"})
}

#[test]
fn select_confirm_input_and_editor_share_docked_modal_chrome() {
    let mut select = event("select");
    select["options"] = json!((0..10).map(|n| format!("Option {n}")).collect::<Vec<_>>());
    run(select, b"\x1b[6~\r", json!({"value":"Option 5"}), false);
    let mut confirm = event("confirm");
    confirm["message"] = json!("Please confirm this action");
    run(confirm, b"\x1b[B\r", json!({"confirmed":true}), false);
    let mut input = event("input");
    input["placeholder"] = json!("Your answer");
    run(input, "héllo\r".as_bytes(), json!({"value":"héllo"}), true);
    let mut editor = event("editor");
    editor["prefill"] = json!("existing text");
    run(editor, b"!\r", json!({"value":"existing text!"}), false);
}

#[test]
fn long_approval_details_are_reviewed_in_panel_before_allowing() {
    let mut approval = event("select");
    approval["title"] = json!(format!(
        "Approval needed: review\nCommand: {}\nLAST_DETAIL",
        "path-to-review ".repeat(100)
    ));
    approval["options"] = json!(["Deny", "Allow", "Always Allow"]);
    let shown = run(
        approval,
        b"\x1b[B\r\x1b[6~\x1b[6~\x1b[6~\r",
        json!({"value":"Allow"}),
        false,
    );
    assert!(shown.contains("review remaining details before Allow"));
    assert!(shown.contains("LAST_DETAIL"));
}

#[test]
fn modal_timeout_and_escape_cancel_without_default_approval() {
    let mut timed = event("select");
    timed["options"] = json!(["Deny", "Allow", "Always Allow"]);
    timed["timeout"] = json!(250);
    run(timed, b"", json!({"cancelled":true}), false);
    run(event("confirm"), b"\x1b", json!({"cancelled":true}), true);
    run(event("confirm"), b"\r", json!({"confirmed":false}), false);
}
