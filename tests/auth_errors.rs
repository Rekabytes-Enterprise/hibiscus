mod support;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};
use support::{unique_temp_dir, Pty};

#[test]
fn missing_sdk_and_node_are_recoverable_without_handoff() {
    run("missing_sdk", "HIBISCUS_PI");
    run("missing_node", "Node.js");
}

#[test]
fn helper_start_and_protocol_failures_are_recoverable_without_handoff() {
    run("helper_start", "permissions");
    run("malformed", "record");
    run("helper_exit", "connectivity");
    run("prompt_exit", "connectivity");
}

#[test]
fn incompatible_sdk_and_unsupported_methods_are_recoverable_without_handoff() {
    run("sdk_import", "incompatible");
    run("sdk_methods", "incompatible");
    run("unsupported", "selected");
    run("discovery_missing", "incompatible");
    run("no_providers", "providers");
}

#[test]
fn provider_failure_is_sanitized_and_can_be_retried_in_the_same_chat() {
    run("provider_failure", "connectivity");
}

const SDK: &str = r#"import {existsSync, writeFileSync} from 'node:fs';
const mode = process.env.TEST_MODE;
const retry = existsSync(process.env.TEST_ROOT + '/retry');
if (!retry && mode === 'sdk_import') throw Error('FAKE_PRIVATE_TOKEN');
export class ModelRuntime {
 static async create() {
  if (!retry && mode === 'sdk_methods') return {};
  return {
   getProviders: !retry && mode === 'discovery_missing' ? undefined : () =>
    !retry && mode === 'no_providers' ? [] : [{id:'anthropic',name:'Anthropic',auth:{apiKey:{}}}],
   getProvider: () => ({auth: !retry && mode === 'unsupported' ? {} : {oauth:{},apiKey:{}}}),
   async login(_provider, _type, interaction) {
    if (!retry && mode === 'prompt_exit') {
     setTimeout(() => process.exit(1), 150);
     await interaction.prompt({type:'secret',message:'Waiting for synthetic credential'});
    }
    if (!retry && mode === 'provider_failure') {
     await interaction.prompt({type:'secret',message:'Enter synthetic credential'});
     throw Error('FAKE_PRIVATE_TOKEN https://example.invalid/?code=FAKE_AUTH_CODE');
    }
    writeFileSync(process.env.TEST_ROOT + '/logged-in', 'synthetic login');
   }
  };
 }
}
"#;

const PI: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$TEST_ROOT/pi-args"
if [ "$1" != "--mode" ] || [ "$2" != "rpc" ]; then
 touch "$TEST_ROOT/tui-launched"
 exit 12
fi
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_session_stats"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"sessionFile":"%s","sessionId":"auth-error-test"}}\n' "$id" "$TEST_SESSION" ;;
  *'"type":"prompt"'*)
   printf '%s\n' "$line" >> "$TEST_ROOT/prompts"
   case "$line" in
    *'"message":"recoverable draft"'*) printf '{"type":"response","id":"%s","success":false,"error":"Synthetic draft failure"}\n' "$id"; continue ;;
   esac
   case "$line" in *'"message":"retry"'*) reply=AFTER_RETRY_REPLY ;; *) reply=AFTER_ERROR_REPLY ;; esac
   printf '{"type":"response","id":"%s","success":true}\n{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"%s"}}\n{"type":"agent_settled"}\n' "$id" "$reply" ;;
  *) exit 13 ;;
 esac
done
"#;

fn executable(path: &std::path::Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn run(mode: &str, expected: &str) {
    let root = unique_temp_dir("hibiscus-auth-errors");
    let pi = root.join("pi");
    let sdk = root.join("sdk.mjs");
    let node = root.join("node");
    let session = root.join("session.jsonl");
    fs::write(&session, "{\"type\":\"session\",\"id\":\"test\"}\n").unwrap();
    executable(&pi, PI);
    if mode != "missing_sdk" {
        fs::write(&sdk, SDK).unwrap();
    }
    match mode {
        "missing_node" => {}
        "helper_start" => fs::write(&node, "not executable").unwrap(),
        "malformed" => executable(&node, "#!/bin/sh\nprintf 'not-json\\n'\n"),
        "helper_exit" => executable(&node, "#!/bin/sh\nexit 1\n"),
        _ => executable(&node, "#!/bin/sh\nexec node \"$@\"\n"),
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_AUTH_SDK", &sdk)
        .env("HIBISCUS_NODE", &node)
        .env("TEST_ROOT", &root)
        .env("TEST_MODE", mode)
        .env("TEST_SESSION", &session)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    if mode == "provider_failure" {
        tty.send(b"recoverable draft\r");
        tty.wait_text("Synthetic draft failure");
        tty.wait_text("Enter send");
    }
    tty.send(b"/login\r");
    tty.wait_text("Sign in to");
    if matches!(mode, "no_providers" | "discovery_missing") {
        tty.send(b"\x1b[B\r");
    } else {
        tty.send(b"\r");
        tty.wait_text("Claude · browser login");
        tty.send(b"\r");
    }
    if mode == "provider_failure" {
        tty.wait_text("Enter synthetic credential");
        tty.send(b"FAKE_PRIVATE_TOKEN\r");
    }
    tty.wait_text("Error (");
    tty.wait_text(expected);
    tty.wait_text("Enter send");
    assert_eq!(
        fs::read_to_string(root.join("pi-args"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    if mode == "provider_failure" {
        tty.send(b"/restore\r");
        tty.wait_text("Restored failed prompt.");
        tty.wait_text("recoverable draft");
        tty.send(b"\x03"); // Clear the restored draft without submitting it.
    }
    tty.send(b"after\r");
    tty.wait_text("AFTER_ERROR_REPLY");
    tty.wait_text("Enter send");
    // Repair only isolated dependencies and retry without restarting Hibiscus.
    fs::write(&sdk, SDK).unwrap();
    executable(&node, "#!/bin/sh\nexec node \"$@\"\n");
    fs::write(root.join("retry"), "ready").unwrap();
    tty.send(b"/login\r");
    tty.wait_text("Sign in to");
    tty.send(b"\r");
    tty.wait_text("Claude · browser login");
    tty.send(b"\r");
    tty.wait_text("Codex sign-in completed.");
    tty.wait_text("Enter send");
    tty.send(b"retry\r");
    tty.wait_text("AFTER_RETRY_REPLY");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    for forbidden in [
        "Hand off to Pi",
        "Open Pi",
        "Run pi",
        "Pi /login",
        "FAKE_PRIVATE_TOKEN",
        "FAKE_AUTH_CODE",
    ] {
        assert!(!shown.contains(forbidden), "{forbidden}: {shown}");
    }
    assert!(!root.join("tui-launched").exists());
    if mode == "provider_failure" {
        let prompts = fs::read_to_string(root.join("prompts")).unwrap();
        assert_eq!(prompts.matches("recoverable draft").count(), 1, "{prompts}");
    }
    assert!(root.join("logged-in").exists());
    let launches = fs::read_to_string(root.join("pi-args")).unwrap();
    assert_eq!(launches.lines().count(), 2, "{launches}");
    assert!(launches.lines().all(|line| line.starts_with("--mode rpc ")));
    assert!(launches
        .lines()
        .nth(1)
        .unwrap()
        .ends_with(&format!("--session {}", session.display())));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn line_mode_and_piped_authentication_report_errors_without_launching_pi_tui() {
    for piped in [false, true] {
        let root = unique_temp_dir("hibiscus-auth-line");
        let pi = root.join("pi");
        executable(&pi, PI);
        // The shell sets TERM after the shared PTY harness configures its child.
        let mut command = Command::new("sh");
        command
            .args(["-c", "TERM=dumb exec \"$@\"", "auth-line-test"])
            .arg(env!("CARGO_BIN_EXE_hibiscus"))
            .env("HIBISCUS_PI", &pi)
            .env("TEST_ROOT", &root)
            .env("TEST_SESSION", "")
            .env("HIBISCUS_NO_UPDATE_CHECK", "1");
        let shown = if piped {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"/login\n/logout\nafter\n/quit\n")
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(!output.status.success());
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        } else {
            let mut tty = Pty::spawn(&mut command);
            tty.wait_text("hibiscus");
            tty.send(b"/login\r");
            tty.wait_text("Authentication requires a full-screen terminal");
            tty.send(b"/logout\r");
            tty.wait_text("Authentication requires a full-screen terminal");
            tty.send(b"after\r");
            tty.wait_text("AFTER_ERROR_REPLY");
            tty.send(b"/quit\r");
            tty.finish()
        };
        assert!(
            shown.contains("Authentication requires a full-screen terminal"),
            "{shown}"
        );
        if !piped {
            assert!(shown.contains("AFTER_ERROR_REPLY"), "{shown}");
        }
        assert!(!shown.contains("Hand off to Pi"), "{shown}");
        assert!(!root.join("tui-launched").exists());
        let launches = fs::read_to_string(root.join("pi-args")).unwrap_or_default();
        assert!(launches.lines().count() <= 1);
        if !piped {
            assert_eq!(launches.lines().count(), 1);
        }
        fs::remove_dir_all(root).unwrap();
    }
}
