mod support;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};
use support::{unique_temp_dir, Pty};

#[test]
fn piped_logout_never_changes_credentials() {
    run("piped");
}

#[test]
fn codex_logout_removes_only_selected_credentials_and_reconnects_saved_session() {
    run("codex");
}
#[test]
fn other_provider_logout_works_without_pi_tui_or_changing_active_provider() {
    run("other");
}
#[test]
fn logout_works_without_a_saved_chat() {
    run("empty_session");
    run("no_model");
}
#[test]
fn escape_and_default_confirmation_cancel_without_mutation_or_reconnect() {
    run("cancel");
    run("cancel_confirm");
    run("slow_cancel");
}
#[test]
fn no_stored_credentials_and_missing_sdk_leave_chat_alive() {
    run("no_credentials");
    run("unavailable");
}
#[test]
fn logout_failures_keep_chat_alive_and_refresh_after_possible_mutation() {
    run("list_failure");
    run("mutation_failure");
}
#[test]
fn removed_credentials_with_sync_failure_are_reported_and_reloaded() {
    run("sync_failure");
}
#[test]
fn completed_logout_does_not_wait_for_a_retained_helper_handle() {
    run("hang_after_done");
}
#[test]
fn reconnect_failure_after_logout_does_not_claim_the_credential_was_preserved() {
    run("reconnect_failure");
}

fn run(mode: &str) {
    let root = unique_temp_dir("hibiscus-logout");
    let session = root.join("session.jsonl");
    if mode != "empty_session" {
        fs::write(&session, "{\"type\":\"session\",\"id\":\"test\"}\n").unwrap();
    }
    let store = root.join("fake-auth.json");
    let initial = if mode == "no_credentials" {
        serde_json::json!({})
    } else {
        serde_json::json!({"openai-codex":{"type":"oauth","access":"FAKE_PRIVATE_TOKEN"}, "anthropic":{"type":"api_key","key":"FAKE_OTHER_KEY"}})
    };
    fs::write(&store, initial.to_string()).unwrap();
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/bin/sh
printf '%s\n' "$*" >> "$TEST_ROOT/launches"
[ "$1" = --mode ] && [ "$2" = rpc ] || exit 12
if [ "$TEST_MODE" = reconnect_failure ] && [ "$(wc -l < "$TEST_ROOT/launches")" -eq 2 ]; then exit 16; fi
printf active > "$TEST_ROOT/active"
trap 'rm -f "$TEST_ROOT/active"' EXIT
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*)
   if [ "$TEST_MODE" = no_model ]; then
    printf '{"type":"response","id":"%s","success":true,"data":{"sessionFile":"%s"}}\n' "$id" "$TEST_SESSION"
    continue
   fi
   printf '{"type":"response","id":"%s","success":true,"data":{"sessionFile":"%s","model":{"provider":"openai-codex","id":"test"}}}\n' "$id" "$TEST_SESSION" ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"AFTER_LOGOUT_REPLY"}}\n'
   sleep 0.1
   printf '{"type":"agent_settled"}\n' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let sdk = root.join("sdk.mjs");
    fs::write(&sdk, r#"import { readFileSync, writeFileSync, existsSync } from 'node:fs';
const root = process.env.TEST_ROOT;
export class CredentialSynchronizationError extends Error { constructor() { super('FAKE_PRIVATE_TOKEN'); this.operation = 'logout'; } }
export class ModelRuntime {
 static async create() {
  if (process.env.TEST_MODE === 'slow_cancel') await new Promise(resolve => setTimeout(resolve, 1500));
  return {
   async listCredentials() {
    if (process.env.TEST_MODE === 'list_failure') throw Error('FAKE_PRIVATE_TOKEN');
    return Object.entries(JSON.parse(readFileSync(root + '/fake-auth.json', 'utf8'))).map(([providerId, value]) => ({providerId, type:value.type, secret:value.access || value.key}));
   },
   async logout(provider, options) {
    if (existsSync(root + '/active')) throw Error('RPC must close before removing credentials');
    if (!options.signal) throw Error('logout needs a bounded signal');
    const store = JSON.parse(readFileSync(root + '/fake-auth.json', 'utf8'));
    delete store[provider];
    writeFileSync(root + '/fake-auth.json', JSON.stringify(store));
    writeFileSync(root + '/removed', provider);
    if (process.env.TEST_MODE === 'sync_failure') throw new CredentialSynchronizationError();
    if (process.env.TEST_MODE === 'mutation_failure') throw Error('FAKE_PRIVATE_TOKEN');
    if (process.env.TEST_MODE === 'hang_after_done') setInterval(() => {}, 1000);
   }
  };
 }
}
"#).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env(
            "HIBISCUS_AUTH_SDK",
            if mode == "unavailable" {
                root.join("missing.mjs")
            } else {
                sdk
            },
        )
        .env("TEST_ROOT", &root)
        .env("TEST_MODE", mode)
        .env(
            "TEST_SESSION",
            if mode == "empty_session" {
                String::new()
            } else {
                session.to_string_lossy().into_owned()
            },
        )
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    if mode == "piped" {
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
            .write_all(b"/logout\n/quit\n")
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success());
        assert!(String::from_utf8_lossy(&result.stdout).contains("requires a full-screen terminal"));
        assert!(!root.join("removed").exists());
        let stored: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&store).unwrap()).unwrap();
        assert_eq!(stored, initial);
        fs::remove_dir_all(root).unwrap();
        return;
    }
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"/logout\r");
    let committed = !matches!(
        mode,
        "cancel"
            | "cancel_confirm"
            | "slow_cancel"
            | "no_credentials"
            | "unavailable"
            | "list_failure"
    );
    match mode {
        "slow_cancel" => {
            tty.wait_text("Reading stored providers");
            tty.send(b"\x1b/quit\r");
            tty.wait_text("Logout cancelled.");
        }
        "no_credentials" => {
            tty.wait_text("No stored credentials to remove.");
        }
        "unavailable" => {
            tty.wait_text("Inline logout needs Node");
        }
        "list_failure" => {
            tty.wait_text("Could not list stored providers");
        }
        _ => {
            tty.wait_text("Sign out of");
            if mode == "cancel" {
                tty.send(b"\x1b");
                tty.wait_text("Logout cancelled.");
            } else {
                tty.send(if mode == "other" { b"\r" } else { b"\x1b[B\r" });
                tty.wait_text("Remove stored credential");
                if mode == "cancel_confirm" {
                    tty.send(b"\r");
                    tty.wait_text("Logout cancelled.");
                } else {
                    tty.send(b"\x1b[B\r");
                    if mode == "mutation_failure" {
                        tty.wait_text("Logout failed or removal could not be confirmed.");
                    } else if mode == "reconnect_failure" {
                        tty.wait_text("Removed stored credentials for openai-codex.");
                        tty.wait_text("Use /reconnect; nothing is resent automatically.");
                    } else {
                        tty.wait_text("Logout complete. Chat is preserved.");
                    }
                }
            }
        }
    }
    if !matches!(mode, "reconnect_failure" | "slow_cancel") {
        tty.wait_text("Enter send");
        tty.send(b"after\r");
        tty.wait_text("AFTER_LOGOUT_REPLY");
        tty.wait_text("Enter send");
    }
    if mode != "slow_cancel" {
        tty.send(b"/quit\r");
    }
    let shown = tty.finish();
    assert!(!shown.contains("FAKE_PRIVATE_TOKEN"));
    assert!(!shown.contains("FAKE_OTHER_KEY"));
    assert!(!shown.contains("Hand off to Pi"));
    let stored: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&store).unwrap()).unwrap();
    if committed {
        let target = if mode == "other" {
            "anthropic"
        } else {
            "openai-codex"
        };
        assert!(stored.get(target).is_none());
        let remaining = if mode == "other" {
            "openai-codex"
        } else {
            "anthropic"
        };
        assert_eq!(stored[remaining], initial[remaining]);
        assert_eq!(fs::read_to_string(root.join("removed")).unwrap(), target);
    } else {
        assert_eq!(stored, initial);
        assert!(!root.join("removed").exists());
    }
    let launches = fs::read_to_string(root.join("launches")).unwrap();
    assert_eq!(
        launches.lines().count(),
        if committed { 2 } else { 1 },
        "{launches}"
    );
    assert!(launches.lines().all(
        |line| line.starts_with("--mode rpc --no-extensions --tools read,bash,edit,write,goal")
    ));
    if committed {
        if mode == "empty_session" {
            assert!(!launches.contains("--session"));
        } else {
            assert!(launches
                .lines()
                .last()
                .unwrap()
                .contains(&format!("--session {}", session.display())));
        }
    }
    if mode == "sync_failure" {
        assert!(shown.contains("could not synchronize"));
    }
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
