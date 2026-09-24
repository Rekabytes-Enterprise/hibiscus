mod support;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use support::{unique_temp_dir, Pty};

#[test]
fn codex_login_uses_pi_sdk_and_reconnects_same_session_without_pi_tui() {
    run("device");
}

#[test]
fn browser_callback_finishes_without_waiting_for_manual_code() {
    run("browser");
}

#[test]
fn failed_browser_open_keeps_login_and_device_code_fallback_available() {
    run("open_failure");
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

#[test]
fn delayed_sdk_startup_does_not_consume_keys_for_the_wrong_prompt() {
    run("slow");
}

fn run(method: &str) {
    let root = unique_temp_dir("hibiscus-auth-sdk");
    let session = root.join("chat.jsonl");
    if !matches!(method, "empty" | "logged_out") {
        fs::write(&session, "{\"type\":\"session\",\"id\":\"test\"}\n").unwrap();
    }
    let pi_log = root.join("pi-args");
    let auth_log = root.join("auth-result");
    let browser_log = root.join("opened-url");
    let browser_opener = root.join("open-browser");
    fs::write(
        &browser_opener,
        if method == "open_failure" {
            "#!/bin/sh\nexit 1\n"
        } else {
            "#!/bin/sh\nprintf '%s' \"$1\" > \"$HIBISCUS_TEST_BROWSER_LOG\"\n"
        },
    )
    .unwrap();
    fs::set_permissions(&browser_opener, fs::Permissions::from_mode(0o755)).unwrap();
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/bin/sh
printf '%s\n' "$*" >> "$HIBISCUS_TEST_PI_LOG"
[ "$1" = "--mode" ] && [ "$2" = "rpc" ] || exit 12
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"sessionFile":"%s","model":{"provider":"%s","id":"gpt-test"}}}\n' "$id" "$HIBISCUS_TEST_SESSION" "$HIBISCUS_TEST_PROVIDER" ;;
  *'"type":"prompt"'*)
   case "$line" in
    *'"message":"hello"'*) reply=MOCK_REPLY_HELLO ;;
    *'"message":"after"'*) reply=MOCK_REPLY_AFTER ;;
    *) exit 14 ;;
   esac
   printf '{"type":"response","id":"%s","success":true}\n{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"%s"}}\n' "$id" "$reply"
   sleep 0.2
   printf '{"type":"agent_settled"}\n' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let sdk = root.join("sdk.mjs");
    fs::write(&sdk, r#"import { writeFileSync } from 'node:fs';
export class ModelRuntime {
  static async create() {
    if (process.env.HIBISCUS_TEST_AUTH_METHOD === 'slow') {
      await new Promise(resolve => setTimeout(resolve, 1500));
    }
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

    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_AUTH_SDK", &sdk)
        .env("HIBISCUS_BROWSER_OPEN", &browser_opener)
        .env("HIBISCUS_TEST_BROWSER_LOG", &browser_log)
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
        );
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("hibiscus");
    if !matches!(method, "empty" | "logged_out") {
        tty.send(b"hello\r");
        tty.wait_text("MOCK_REPLY_HELLO");
        tty.wait_text("Enter send  ·  Wheel / PgUp/PgDn scroll");
    }
    tty.send(b"/login\r");
    tty.wait_text("Sign in to");
    tty.send(b"\r");
    tty.wait_text("Browser login");
    if method == "cancel" {
        tty.send(b"\x1b");
        tty.wait_text("Codex sign-in cancelled.");
        tty.send(b"/quit\r");
        tty.finish();
        assert!(!auth_log.exists());
        assert!(!browser_log.exists());
        assert_eq!(
            fs::read_to_string(pi_log).unwrap().trim(),
            "--mode rpc --no-extensions --tools read,bash,edit,write"
        );
        drop(tty);
        fs::remove_dir_all(root).unwrap();
        return;
    }
    tty.send(if method == "device" {
        b"\x1b[B\r"
    } else {
        b"\r"
    });
    if method == "manual" {
        tty.wait_text("Paste redirect URL:");
        tty.send(b"\x19");
        tty.wait_text("Sent sign-in URL to terminal clipboard");
        tty.send(b"FAKE-SECRET-CODE\r");
    }
    tty.wait_text("Codex sign-in completed. Reconnected Pi to this chat.");
    tty.send(b"after\r");
    tty.wait_text("MOCK_REPLY_AFTER");
    tty.wait_text("Enter send  ·  Wheel / PgUp/PgDn scroll");
    tty.send(b"/quit\r");
    let shown = tty.finish();
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
        assert!(!browser_log.exists());
    } else {
        if method == "open_failure" {
            assert!(
                shown.contains("browser did not open; Ctrl+Y to copy link or")
                    && shown.contains("retry with device code"),
                "{shown}"
            );
            assert!(!browser_log.exists());
        } else {
            assert!(
                shown.contains("opened in browser; Ctrl+Y to copy link"),
                "{shown}"
            );
            assert!(fs::read_to_string(&browser_log)
                .unwrap()
                .starts_with("https://auth.openai.com/oauth/authorize?response_type=code"));
        }
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
    let base = "--mode rpc --no-extensions --tools read,bash,edit,write";
    assert_eq!(log.lines().next().unwrap(), base, "{log}");
    if method == "logged_out" {
        assert!(shown.contains("Sign in to"), "{shown}");
        assert!(
            shown.contains("OpenAI Codex · browser or device code"),
            "{shown}"
        );
        assert!(!shown.contains("Hand off to Pi"), "{shown}");
    }
    if matches!(method, "empty" | "logged_out") {
        assert_eq!(log.lines().nth(1).unwrap(), base, "{log}");
    } else {
        assert!(
            log.lines().nth(1).unwrap() == format!("{base} --session {}", session.display()),
            "{log}"
        );
    }
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
