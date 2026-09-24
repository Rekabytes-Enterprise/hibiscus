use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A bounded, single-reader PTY driver. Drop cleans up the isolated process
/// group even when a test panics; no blocking reader thread needs joining.
pub struct Pty {
    child: Child,
    master: File,
    output: Vec<u8>,
    cursor: usize,
}

impl Pty {
    pub fn spawn(command: &mut Command) -> Self {
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe {
            assert_eq!(
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::addr_of_mut!(size)
                ),
                0
            );
        }
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            assert_ne!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
                -1
            );
        }
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert_ne!(flags, -1);
        assert_ne!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
            -1
        );
        command
            .env("TERM", "xterm-256color")
            .env("NO_COLOR", "1")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Self {
            child: command.spawn().unwrap(),
            master,
            output: Vec::new(),
            cursor: 0,
        }
    }

    fn pump(&mut self) {
        let mut buf = [0; 8192];
        // Bound work per iteration even if the child writes continuously.
        for _ in 0..128 {
            match self.master.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.output.extend_from_slice(&buf[..n]);
                    if self.output.len() > 1024 * 1024 {
                        let remove = self.output.len() - 1024 * 1024;
                        self.output.drain(..remove);
                        self.cursor = self.cursor.saturating_sub(remove);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.raw_os_error() == Some(libc::EIO) =>
                {
                    break
                }
                Err(e) => panic!("PTY read: {e}"),
            }
        }
    }

    pub fn send(&mut self, mut bytes: &[u8]) {
        self.pump();
        self.cursor = self.output.len();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !bytes.is_empty() {
            match self.master.write(bytes) {
                Ok(0) => self.fail("PTY write returned zero"),
                Ok(n) => bytes = &bytes[n..],
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => self.fail(&format!("PTY write: {e}")),
            }
            if Instant::now() >= deadline {
                self.fail("PTY write timeout");
            }
            self.pump();
        }
    }

    pub fn wait_text(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.pump();
            if let Some(pos) = self.output[self.cursor..]
                .windows(text.len())
                .position(|w| w == text.as_bytes())
            {
                self.cursor += pos + text.len();
                return;
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                self.fail(&format!("exited {status} before {text:?}"));
            }
            if Instant::now() >= deadline {
                self.fail(&format!("waiting for {text:?}"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn finish(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.pump();
            if let Some(status) = self.child.try_wait().unwrap() {
                self.pump();
                if !status.success() {
                    self.fail(&format!("child exited {status}"));
                }
                return String::from_utf8_lossy(&self.output).into_owned();
            }
            if Instant::now() >= deadline {
                self.fail("waiting for child exit");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn fail(&mut self, stage: &str) -> ! {
        let status = self.child.try_wait();
        panic!(
            "PTY failure: {stage}; child status: {status:?}\n{}",
            String::from_utf8_lossy(&self.output)
        );
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Every fixture child starts its own session/process group. Terminate
        // its mock Pi and Node descendants as well as the direct child.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
