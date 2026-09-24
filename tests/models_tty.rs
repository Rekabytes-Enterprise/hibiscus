use std::fs;
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn model_picker_marks_current_scrolls_and_cancels_without_switching() {
    let root = std::env::temp_dir().join(format!(
        "hibiscus-models-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let script = root.join("pi");
    let current = root.join("current");
    let log = root.join("switches");
    fs::write(&current, "m2\n").unwrap();
    fs::write(&script, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*)
   model=$(tr -d '\n' < "$HIBISCUS_TEST_CURRENT")
   printf '{"type":"response","id":"%s","success":true,"data":{"model":{"provider":"demo","id":"%s"},"sessionName":"chat"}}\n' "$id" "$model" ;;
  *'"type":"get_available_models"'*)
   printf '{"type":"response","id":"%s","success":true,"data":{"models":[' "$id"
   for n in 1 2 3 4 5 6 7 8; do
    [ "$n" = 1 ] || printf ','
    printf '{"provider":"demo","id":"m%s"}' "$n"
   done
   printf ']}}\n' ;;
  *'"type":"set_model"'*)
   model=$(printf '%s\n' "$line" | sed -n 's/.*"modelId":"\([^"]*\)".*/\1/p')
   printf '%s\n' "$model" >> "$HIBISCUS_TEST_LOG"
   printf '%s\n' "$model" > "$HIBISCUS_TEST_CURRENT"
   printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *) exit 13 ;;
 esac
done
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
        .env("HIBISCUS_PI", &script)
        .env("HIBISCUS_TEST_CURRENT", &current)
        .env("HIBISCUS_TEST_LOG", &log)
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
            }
            all.extend_from_slice(&buf[..n as usize]);
        }
        all
    });
    std::thread::sleep(Duration::from_millis(220));
    send(master, b"/m");
    std::thread::sleep(Duration::from_millis(120)); // suggestion appears while typing
    send(master, b"\r"); // complete /models and open the model picker
    std::thread::sleep(Duration::from_millis(220));
    // Active m2 starts selected. Move down past the five-row viewport to m7.
    for _ in 0..5 {
        send(master, b"\x1b[B");
    }
    std::thread::sleep(Duration::from_millis(120));
    send(master, b"\r");
    std::thread::sleep(Duration::from_millis(160));
    send(master, b"/models\r");
    std::thread::sleep(Duration::from_millis(200));
    send(master, b"\r"); // Enter on current m7 must not send set_model
    std::thread::sleep(Duration::from_millis(120));
    send(master, b"/models\r");
    std::thread::sleep(Duration::from_millis(200));
    send(master, b"\x1b/quit\r"); // Esc cancels; the immediately typed / is not lost
    let status = child.wait().unwrap();
    let bytes = reader.join().unwrap();
    let display = String::from_utf8_lossy(&bytes);
    assert!(status.success(), "{status}: {display}");
    assert_eq!(fs::read_to_string(&log).unwrap(), "m7\n");
    assert!(display.contains("\r\n  ╭─ ✿ Commands"), "{display}");
    assert!(display.contains("Choose a model"), "{display}");
    assert!(display.contains("\r\n  ╭─ ✿ Models · demo"), "{display}");
    assert!(display.contains("● current"), "{display}");
    assert!(display.contains("7/8"), "{display}");
    assert!(display.contains("█"), "{display}");
    assert!(display.contains("demo/m7"), "{display}");
    assert!(display.contains("Already using demo/m7."), "{display}");
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
