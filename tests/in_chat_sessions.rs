use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

fn fixture_root(parent: &Path, timestamp: u128) -> PathBuf {
    // macOS clocks can return the same timestamp in parallel test threads.
    // A unique root also ensures one fixture's Drop cannot delete another's files.
    parent.join(format!(
        "hibiscus-switch-test-{}-{timestamp}-{}",
        std::process::id(),
        NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

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
        Self::in_dir(&std::env::temp_dir())
    }

    fn in_dir(parent: &Path) -> Self {
        let root = fixture_root(
            parent,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        );
        fs::create_dir_all(&root).unwrap();
        // temp_dir may contain aliases (macOS /var vs /private/var). Match the
        // physical cwd returned by getcwd in the Hibiscus subprocess.
        let root = root.canonicalize().unwrap();
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
fn fixtures_with_the_same_clock_tick_have_distinct_roots() {
    let parent = std::env::temp_dir();
    let first = fixture_root(&parent, 123);
    let second = fixture_root(&parent, 123);
    assert_ne!(first, second);
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    fs::remove_dir(&first).unwrap();
    assert!(second.is_dir());
    fs::remove_dir(&second).unwrap();
}

#[test]
fn sessions_work_when_fixture_parent_is_a_symlink() {
    let parent = Fixture::new();
    let alias = parent.root.join("alias");
    std::os::unix::fs::symlink(&parent.cwd, &alias).unwrap();
    let fixture = Fixture::in_dir(&alias);
    assert!(fixture.root.starts_with(&parent.cwd));
    let output = fixture.run("/continue\n/sessions\n2\n/quit\n", &fixture.older);
    assert!(
        output.contains("you> latest\nassistant> latest answer\n"),
        "{output}"
    );
    assert!(output.contains("Sessions for this directory"), "{output}");
    assert!(
        output.contains("you> older\nassistant> older answer\n"),
        "{output}"
    );
    let switched = fs::read_to_string(&fixture.log).unwrap();
    assert_eq!(
        switched.lines().collect::<Vec<_>>(),
        vec![
            fixture.latest.to_str().unwrap(),
            fixture.older.to_str().unwrap()
        ]
    );
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
