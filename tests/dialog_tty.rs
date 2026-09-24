use std::fs;
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn confirm_can_be_approved_in_chat() {
    let root = std::env::temp_dir().join(format!(
        "hibiscus-dialog-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let script = root.join("pi");
    let log = root.join("reply");
    fs::write(&script, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"sessionName":"test","model":{"provider":"demo","id":"small"}}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '{"type":"extension_ui_request","id":"ui-1","method":"confirm","title":"Approve tool","message":"Run it?"}\n'
   ;;
  *'"type":"extension_ui_response"'*)
   printf '%s\n' "$line" > "$HIBISCUS_TEST_REPLY"
   printf '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"ok"}}\n{"type":"agent_settled"}\n'
   ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let mut master = 0;
    let mut slave = 0;
    let mut size = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::addr_of_mut!(size),
            )
        },
        0
    );
    let stdin = unsafe { std::fs::File::from_raw_fd(slave) };
    let stdout = stdin.try_clone().unwrap();
    let stderr = stdin.try_clone().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
        .env("TERM", "xterm-256color")
        .env("HIBISCUS_PI", &script)
        .env("HIBISCUS_TEST_REPLY", &log)
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .pre_exec_set_tty()
        .spawn()
        .unwrap();
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while unsafe { libc::read(master, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
    });
    std::thread::sleep(Duration::from_millis(250));
    write_bytes(master, b"hello\r");
    std::thread::sleep(Duration::from_millis(250));
    write_bytes(master, b"yes\r");
    std::thread::sleep(Duration::from_millis(250));
    write_bytes(master, b"/quit\r");
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert!(status.success(), "{status}");
    let reply: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&log).unwrap()).unwrap();
    assert_eq!(reply["type"], "extension_ui_response");
    assert_eq!(reply["id"], "ui-1");
    assert_eq!(reply["confirmed"], true);
    unsafe {
        libc::close(master);
    }
    fs::remove_dir_all(root).unwrap();
}

fn write_bytes(fd: i32, bytes: &[u8]) {
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
