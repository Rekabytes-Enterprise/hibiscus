mod support;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use support::{unique_temp_dir, Pty};

#[test]
fn model_picker_groups_providers_and_switches_without_restarting_pi() {
    let root = unique_temp_dir("hibiscus-provider-models");
    let pi = root.join("pi");
    let current = root.join("current");
    let changes = root.join("changes");
    fs::write(&current, "anthropic/sonnet\n").unwrap();
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*)
   active=$(head -n 1 "$HIBISCUS_TEST_CURRENT")
   provider=${active%%/*}
   model=${active#*/}
   printf '{"type":"response","id":"%s","success":true,"data":{"model":{"provider":"%s","id":"%s"},"sessionId":"test"}}\n' "$id" "$provider" "$model" ;;
  *'"type":"get_available_models"'*)
   printf '{"type":"response","id":"%s","success":true,"data":{"models":[{"provider":"openrouter","id":"z"},{"provider":"openai-codex","id":"gpt"},{"provider":"anthropic","id":"sonnet"},{"provider":"openrouter","id":"a"},{"provider":"anthropic","id":"haiku"}]}}\n' "$id" ;;
  *'"type":"set_model"'*)
   provider=$(printf '%s\n' "$line" | sed -n 's/.*"provider":"\([^"]*\)".*/\1/p')
   model=$(printf '%s\n' "$line" | sed -n 's/.*"modelId":"\([^"]*\)".*/\1/p')
   printf '%s/%s\n' "$provider" "$model" >> "$HIBISCUS_TEST_CHANGES"
   printf '%s/%s\n' "$provider" "$model" > "$HIBISCUS_TEST_CURRENT"
   printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_TEST_CURRENT", &current)
        .env("HIBISCUS_TEST_CHANGES", &changes);
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("hibiscus");
    tty.send(b"/models\r");
    tty.wait_text("Models · all providers");
    // The active anthropic/sonnet is second after anthropic/haiku.
    tty.send(b"\x1b[B\r");
    tty.wait_text("Selected openai-codex/gpt.");
    tty.wait_text("Enter send · Ctrl+Enter");
    tty.send(b"/models\r");
    tty.wait_text("Models · all providers");
    tty.send(b"\r");
    tty.wait_text("Already using openai-codex/gpt.");
    tty.wait_text("Enter send · Ctrl+Enter");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert_eq!(fs::read_to_string(changes).unwrap(), "openai-codex/gpt\n");
    let first = shown.find("Models · all providers").unwrap();
    let order = [
        "anthropic/haiku",
        "anthropic/sonnet",
        "openai-codex/gpt",
        "openrouter/a",
        "openrouter/z",
    ];
    let mut last = first;
    for model in order {
        let pos = shown[last..]
            .find(model)
            .expect("model missing or out of order")
            + last;
        last = pos + model.len();
    }
    assert!(shown.contains("openai-codex/gpt  ·  test"), "{shown}");
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
