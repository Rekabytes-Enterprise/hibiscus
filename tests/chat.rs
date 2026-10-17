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
for arg in "$@"; do
 case "$arg" in
  */writing-style.mjs) [ -s "$arg" ] || exit 14 ;;
 esac
done
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
    let launch = fs::read_to_string(log).unwrap();
    assert!(launch
        .trim()
        .starts_with("--mode rpc --no-extensions --tools read,bash,edit,write,goal --extension "));
    assert!(launch.trim().ends_with("/writing-style.mjs --continue"));
    let extensions: Vec<_> = launch.trim().split(" --extension ").skip(1).collect();
    assert_eq!(extensions.len(), 3);
    assert!(extensions[1].ends_with("/model-context.mjs"));
    assert!(extensions[0].ends_with("/approval.mjs"));
    let writing = extensions[2].strip_suffix(" --continue").unwrap();
    assert!(
        !std::path::Path::new(writing).exists(),
        "writing extension must be cleaned up"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn approval_commands_are_local_and_persist_across_restarts() {
    let directory = std::env::temp_dir().join(format!(
        "hibiscus-approval-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let script = directory.join("pi");
    // Any RPC input is a failure: these commands must be handled locally.
    fs::write(
        &script,
        "#!/bin/sh\nwhile IFS= read -r line; do exit 13; done\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let run = |input: &[u8]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
            .env("HIBISCUS_PI", &script)
            .env("XDG_CONFIG_HOME", directory.join("config"))
            .env("HIBISCUS_HOME", directory.join("hibiscus"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    };
    let output = run(b"/approval\n/approval off\n/approval\n/approval invalid\n/approval\n/approval enable\n/approval on\n/approval disable\n/quit\n");
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), 8, "{output}");
    for index in [0, 5, 6] {
        assert!(lines[index].starts_with("Approvals enabled:"));
    }
    for index in [1, 2, 4, 7] {
        assert!(lines[index].starts_with("Approvals disabled:"));
    }
    assert!(lines[3].starts_with("Usage: /approval"));
    assert!(run(b"/approval\n/quit\n").starts_with("Approvals disabled:"));
    assert!(run(b"/approval on\n/quit\n").starts_with("Approvals enabled:"));
    assert!(run(b"/approval\n/quit\n").starts_with("Approvals enabled:"));
    let config = directory.join("hibiscus/approval.json");
    assert_eq!(
        fs::read_to_string(&config).unwrap().trim(),
        "{\"enabled\":true}"
    );
    let legacy = directory.join("config/hibiscus/approval.json");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, "{\"enabled\":false}").unwrap();
    assert!(
        run(b"/approval\n/quit\n").starts_with("Approvals enabled:"),
        "new settings take precedence"
    );
    fs::remove_file(&config).unwrap();
    assert!(
        run(b"/approval\n/quit\n").starts_with("Approvals disabled:"),
        "legacy preference migrates"
    );
    assert_eq!(
        fs::read_to_string(&config).unwrap().trim(),
        "{\"enabled\":false}"
    );
    assert!(legacy.exists(), "migration preserves the original file");
    fs::remove_file(&config).unwrap();
    fs::create_dir(&config).unwrap();
    // Startup rejects unreadable settings instead of silently bypassing approvals.
    let result = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
        .env("HIBISCUS_PI", &script)
        .env("XDG_CONFIG_HOME", directory.join("config"))
        .env("HIBISCUS_HOME", directory.join("hibiscus"))
        .arg("test")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr)
        .contains("could not load Hibiscus approval setting"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_context_config_fails_before_launching_pi() {
    let directory =
        std::env::temp_dir().join(format!("hibiscus-context-config-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("config.json"),
        r#"{"codexContextWindow":2000000}"#,
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
        .env("HIBISCUS_HOME", &directory)
        .env("HIBISCUS_PI", directory.join("must-not-launch"))
        .arg("test")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("could not load Hibiscus config.json"));
    fs::remove_dir_all(directory).unwrap();
}
