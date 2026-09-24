use std::fs;
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn login_handoff_does_not_steal_pi_keys_and_returns_to_chat() {
    run(true, false);
}

#[test]
fn login_handoff_without_saved_chat_uses_an_ephemeral_pi_session() {
    run(false, false);
}

#[test]
fn other_provider_choice_uses_pi_tui_even_when_codex_sdk_is_available() {
    run(false, true);
}

fn run(saved: bool, other_provider: bool) {
    let root = std::env::temp_dir().join(format!(
        "hibiscus-auth-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let session = root.join("session.jsonl");
    if saved {
        fs::write(&session, "{\"type\":\"session\",\"id\":\"test\"}\n").unwrap();
    }
    let log = root.join("pi-keys");
    let args = root.join("pi-args");
    let sdk = root.join("sdk.mjs");
    let sdk_marker = root.join("sdk-loaded");
    fs::write(&sdk, "import { writeFileSync } from 'node:fs'; writeFileSync(process.env.HIBISCUS_TEST_SDK_MARKER, 'loaded'); throw new Error('should not load');").unwrap();
    let script = root.join("pi");
    fs::write(&script, r#"#!/bin/sh
if [ "$1" = "--mode" ]; then
 while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
  case "$line" in
   *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"sessionFile":"%s"}}\n' "$id" "$HIBISCUS_TEST_SESSION" ;;
   *'"type":"prompt"'*) printf '{"type":"response","id":"%s","success":true}\n{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"ok"}}\n{"type":"agent_settled"}\n' "$id" ;;
   *) exit 13 ;;
  esac
 done
else
 printf '%s' "$*" > "$HIBISCUS_TEST_ARGS"
 stty -echo
 IFS= read -r value
 stty echo
 printf '%s' "$value" > "$HIBISCUS_TEST_LOG"
fi
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let mut master = 0;
    let mut slave = 0;
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    let stdin = unsafe { std::fs::File::from_raw_fd(slave) };
    let stdout = stdin.try_clone().unwrap();
    let stderr = stdin.try_clone().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
        .env("HIBISCUS_PI", &script)
        .env("HIBISCUS_TEST_SESSION", &session)
        .env("HIBISCUS_TEST_LOG", &log)
        .env("HIBISCUS_TEST_ARGS", &args)
        .env("HIBISCUS_TEST_SDK_MARKER", &sdk_marker)
        .env(
            "HIBISCUS_AUTH_SDK",
            if other_provider {
                sdk.as_os_str()
            } else {
                std::ffi::OsStr::new("")
            },
        )
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .pre_exec_set_tty()
        .spawn()
        .unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let reader_output = Arc::clone(&captured);
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            let count = unsafe { libc::read(master, buf.as_mut_ptr().cast(), buf.len()) };
            if count <= 0 {
                break;
            }
            reader_output
                .lock()
                .unwrap()
                .extend_from_slice(&buf[..count as usize]);
        }
    });
    wait_for_text(&captured, "hibiscus");
    if saved {
        send(master, b"hello\r");
        wait_for_text(&captured, "ok");
    }
    send(master, b"/login\r");
    wait_for_text(&captured, "Sign in to");
    if other_provider {
        send(master, b"\x1b[B\r");
    } else {
        send(master, b"\r");
    } // Codex choice; SDK unavailable in this mock
    wait_for_text(&captured, "Hand off to Pi for /login?");
    send(master, b"y\r");
    wait_for_path(&args);
    send(master, b"pi-only-input\r");
    wait_for_path(&log);
    wait_for_text(&captured, "Returned from Pi.");
    send(master, b"after\r");
    wait_for_text(&captured, "after");
    send(master, b"/quit\r");
    let status = child.wait().unwrap();
    reader.join().unwrap();
    let display = String::from_utf8_lossy(&captured.lock().unwrap()).into_owned();
    assert!(status.success(), "{status}: {display}");
    assert_eq!(fs::read_to_string(log).unwrap(), "pi-only-input");
    assert!(
        !sdk_marker.exists(),
        "SDK must not run for an explicitly chosen other provider"
    );
    let handed_off = fs::read_to_string(args).unwrap();
    if saved {
        assert_eq!(handed_off, format!("--session {}", session.display()));
    } else {
        assert_eq!(handed_off, "--no-session");
    }
    unsafe {
        libc::close(master);
    }
    fs::remove_dir_all(root).unwrap();
}

fn wait_for_text(output: &Arc<Mutex<Vec<u8>>>, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if String::from_utf8_lossy(&output.lock().unwrap()).contains(expected) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let shown = String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
    panic!("timed out waiting for {expected:?}: {shown}");
}

fn wait_for_path(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {}", path.display());
}

fn send(fd: i32, bytes: &[u8]) {
    assert_eq!(
        unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) },
        bytes.len() as isize
    );
}
trait ControllingTty {
    fn pre_exec_set_tty(&mut self) -> &mut Self;
}
impl ControllingTty for Command {
    fn pre_exec_set_tty(&mut self) -> &mut Self {
        use std::os::unix::process::CommandExt;
        unsafe {
            self.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            })
        }
    }
}
