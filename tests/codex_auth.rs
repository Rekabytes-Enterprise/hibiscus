use std::fs;
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn codex_login_uses_pi_sdk_and_reconnects_same_session_without_pi_tui() {
    run("device");
}

#[test]
fn browser_callback_finishes_without_waiting_for_manual_code() {
    run("browser");
}

#[test]
fn completed_login_reconnects_without_waiting_for_browser_connection_to_close() {
    run("open_connection");
}

#[test]
fn pasted_manual_code_is_not_echoed_or_stored_in_transcript() {
    run("manual");
}

#[test]
fn codex_login_works_before_a_chat_session_has_been_saved() {
    run("empty");
}

#[test]
fn logged_out_with_another_selected_provider_can_choose_codex_without_a_session() {
    run("logged_out");
}

#[test]
fn cancelling_codex_login_does_not_reconnect_or_write_credentials() {
    run("cancel");
}

fn run(method: &str) {
    let root = std::env::temp_dir().join(format!(
        "hibiscus-auth-sdk-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let session = root.join("chat.jsonl");
    if !matches!(method, "empty" | "logged_out") {
        fs::write(&session, "{\"type\":\"session\",\"id\":\"test\"}\n").unwrap();
    }
    let pi_log = root.join("pi-args");
    let auth_log = root.join("auth-result");
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/bin/sh
printf '%s\n' "$*" >> "$HIBISCUS_TEST_PI_LOG"
[ "$1" = "--mode" ] && [ "$2" = "rpc" ] || exit 12
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"sessionFile":"%s","model":{"provider":"%s","id":"gpt-test"}}}\n' "$id" "$HIBISCUS_TEST_SESSION" "$HIBISCUS_TEST_PROVIDER" ;;
  *'"type":"prompt"'*) printf '{"type":"response","id":"%s","success":true}\n{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"ok"}}\n{"type":"agent_settled"}\n' "$id" ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let sdk = root.join("sdk.mjs");
    fs::write(&sdk, r#"import { writeFileSync } from 'node:fs';
export class ModelRuntime {
  static async create() {
    return { async login(provider, type, interaction) {
      const method = await interaction.prompt({type:'select',message:'Choose method', options:[
        {id:'browser',label:'Browser login'}, {id:'device_code',label:'Device code login'}]});
      if (method === 'browser') {
        interaction.notify({type:'auth_url',url:'https://auth.openai.com/oauth/authorize?response_type=code&client_id=TEST_CLIENT&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback&scope=openid%20profile%20email%20offline_access&state=TEST_STATE_TO_PROVE_THE_URL_IS_NOT_BROKEN_ACROSS_TERMINAL_ROWS'});
        if (process.env.HIBISCUS_TEST_AUTH_METHOD === 'manual') {
          const code = await interaction.prompt({type:'manual_code',message:'Paste redirect URL:'});
          if (code !== 'FAKE-SECRET-CODE') throw new Error('unexpected test code');
        } else {
          const abort = new AbortController();
          const manual = interaction.prompt({type:'manual_code',message:'Paste redirect URL:',signal:abort.signal}).catch(() => {});
          await new Promise(resolve => setTimeout(resolve, 150));
          abort.abort();
          await manual;
        }
      } else {
        interaction.notify({type:'device_code',verificationUri:'https://example.invalid/device',userCode:'FAKE-CODE'});
        await new Promise(resolve => setTimeout(resolve, 150));
      }
      if (process.env.HIBISCUS_TEST_AUTH_METHOD === 'open_connection') {
        const { createServer } = await import('node:http');
        const { connect } = await import('node:net');
        const server = createServer();
        const accepted = new Promise(resolve => server.once('connection', resolve));
        await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
        const browser = connect(server.address().port, '127.0.0.1');
        const socket = await accepted;
        // Like Pi's OAuth callback cleanup: stop listening, but a browser's
        // preconnected socket keeps Node alive after login has completed.
        server.close();
        setTimeout(() => {
          writeFileSync(process.env.HIBISCUS_TEST_AUTH_LOG + '.closed', 'natural shutdown');
          browser.destroy();
          socket.destroy();
        }, 4000); // bounded failure on the old implementation, not a hung test
      }
      writeFileSync(process.env.HIBISCUS_TEST_AUTH_LOG, `${provider} ${type} ${method}`);
      return {}; // test-only: never uses real credentials or the network
    }};
  }
}
"#).unwrap();

    let mut master = 0;
    let mut slave = 0;
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    let stdin = unsafe { fs::File::from_raw_fd(slave) };
    let stdout = stdin.try_clone().unwrap();
    let stderr = stdin.try_clone().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_AUTH_SDK", &sdk)
        .env("HIBISCUS_TEST_SESSION", &session)
        .env("HIBISCUS_TEST_PI_LOG", &pi_log)
        .env("HIBISCUS_TEST_AUTH_LOG", &auth_log)
        .env("HIBISCUS_TEST_AUTH_METHOD", method)
        .env(
            "HIBISCUS_TEST_PROVIDER",
            if method == "logged_out" {
                "openai"
            } else {
                "openai-codex"
            },
        )
        .env("NO_COLOR", "1")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .pre_exec_set_tty()
        .spawn()
        .unwrap();
    let reader = std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = unsafe { libc::read(master, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            };
            all.extend_from_slice(&buf[..n as usize]);
        }
        all
    });
    std::thread::sleep(Duration::from_millis(230));
    if !matches!(method, "empty" | "logged_out") {
        send(master, b"hello\r");
        std::thread::sleep(Duration::from_millis(170));
    }
    send(master, b"/login\r");
    std::thread::sleep(Duration::from_millis(300));
    send(master, b"\r"); // choose Codex provider, regardless of selected model
    std::thread::sleep(Duration::from_millis(250));
    if method == "cancel" {
        send(master, b"\x1b");
        std::thread::sleep(Duration::from_millis(170));
        send(master, b"/quit\r");
        let status = child.wait().unwrap();
        let bytes = reader.join().unwrap();
        let shown = String::from_utf8_lossy(&bytes);
        assert!(status.success(), "{status}: {shown}");
        assert!(shown.contains("Codex sign-in cancelled."), "{shown}");
        assert!(!auth_log.exists());
        assert_eq!(fs::read_to_string(pi_log).unwrap().lines().count(), 1);
        unsafe {
            libc::close(master);
        }
        fs::remove_dir_all(root).unwrap();
        return;
    }
    if method == "device" {
        send(master, b"\x1b[B");
    }
    send(master, b"\r");
    if method == "manual" {
        std::thread::sleep(Duration::from_millis(300));
        send(master, b"\x19"); // Ctrl+Y asks the terminal to copy the full URL
        send(master, b"FAKE-SECRET-CODE\r");
    }
    std::thread::sleep(Duration::from_millis(700));
    send(master, b"after\r");
    std::thread::sleep(Duration::from_millis(160));
    send(master, b"/quit\r");
    let status = child.wait().unwrap();
    let bytes = reader.join().unwrap();
    let shown = String::from_utf8_lossy(&bytes);
    assert!(status.success(), "{status}: {shown}");
    let expected = if method == "device" {
        "device_code"
    } else {
        "browser"
    };
    assert_eq!(
        fs::read_to_string(&auth_log).unwrap(),
        format!("openai-codex oauth {expected}")
    );
    assert!(
        shown.contains("Codex sign-in completed. Reconnected Pi to this chat."),
        "{shown}"
    );
    if method == "device" {
        assert!(shown.contains("Enter device code: FAKE-CODE"), "{shown}");
    } else {
        assert!(shown.contains("Open Codex sign-in ↗"), "{shown}");
        assert!(
            shown.contains("\x1b]8;;https://auth.openai.com/oauth/authorize?response_type=code"),
            "{shown}"
        );
        assert!(
            !shown.contains("  · https://auth.openai.com/oauth/authorize"),
            "{shown}"
        );
        if method == "manual" {
            assert!(shown.contains("\x1b]52;c;"), "{shown}");
            assert!(
                shown.contains("Sent sign-in URL to terminal clipboard"),
                "{shown}"
            );
        }
    }
    if method == "open_connection" {
        assert!(
            !auth_log.with_file_name("auth-result.closed").exists(),
            "login waited for the browser connection to close naturally"
        );
    }
    assert!(!shown.contains("FAKE-SECRET-CODE"), "{shown}");
    assert!(!shown.contains("pi-only-input"), "{shown}");
    let log = fs::read_to_string(pi_log).unwrap();
    assert_eq!(log.lines().count(), 2, "{log}");
    if method == "logged_out" {
        assert!(shown.contains("Sign in to"), "{shown}");
        assert!(
            shown.contains("OpenAI Codex · browser or device code"),
            "{shown}"
        );
        assert!(!shown.contains("Hand off to Pi"), "{shown}");
    }
    if matches!(method, "empty" | "logged_out") {
        assert_eq!(log.lines().nth(1).unwrap(), "--mode rpc", "{log}");
    } else {
        assert!(
            log.lines()
                .nth(1)
                .unwrap()
                .contains(&format!("--session {}", session.display())),
            "{log}"
        );
    }
    unsafe {
        libc::close(master);
    }
    fs::remove_dir_all(root).unwrap();
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
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            })
        }
    }
}
