use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn chat_reuses_rpc_process_and_new_session() {
    let directory = std::env::temp_dir().join(format!(
        "hibiscus-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let script = directory.join("pi");
    fs::write(&script, r#"#!/bin/sh
[ "$1" = "--mode" ] && [ "$2" = "rpc" ] || exit 12
printf '%s\n' "$*" > "$HIBISCUS_TEST_LOG"
while IFS= read -r line; do
    id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
    case "$line" in
        *'"type":"get_messages"'*)
            printf '{"type":"response","id":"%s","success":true,"data":{"messages":[{"role":"user","content":"previous"},{"role":"assistant","content":[{"type":"text","text":"past reply"}]}]}}\n' "$id"
            ;;
        *'"type":"new_session"'*)
            printf '{"type":"response","id":"%s","success":true,"data":{"cancelled":false}}\n' "$id"
            ;;
        *'"type":"prompt"'*)
            printf '{"type":"response","id":"%s","success":true}\n' "$id"
            printf '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"reply"}}\n'
            printf '{"type":"agent_settled"}\n'
            ;;
        *) exit 13 ;;
    esac
done
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let log = directory.join("args");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
        .env("HIBISCUS_PI", &script)
        .env("HIBISCUS_TEST_LOG", &log)
        .arg("--continue")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"one\n/new\ntwo\n/quit\n")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        "you> previous\nassistant> past reply\n\nreply\nStarted new session.\nreply\n"
    );
    assert_eq!(
        fs::read_to_string(log).unwrap().trim(),
        "--mode rpc --no-extensions --tools read,bash,edit,write --continue"
    );
    fs::remove_dir_all(directory).unwrap();
}
