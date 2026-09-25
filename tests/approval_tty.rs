mod support;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};
use support::{unique_temp_dir, Pty};

fn fixture() -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = unique_temp_dir("hibiscus-approval-dialog");
    let script = dir.join("pi");
    fs::write(&script, r#"#!/bin/sh
printf '%s\n' "$*" >> "$APPROVAL_ROOT/launches"
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"model":{"provider":"test","id":"demo"}}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '{"type":"extension_ui_request","id":"approval-1","method":"select","title":"Approval needed: recursive deletion; Directory: /workspace; Command: rm -rf build","options":["Deny","Allow","Always Allow"],"timeout":60000}\n' ;;
  *'"type":"extension_ui_response"'*)
   printf '%s\n' "$line" > "$APPROVAL_ROOT/reply"
   case "$line" in *'"value":"Allow"'*|*'"value":"Always Allow"'*) printf allowed > "$APPROVAL_ROOT/ran";; esac
   printf '{"type":"agent_settled"}\n' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    (dir, script)
}

#[test]
fn approval_choices_are_correlated_and_default_to_deny() {
    for (choice, expected) in [
        (b"\r".as_slice(), Some("Deny")),
        (b"\x1b[B\x1b[A\r", Some("Deny")),
        (b"\x1b[B\r", Some("Allow")),
        (b"\x1b[B\x1b[B\r", Some("Always Allow")),
        (b"\x1b", None),
    ] {
        let (root, pi) = fixture();
        let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
        command
            .env("HIBISCUS_PI", &pi)
            .env("APPROVAL_ROOT", &root)
            .env("HIBISCUS_NO_UPDATE_CHECK", "1");
        let mut tty = Pty::spawn(&mut command);
        tty.wait_text("Enter send");
        tty.send(b"trigger\r");
        tty.wait_text("Always Allow");
        tty.send(choice);
        tty.wait_text("Enter send");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        let reply: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("reply")).unwrap()).unwrap();
        assert_eq!(reply["id"], "approval-1");
        match expected {
            Some(option) => assert_eq!(reply["value"], option),
            None => assert_eq!(reply["cancelled"], true),
        }
        assert_eq!(
            root.join("ran").exists(),
            matches!(expected, Some("Allow" | "Always Allow"))
        );
        assert!(
            shown.contains("Deny") && shown.contains("Allow") && shown.contains("Always Allow")
        );
        assert!(shown.contains("╭─ ✿ Approval needed:"));
        assert!(!shown.contains("Choose a number"));
        assert!(!shown.contains("[pi] Approval"));
        let launches = fs::read_to_string(root.join("launches")).unwrap();
        let extension = launches
            .lines()
            .next()
            .unwrap()
            .split(" --extension ")
            .nth(1)
            .unwrap();
        assert!(extension.ends_with("/approval.mjs"));
        assert!(
            !std::path::Path::new(extension).exists(),
            "temporary extension must be cleaned up"
        );
        drop(tty);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn piped_prompts_cannot_approve_tools() {
    let (root, pi) = fixture();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
        .env("HIBISCUS_PI", &pi)
        .env("APPROVAL_ROOT", &root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"trigger\n/quit\n")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    let reply: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("reply")).unwrap()).unwrap();
    assert_eq!(reply["cancelled"], true);
    assert!(!root.join("ran").exists());
    fs::remove_dir_all(root).unwrap();
}
