use std::fs;
mod support;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use support::Pty;

#[test]
fn model_picker_marks_current_scrolls_and_cancels_without_switching() {
    let root = std::env::temp_dir().join(format!(
        "hibiscus-models-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let script = root.join("pi");
    let current = root.join("current");
    let log = root.join("switches");
    fs::write(&current, "m2\n").unwrap();
    fs::write(&script, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_session_stats"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"get_state"'*)
   model=$(tr -d '\n' < "$HIBISCUS_TEST_CURRENT")
   printf '{"type":"response","id":"%s","success":true,"data":{"model":{"provider":"demo","id":"%s"},"sessionName":"chat"}}\n' "$id" "$model" ;;
  *'"type":"get_available_models"'*)
   printf '{"type":"response","id":"%s","success":true,"data":{"models":[' "$id"
   for n in 1 2 3 4 5 6 7 8; do
    [ "$n" = 1 ] || printf ','
    printf '{"provider":"demo","id":"m%s"}' "$n"
   done
   printf ']}}\n' ;;
  *'"type":"set_model"'*)
   model=$(printf '%s\n' "$line" | sed -n 's/.*"modelId":"\([^"]*\)".*/\1/p')
   printf '%s\n' "$model" >> "$HIBISCUS_TEST_LOG"
   printf '%s\n' "$model" > "$HIBISCUS_TEST_CURRENT"
   printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &script)
        .env("HIBISCUS_TEST_CURRENT", &current)
        .env("HIBISCUS_TEST_LOG", &log)
        .env("NO_COLOR", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"/m");
    tty.wait_text("Choose a model");
    tty.send(b"\r");
    tty.wait_text("Models · all providers");
    tty.send(b"\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B");
    tty.wait_text("7/8");
    tty.send(b"\r");
    tty.wait_text("Enter send");
    tty.send(b"/models\r");
    tty.wait_text("Models · all providers");
    tty.send(b"\r"); // Current model is a silent no-op.
    tty.wait_text("Enter send");
    tty.send(b"/models\r");
    tty.wait_text("Models · all providers");
    tty.send(b"nonexistent\r");
    tty.wait_text("No matching models");
    tty.send(b"\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x1b[200~DEMO M4\x1b[201~");
    tty.wait_text("> DEMO M4");
    tty.send(b"\r");
    tty.wait_text("Enter send");
    tty.send(b"/models\r");
    tty.wait_text("Models · all providers");
    tty.send(b"m8");
    tty.wait_text("1/1");
    tty.send(b"\x1b/quit\r");
    let display = tty.finish();
    assert_eq!(fs::read_to_string(&log).unwrap(), "m7\nm4\n");
    assert!(display.contains("H  ╭─ ✿ Commands"), "{display}");
    assert!(display.contains("Choose a model"), "{display}");
    assert!(
        display.contains("H  ╭─ ✿ Models · all providers"),
        "{display}"
    );
    assert!(display.contains("demo/m7"), "{display}");
    assert!(display.contains("● current"), "{display}");
    assert!(display.contains("No matching models"), "{display}");
    assert!(display.contains("> DEMO M4"), "{display}");
    assert!(display.contains("7/8"), "{display}");
    assert!(display.contains("█"), "{display}");
    assert!(display.contains("demo/m7"), "{display}");
    assert!(!display.contains("Already using demo/m7."), "{display}");
    assert!(!display.contains("Selected demo/"), "{display}");
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
