use crate::Result;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// Owns a terminal in raw mode, independent of piped chat input/output.
/// The independently opened, nonblocking reader can be stopped before handoff.
pub(crate) struct Terminal {
    tty: File,
    original: libc::termios,
}

impl Terminal {
    pub(crate) fn open() -> Option<Self> {
        let tty = File::options()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .ok()?;
        let fd = tty.as_raw_fd();
        let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: fd is a live tty; tcgetattr writes a termios on success.
        if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
            return None;
        }
        let original = unsafe { original.assume_init() };
        Some(Self { tty, original })
    }

    pub(crate) fn events(&self) -> io::Result<TerminalEvents> {
        // Do not set O_NONBLOCK on a clone: clones share file status flags with
        // the output handle. A separate open keeps terminal writes blocking.
        let tty = File::options()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open("/dev/tty")?;
        Self::events_from(tty)
    }

    // The caller supplies an independently opened nonblocking tty.
    fn events_from(tty: File) -> io::Result<TerminalEvents> {
        let fd = tty.as_raw_fd();
        if fd as usize >= libc::FD_SETSIZE {
            return Err(io::Error::other(
                "terminal descriptor exceeds select capacity",
            ));
        }
        // select supports Darwin's controlling tty device. Nonblocking reads
        // also prevent stale readiness from trapping stop()/join().
        let (sender, receiver) = mpsc::channel();
        // Each reader is stopped and joined before handing the tty to Pi. A
        // detached reader could steal Pi's login keystrokes even after raw mode
        // was restored.
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = stop.clone();
        let reader = thread::spawn(move || {
            let mut tty = tty;
            let mut bytes = [0; 4096];
            while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
                // SAFETY: fd is owned here and checked against FD_SETSIZE.
                let ready = unsafe {
                    let mut reads: libc::fd_set = std::mem::zeroed();
                    libc::FD_ZERO(&mut reads);
                    libc::FD_SET(fd, &mut reads);
                    let mut timeout = libc::timeval {
                        tv_sec: 0,
                        tv_usec: 50_000,
                    };
                    libc::select(
                        fd + 1,
                        &mut reads,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        &mut timeout,
                    )
                };
                if ready == 0 {
                    continue;
                }
                if ready < 0 {
                    if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    break;
                }
                if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                match tty.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => {
                        for byte in &bytes[..n] {
                            if sender.send(*byte).is_err() {
                                return;
                            }
                        }
                        crate::pi::transport::signal_activity();
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                        ) => {}
                    Err(_) => break,
                }
            }
        });
        Ok(TerminalEvents {
            receiver,
            stop,
            reader: Some(reader),
        })
    }

    pub(crate) fn raw(&self) -> io::Result<RawMode> {
        let fd = self.tty.as_raw_fd();
        let mut raw = self.original;
        let writer = self.tty.try_clone()?;
        // Disable line buffering, echo, and terminal-generated signals. Escape
        // is delivered as a byte immediately rather than waiting for Enter.
        unsafe { libc::cfmakeraw(&mut raw) };
        // SAFETY: fd is a live tty and both termios values are initialized.
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(RawMode {
            fd,
            original: self.original,
            tty: writer,
        })
    }
}

pub(crate) struct TerminalEvents {
    pub(crate) receiver: Receiver<u8>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    reader: Option<thread::JoinHandle<()>>,
}

impl TerminalEvents {
    pub(crate) fn stop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for TerminalEvents {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) struct RawMode {
    fd: i32,
    original: libc::termios,
    tty: File,
}

impl RawMode {
    pub(crate) fn write(&mut self, text: &str) -> io::Result<()> {
        self.tty.write_all(text.as_bytes())?;
        self.tty.flush()
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: fd is owned by Terminal and remains open for this guard's lifetime.
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.original) };
    }
}

/// A single raw-mode reader for prompts and extension dialogs; never combine
/// it with a buffered reader of the same tty (which could consume future input).
pub(crate) fn read_line(events: &Receiver<u8>, raw: &mut RawMode) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    let mut pending_utf8 = Vec::new();
    while let Ok(byte) = events.recv() {
        match byte {
            b'\r' | b'\n' => {
                raw.write("\r\n")?;
                return Ok(Some(String::from_utf8(bytes)?));
            }
            3 | 4 => return Ok(None), // Ctrl+C / Ctrl+D at the editor
            8 | 127 => {
                pending_utf8.clear();
                if !bytes.is_empty() {
                    while bytes.last().is_some_and(|b| b & 0b1100_0000 == 0b1000_0000) {
                        bytes.pop();
                    }
                    bytes.pop();
                    raw.write("\x08 \x08")?;
                }
            }
            27 => {} // Esc in the editor does not cancel an idle session.
            32..=126 | 128..=255 => {
                pending_utf8.push(byte);
                match std::str::from_utf8(&pending_utf8) {
                    Ok(text) => {
                        raw.write(text)?;
                        bytes.extend_from_slice(&pending_utf8);
                        pending_utf8.clear();
                    }
                    Err(error) if error.error_len().is_none() => {}
                    Err(_) => pending_utf8.clear(),
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::FromRawFd;
    use std::time::Duration;

    #[test]
    fn idle_terminal_reader_stops_without_an_extra_keypress() {
        let mut master = -1;
        let mut slave = -1;
        unsafe {
            assert_eq!(
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut()
                ),
                0
            );
        }
        let mut master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        unsafe {
            let mut raw: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(slave.as_raw_fd(), &mut raw), 0);
            libc::cfmakeraw(&mut raw);
            assert_eq!(libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &raw), 0);
            let flags = libc::fcntl(slave.as_raw_fd(), libc::F_GETFL);
            assert_ne!(flags, -1);
            assert_ne!(
                libc::fcntl(slave.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK),
                -1
            );
        }
        let mut events = Terminal::events_from(slave).unwrap();
        master.write_all(b"x").unwrap();
        assert_eq!(
            events
                .receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            b'x'
        );
        let (done, stopped) = mpsc::channel();
        let worker = thread::spawn(move || {
            events.stop();
            done.send(()).unwrap();
        });
        stopped
            .recv_timeout(Duration::from_secs(2))
            .expect("idle reader did not stop");
        worker.join().unwrap();
    }
}
