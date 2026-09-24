use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
    cwd: PathBuf,
    pi: PathBuf,
    older: PathBuf,
    latest: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hibiscus-switch-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cwd = root.join("project");
        let dir = root.join("sessions");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let older = dir.join("older.jsonl");
        let latest = dir.join("latest.jsonl");
        for (path, title) in [(&older, "older"), (&latest, "latest")] {
            fs::write(path, format!("{{\"type\":\"session\",\"id\":\"{title}\",\"cwd\":\"{}\"}}\n{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"{title}\"}}}}\n", cwd.display())).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let pi = root.join("mock-pi");
        fs::write(&pi, r#"#!/bin/sh
current="$HIBISCUS_TEST_CURRENT"
while IFS= read -r line; do
    id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
    case "$line" in
        *'"type":"get_state"'*)
            printf '{"type":"response","id":"%s","success":true,"data":{"sessionFile":"%s"}}\n' "$id" "$current"
            ;;
        *'"type":"switch_session"'*)
            case "$line" in
                *'older.jsonl'*) current="$HIBISCUS_TEST_OLDER" ;;
                *'latest.jsonl'*) current="$HIBISCUS_TEST_LATEST" ;;
                *) exit 14 ;;
            esac
            printf '%s\n' "$current" >> "$HIBISCUS_TEST_LOG"
            printf '{"type":"response","id":"%s","success":true,"data":{"cancelled":false}}\n' "$id"
            ;;
        *'"type":"get_messages"'*)
            case "$current" in
                *'older.jsonl') title=older ;;
                *'latest.jsonl') title=latest ;;
                *) title=empty ;;
            esac
            printf '{"type":"response","id":"%s","success":true,"data":{"messages":[{"role":"user","content":"%s"},{"role":"assistant","content":[{"type":"text","text":"%s answer"}]}]}}\n' "$id" "$title" "$title"
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
        fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
        let log = root.join("switch-log");
        Self {
            root,
            cwd,
            pi,
            older,
            latest,
            log,
        }
    }

    fn run(&self, input: &str, current: &Path) -> String {
        let mut child = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
            .current_dir(&self.cwd)
            .env("HIBISCUS_PI", &self.pi)
            .env("PI_CODING_AGENT_SESSION_DIR", self.root.join("sessions"))
            .env("HIBISCUS_TEST_OLDER", &self.older)
            .env("HIBISCUS_TEST_LATEST", &self.latest)
            .env("HIBISCUS_TEST_CURRENT", current)
            .env("HIBISCUS_TEST_LOG", &self.log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn continue_switches_to_latest_in_same_process() {
    let fixture = Fixture::new();
    let output = fixture.run("/continue\nhello\n/quit\n", &fixture.older);
    assert_eq!(output, "you> latest\nassistant> latest answer\n\nreply\n");
    assert_eq!(
        fs::read_to_string(&fixture.log).unwrap().trim(),
        fixture.latest.to_str().unwrap()
    );
}

#[test]
fn sessions_selects_previous_and_cancel_does_not_switch() {
    let fixture = Fixture::new();
    let output = fixture.run("/sessions\n\n/sessions\n2\n/quit\n", &fixture.latest);
    assert!(output.contains("Sessions for this directory"));
    assert!(output.contains("you> older\nassistant> older answer\n"));
    assert_eq!(
        fs::read_to_string(&fixture.log).unwrap().trim(),
        fixture.older.to_str().unwrap()
    );
}

#[test]
fn continue_already_active_only_shows_history() {
    let fixture = Fixture::new();
    let output = fixture.run("/continue\n/quit\n", &fixture.latest);
    assert!(output.contains("you> latest\nassistant> latest answer\n"));
    assert!(!fixture.log.exists());
}
