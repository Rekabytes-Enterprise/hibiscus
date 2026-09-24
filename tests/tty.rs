use std::fs;
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn escape_aborts_active_run_and_next_prompt_still_works() {
    let root = std::env::temp_dir().join(format!(
        "hibiscus-tty-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let script = root.join("pi");
    fs::write(&script, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"sessionName":"test","model":{"provider":"demo","id":"small"}}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   case "$line" in
    *'second'*)
     printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"thinking_start"}}'
     printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","delta":"private thought"}}'
     printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"thinking_end"}}'
     printf '%s\n' '{"type":"tool_execution_start","toolCallId":"r1","toolName":"read","args":{"path":"README.md"}}'
     printf '%s\n' '{"type":"tool_execution_end","toolCallId":"r1","toolName":"read","isError":false}'
     printf '%s\n' '{"type":"tool_execution_start","toolCallId":"w1","toolName":"write","args":{"path":"notes.md"}}'
     printf '%s\n' '{"type":"tool_execution_end","toolCallId":"w1","toolName":"write","isError":false}'
     printf '%s\n' '{"type":"tool_execution_start","toolCallId":"e1","toolName":"edit","args":{"path":"broken.md"}}'
     printf '%s\n' '{"type":"tool_execution_end","toolCallId":"e1","toolName":"edit","isError":true,"result":{"details":{"diff":"+999 must-not-show"}}}'
     printf '%s\n' '{"type":"tool_execution_start","toolCallId":"e2","toolName":"edit","args":{"path":"updated.md"}}'
     printf '%s\n' '{"type":"tool_execution_end","toolCallId":"e2","toolName":"edit","isError":false,"result":{"details":{"diff":" 11 context\n-12 old line\n+12 new line\n 13 after"}}}'
     printf '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"**done** with `code`"}}\n{"type":"agent_settled"}\n' ;;
   esac
   ;;
  *'"type":"clear_queue"'*) printf '{"type":"response","id":"%s","success":true}\n' "$id" ;;
  *'"type":"abort"'*) printf '{"type":"response","id":"%s","success":true}\n{"type":"agent_settled"}\n' "$id" ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    // A controlling PTY lets Hibiscus open /dev/tty, unlike a pipe-backed test.
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
        .env("NO_COLOR", "1")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .pre_exec_set_tty()
        .spawn()
        .unwrap();
    let reader = std::thread::spawn(move || {
        let mut transcript = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = unsafe { libc::read(master, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            transcript.extend_from_slice(&buf[..n as usize]);
        }
        transcript
    });
    std::thread::sleep(Duration::from_millis(250));
    let input = b"first\r";
    assert_eq!(
        unsafe { libc::write(master, input.as_ptr().cast(), input.len()) },
        input.len() as isize
    );
    std::thread::sleep(Duration::from_millis(300));
    let keys = b"\x1b";
    assert_eq!(
        unsafe { libc::write(master, keys.as_ptr().cast(), keys.len()) },
        1
    );
    std::thread::sleep(Duration::from_millis(300));
    let input = b"second\r";
    assert_eq!(
        unsafe { libc::write(master, input.as_ptr().cast(), input.len()) },
        input.len() as isize
    );
    std::thread::sleep(Duration::from_millis(300));
    // Scrolling must continue to work after multi-tool progress and diff output.
    let scroll = b"\x1b[5~\x1b[5~\x1b[<64;10;10M";
    assert_eq!(
        unsafe { libc::write(master, scroll.as_ptr().cast(), scroll.len()) },
        scroll.len() as isize
    );
    std::thread::sleep(Duration::from_millis(100));
    let input = b"/quit\r";
    assert_eq!(
        unsafe { libc::write(master, input.as_ptr().cast(), input.len()) },
        input.len() as isize
    );
    let status = child.wait().unwrap();
    let bytes = reader.join().unwrap();
    let transcript = String::from_utf8_lossy(&bytes);
    assert!(status.success(), "{status}: {transcript}");
    assert!(transcript.contains("✿ hibiscus"), "{transcript}");
    assert!(transcript.contains("demo/small  ·  test"), "{transcript}");
    assert!(transcript.contains("│  done with code"), "{transcript}");
    assert!(transcript.contains("❀ hibiscus"), "{transcript}");
    assert!(transcript.contains("reasoning · <1s"), "{transcript}");
    assert!(transcript.contains("↳ read · README.md"), "{transcript}");
    assert!(
        transcript.contains("✓ done · read · README.md"),
        "{transcript}"
    );
    assert!(transcript.contains("↳ write · notes.md"), "{transcript}");
    assert!(
        transcript.contains("✓ done · write · notes.md"),
        "{transcript}"
    );
    assert!(
        transcript.contains("✗ failed · edit · broken.md"),
        "{transcript}"
    );
    assert!(
        transcript.contains("✓ done · edit · updated.md"),
        "{transcript}"
    );
    assert!(transcript.contains("diff · updated.md"), "{transcript}");
    assert!(transcript.contains("-12 old line"), "{transcript}");
    assert!(transcript.contains("+12 new line"), "{transcript}");
    assert!(!transcript.contains("must-not-show"), "{transcript}");
    assert!(!transcript.contains("private thought"), "{transcript}");
    assert!(!transcript.contains("│  **done**"), "{transcript}");
    assert!(
        transcript.contains("Stopped. You can keep chatting."),
        "{transcript}"
    );
    assert!(transcript.contains("╭"), "{transcript}");
    assert!(transcript.contains("\x1b[?1049h"), "{transcript}");
    assert!(transcript.contains("\x1b[?1049l"), "{transcript}");
    assert!(transcript.contains("\x1b[?1000h"), "{transcript}");
    assert!(transcript.contains("\x1b[?1000l"), "{transcript}");
    assert!(!transcript.contains("\x1b[35m"), "{transcript}");
    unsafe {
        libc::close(master);
    }
    fs::remove_dir_all(root).unwrap();
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
