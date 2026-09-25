mod support;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
};
use support::{unique_temp_dir, Pty};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = unique_temp_dir("hibiscus-errors");
        fs::write(root.join("count"), "0").unwrap();
        fs::write(root.join("session.jsonl"), "").unwrap();
        let pi = root.join("pi");
        fs::write(&pi, r#"#!/bin/sh
printf '%s\n' "$*" >> "$FIXTURE/launches"
while IFS= read -r line; do
 printf '%s\n' "$line" >> "$FIXTURE/commands"
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 n=$(cat "$FIXTURE/count")
 case "$line" in
  *'"type":"get_state"'*)
   if [ "$SCENARIO" = reconnect_failure ] && [ "$(wc -l < "$FIXTURE/launches")" -eq 2 ]; then exit 8; fi
   if [ "$SCENARIO" = startup_disconnect ] && [ "$(wc -l < "$FIXTURE/launches")" -eq 1 ]; then exit 7; fi
   printf '{"type":"response","id":"%s","success":true,"data":{"sessionFile":"%s/session.jsonl","sessionName":"READY-%s"}}\n' "$id" "$FIXTURE" "$n" ;;
  *'"type":"get_available_models"'*)
   printf '{"type":"response","id":"%s","success":false,"error":"model not found"}\n' "$id" ;;
  *'"type":"clear_queue"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id" ;;
  *'"type":"abort"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '{"type":"message_end","message":{"role":"assistant","stopReason":"aborted"}}\n'
   printf '{"type":"agent_settled"}\n' ;;
  *'"type":"new_session"'*)
   printf '{"type":"response","id":"%s","success":true,"data":{"cancelled":false}}\n' "$id" ;;
  *'"type":"prompt"'*)
   n=$((n+1)); printf '%s' "$n" > "$FIXTURE/count"
   if [ "$n" -eq 1 ] && [ "$SCENARIO" = reject ]; then
    printf '{"type":"response","id":"%s","success":false,"error":"401 authentication failed"}\n' "$id"
    continue
   fi
   if [ "$n" -eq 1 ] && [ "$SCENARIO" = badresponse ]; then
    printf '{"type":"response","id":"%s"}\n' "$id"
    continue
   fi
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   if [ "$n" -eq 1 ]; then
    case "$SCENARIO" in
     disconnect|reconnect_failure)
      printf '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"PARTIAL"}}\n'
      exit 7 ;;
     malformed)
      printf 'not-json\n'
      sleep 30 ;;
     retry_wait)
      printf '{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"429 rate limit"}}\n'
      printf '{"type":"auto_retry_start","attempt":1,"maxAttempts":3,"delayMs":30000}\n'
      continue ;;
     billing|rate|retry)
      if [ "$SCENARIO" = billing ]; then err='429 insufficient_quota subscription exhausted'; else err='429 rate limit'; fi
      printf '{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"%s"}}\n' "$err"
      printf '{"type":"agent_end","willRetry":true}\n'
      if [ "$SCENARIO" != billing ]; then
       printf '{"type":"auto_retry_start","attempt":1,"maxAttempts":1,"delayMs":10}\n'
       sleep 0.1
      fi
      if [ "$SCENARIO" != retry ]; then
       printf '{"type":"auto_retry_end","success":false,"attempt":1,"finalError":"%s"}\n' "$err"
       sleep 0.1
       printf '{"type":"agent_settled"}\n'
       continue
      fi ;;
     overflow_retry)
      printf '{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"context limit exceeded"}}\n'
      printf '{"type":"compaction_start","reason":"overflow"}\n'
      printf '{"type":"compaction_end","reason":"overflow","result":{},"willRetry":true}\n' ;;
     threshold_warning)
      printf '{"type":"compaction_end","reason":"threshold","errorMessage":"summary unavailable","aborted":false}\n' ;;
     compaction)
      printf '{"type":"compaction_start","reason":"overflow"}\n'
      printf '{"type":"compaction_end","reason":"overflow","errorMessage":"context limit: compaction failed","aborted":false,"willRetry":false}\n'
      printf '{"type":"summarization_retry_finished"}\n'
      printf '{"type":"agent_settled"}\n'
      continue ;;
     warnings)
      printf '{"type":"tool_execution_end","toolName":"read","isError":true}\n'
      printf '{"type":"extension_error","error":"Example extension warning"}\n'
      printf '{"type":"extension_ui_request","method":"notify","notifyType":"error","message":"Example notification"}\n' ;;
    esac
   fi
   printf '{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"SUCCESS_%s"}]}}\n' "$n"
   if [ "$SCENARIO" = retry ]; then printf '{"type":"auto_retry_end","success":true,"attempt":1}\n'; fi
   sleep 0.1
   printf '{"type":"agent_settled"}\n' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
        fs::set_permissions(pi, fs::Permissions::from_mode(0o755)).unwrap();
        Self { root }
    }
    fn command(&self, scenario: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
        command
            .env("HIBISCUS_PI", self.root.join("pi"))
            .env("FIXTURE", &self.root)
            .env("SCENARIO", scenario)
            .env("HIBISCUS_NO_UPDATE_CHECK", "1");
        command
    }
    fn prompts(&self) -> Vec<serde_json::Value> {
        fs::read_to_string(self.root.join("commands"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|v| v["type"] == "prompt")
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn provider_and_preflight_errors_keep_chat_and_commands_alive() {
    for scenario in ["billing", "rate", "reject", "compaction"] {
        let fixture = Fixture::new();
        let mut tty = Pty::spawn(&mut fixture.command(scenario));
        tty.wait_text("READY-0");
        tty.send(b"original prompt\r");
        tty.wait_text("Use /restore to review");
        tty.wait_text("READY-1");
        tty.send(b"/models\r");
        tty.wait_text("Pi rejected get_available_models");
        tty.wait_text("Enter send");
        tty.send(b"/restore\r");
        tty.wait_text("Restored failed prompt");
        tty.wait_text("Enter send");
        tty.send(b" edited\r");
        tty.wait_text("SUCCESS_2");
        tty.wait_text("READY-2");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        assert!(!shown.contains("Pi disconnected"), "{scenario}: {shown}");
        let prompts = fixture.prompts();
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[1]["message"], "original prompt edited");
        assert_eq!(
            fs::read_to_string(fixture.root.join("launches"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
}

#[test]
fn retry_success_and_nonfatal_warnings_are_not_failed_runs() {
    for scenario in ["retry", "warnings", "overflow_retry", "threshold_warning"] {
        let fixture = Fixture::new();
        let mut tty = Pty::spawn(&mut fixture.command(scenario));
        tty.wait_text("READY-0");
        tty.send(b"hello\r");
        tty.wait_text("SUCCESS_1");
        tty.wait_text("READY-1");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        assert!(!shown.contains("Use /restore to review"), "{shown}");
        if scenario == "retry" {
            assert!(shown.contains("Pi retry 1/1"));
        } else if scenario == "warnings" {
            assert!(shown.contains("Example extension warning"));
        } else if scenario == "threshold_warning" {
            assert!(shown.contains("summary unavailable"));
        }
    }
}

#[test]
fn broken_backends_reconnect_same_session_without_replaying() {
    for scenario in ["disconnect", "malformed", "badresponse"] {
        let fixture = Fixture::new();
        let mut tty = Pty::spawn(&mut fixture.command(scenario));
        tty.wait_text("READY-0");
        tty.send(b"uncertain operation\r");
        tty.wait_text("Use /restore to review");
        tty.send(b"/reconnect\r");
        tty.wait_text("Reconnected to Pi. No prompt was resent.");
        tty.wait_text("Enter send");
        assert_eq!(fixture.prompts().len(), 1);
        tty.send(b"different operation\r");
        tty.wait_text("SUCCESS_2");
        tty.wait_text("READY-2");
        tty.send(b"/quit\r");
        tty.finish();
        let launches = fs::read_to_string(fixture.root.join("launches")).unwrap();
        assert_eq!(launches.lines().count(), 2);
        assert!(launches.lines().last().unwrap().contains(&format!(
            "--session {}/session.jsonl",
            fixture.root.display()
        )));
        assert_eq!(fixture.prompts()[1]["message"], "different operation");
    }
}

#[test]
fn failed_image_prompt_restores_the_same_attachment_without_auto_resending() {
    let fixture = Fixture::new();
    for name in ["wl-paste", "xclip", "osascript", "powershell.exe"] {
        let path = fixture.root.join(name);
        fs::write(&path, "#!/bin/sh\nif [ \"$1\" = --list-types ]; then printf 'image/png\\n'; else printf '\\211PNG\\r\\n\\032\\n'; fi\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut command = fixture.command("billing");
    command
        .env(
            "PATH",
            format!(
                "{}:{}",
                fixture.root.display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .env("WAYLAND_DISPLAY", "test")
        .env_remove("WSL_DISTRO_NAME")
        .env_remove("WSL_INTEROP");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("READY-0");
    tty.send(b"\x16");
    tty.wait_text("1 image(s) attached");
    tty.send(b"describe image\r");
    tty.wait_text("Use /restore to review");
    tty.wait_text("READY-1");
    tty.send(b"/restore\r");
    tty.wait_text("Restored failed prompt");
    tty.wait_text("1 image(s) attached");
    assert_eq!(fixture.prompts().len(), 1);
    tty.send(b"\r");
    tty.wait_text("SUCCESS_2");
    tty.wait_text("READY-2");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let prompts = fixture.prompts();
    assert_eq!(prompts.len(), 2);
    assert_eq!(prompts[0]["images"], prompts[1]["images"]);
    assert_eq!(prompts[1]["message"], "describe image");
    assert!(!shown.contains("iVBORw0KGgo"));
}

#[test]
fn escape_during_retry_is_cancellation_not_a_fatal_error() {
    let fixture = Fixture::new();
    let mut tty = Pty::spawn(&mut fixture.command("retry_wait"));
    tty.wait_text("READY-0");
    tty.send(b"hello\r");
    tty.wait_text("Pi retry 1/3");
    tty.send(b"\x1b");
    tty.wait_text("Stopped. You can keep chatting.");
    tty.wait_text("READY-1");
    tty.send(b"again\r");
    tty.wait_text("SUCCESS_2");
    tty.wait_text("READY-2");
    tty.send(b"/quit\r");
    assert!(!tty.finish().contains("Use /restore to review"));
}

#[test]
fn failed_reconnect_stays_disconnected_and_can_be_retried() {
    let fixture = Fixture::new();
    let mut tty = Pty::spawn(&mut fixture.command("reconnect_failure"));
    tty.wait_text("READY-0");
    tty.send(b"hello\r");
    tty.wait_text("Use /restore to review");
    tty.send(b"/reconnect\r");
    tty.wait_text("Error (Network)");
    tty.wait_text("Use /reconnect; nothing is resent automatically.");
    tty.send(b"not sent\r");
    tty.wait_text("Nothing was sent.");
    assert_eq!(fixture.prompts().len(), 1);
    tty.send(b"/reconnect\r");
    tty.wait_text("Reconnected to Pi. No prompt was resent.");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(fixture.prompts().len(), 1);
}

#[test]
fn startup_disconnect_keeps_ui_and_does_not_guess_a_saved_session() {
    let fixture = Fixture::new();
    let mut tty = Pty::spawn(&mut fixture.command("startup_disconnect"));
    tty.wait_text("Use /reconnect; nothing is resent automatically.");
    tty.send(b"/help\r");
    tty.wait_text("Recovery");
    tty.send(b"/reconnect\r");
    tty.wait_text("No saved session path was confirmed");
    tty.wait_text("Reconnected to Pi. No prompt was resent.");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert!(fixture.prompts().is_empty());
    let launches = fs::read_to_string(fixture.root.join("launches")).unwrap();
    let last = launches.lines().last().unwrap();
    assert!(!last.contains("--session"));
    assert!(!last.contains("--continue"));
}

#[test]
fn quitting_a_disconnected_chat_is_clean() {
    let fixture = Fixture::new();
    let mut tty = Pty::spawn(&mut fixture.command("disconnect"));
    tty.wait_text("READY-0");
    tty.send(b"hello\r");
    tty.wait_text("Use /restore to review");
    tty.send(b"/quit\r");
    tty.finish();
}

#[test]
fn failed_one_shot_and_piped_requests_still_exit_nonzero() {
    for piped in [false, true] {
        let fixture = Fixture::new();
        let mut command = fixture.command("billing");
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped());
        if !piped {
            command.arg("hello");
        }
        let mut child = command.spawn().unwrap();
        if piped {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"hello\nnext\n")
                .unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("insufficient_quota"));
        assert_eq!(fixture.prompts().len(), 1);
    }
}
