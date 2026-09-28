mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{screen, unique_temp_dir, Pty};

#[test]
fn mermaid_source_changes_to_diagram_only_after_settlement() {
    let root = unique_temp_dir("hibiscus-mermaid-tty");
    let pi = root.join("pi");
    let sdk = root.join("sdk.js");
    let package = root.join("node_modules/grok-mermaid");
    fs::create_dir_all(&package).unwrap();
    fs::write(&sdk, "").unwrap();
    fs::write(
        package.join("package.json"),
        r#"{"type":"module","exports":"./index.js"}"#,
    )
    .unwrap();
    fs::write(package.join("index.js"), "export function render() { return { warnings: [], width: 16, plain: ['╭──────────────╮', '│ DIAGRAM_DONE │', '╰──────────────╯'] }; }\n").unwrap();
    let events = root.join("events");
    fs::write(&events, serde_json::json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"Before\n```mermaid\nflowchart LR\n  A[Start] --> B[Done]\n```\nAfter"}}).to_string()).unwrap();
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_session_stats"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '%s\n' "$(cat "$MERMAID_EVENTS")"
   while [ ! -f "$MERMAID_RELEASE" ]; do sleep 0.02; done
   printf '%s\n' '{"type":"agent_settled"}' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_AUTH_SDK", &sdk)
        .env("MERMAID_EVENTS", &events)
        .env("MERMAID_RELEASE", root.join("release"))
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"diagram\r");
    tty.wait_text("A[Start]");
    fs::write(root.join("release"), "").unwrap();
    tty.wait_text("DIAGRAM_DONE");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let output = tty.finish();
    let view = screen::snapshots(&output).pop().unwrap();
    assert!(view.contains("Before"), "{view}");
    assert!(view.contains("DIAGRAM_DONE"), "{view}");
    assert!(view.contains("After"), "{view}");
    assert!(!view.contains("A[Start]"), "{view}");
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
