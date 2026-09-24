mod support;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use support::{unique_temp_dir, Pty};

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
    let root = unique_temp_dir("hibiscus-auth");
    let session = root.join("session.jsonl");
    if saved {
        fs::write(&session, "{\"type\":\"session\",\"id\":\"test\"}\n").unwrap();
    }
    let log = root.join("pi-keys");
    let args = root.join("pi-args");
    let rpc_args = root.join("rpc-args");
    let sdk = root.join("sdk.mjs");
    let sdk_marker = root.join("sdk-loaded");
    fs::write(&sdk, "import { writeFileSync } from 'node:fs'; writeFileSync(process.env.HIBISCUS_TEST_SDK_MARKER, 'loaded'); throw new Error('should not load');").unwrap();
    let script = root.join("pi");
    fs::write(&script, r#"#!/bin/sh
if [ "$1" = "--mode" ]; then
 printf '%s\n' "$*" >> "$HIBISCUS_TEST_RPC_ARGS"
 while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
  case "$line" in
   *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"sessionFile":"%s"}}\n' "$id" "$HIBISCUS_TEST_SESSION" ;;
   *'"type":"prompt"'*)
    case "$line" in
     *'"message":"hello"'*) reply=MOCK_REPLY_HELLO ;;
     *'"message":"after"'*) reply=MOCK_REPLY_AFTER ;;
     *) exit 14 ;;
    esac
    printf '{"type":"response","id":"%s","success":true}\n{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"%s"}}\n' "$id" "$reply"
    # Deliberately separate visible text from settlement to expose early input.
    sleep 0.2
    printf '{"type":"agent_settled"}\n' ;;
   *) exit 13 ;;
  esac
 done
else
 printf '%s' "$*" > "$HIBISCUS_TEST_ARGS"
 stty -echo
 printf 'MOCK_TUI_READY\n'
 IFS= read -r value
 stty echo
 printf '%s' "$value" > "$HIBISCUS_TEST_LOG"
fi
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &script)
        .env("HIBISCUS_TEST_SESSION", &session)
        .env("HIBISCUS_TEST_LOG", &log)
        .env("HIBISCUS_TEST_ARGS", &args)
        .env("HIBISCUS_TEST_RPC_ARGS", &rpc_args)
        .env("HIBISCUS_TEST_SDK_MARKER", &sdk_marker)
        .env(
            "HIBISCUS_AUTH_SDK",
            if other_provider {
                sdk.as_os_str()
            } else {
                std::ffi::OsStr::new("")
            },
        );
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("hibiscus");
    if saved {
        tty.send(b"hello\r");
        tty.wait_text("MOCK_REPLY_HELLO");
        tty.wait_text("Enter send  ·  Wheel / PgUp/PgDn scroll");
    }
    tty.send(b"/login\r");
    tty.wait_text("Sign in to");
    tty.send(if other_provider { b"\x1b[B\r" } else { b"\r" });
    tty.wait_text("Hand off to Pi for /login?");
    tty.send(b"y\r");
    // This marker is emitted only after the old reader is joined and Pi owns
    // the tty. No input is needed to unblock the reader being stopped.
    tty.wait_text("MOCK_TUI_READY");
    tty.send(b"pi-only-input\r");
    tty.wait_text("Returned from Pi.");
    tty.send(b"after\r");
    tty.wait_text("MOCK_REPLY_AFTER");
    tty.wait_text("Enter send  ·  Wheel / PgUp/PgDn scroll");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(fs::read_to_string(log).unwrap(), "pi-only-input");
    assert!(
        !sdk_marker.exists(),
        "SDK must not run for an explicitly chosen other provider"
    );
    let handed_off = fs::read_to_string(args).unwrap();
    let rpc_launches = fs::read_to_string(rpc_args).unwrap();
    let mut launches = rpc_launches.lines();
    let base = "--mode rpc --no-extensions --tools read,bash,edit,write";
    assert_eq!(launches.next(), Some(base));
    if saved {
        assert_eq!(
            launches.next(),
            Some(format!("{base} --session {}", session.display()).as_str())
        );
        assert_eq!(
            handed_off,
            format!(
                "--no-extensions --tools read,bash,edit,write --session {}",
                session.display()
            )
        );
    } else {
        assert_eq!(launches.next(), Some(base));
        assert_eq!(
            handed_off,
            "--no-extensions --tools read,bash,edit,write --no-session"
        );
    }
    assert_eq!(launches.next(), None);
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
