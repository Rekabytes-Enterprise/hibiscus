use crate::Result;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// Owns a terminal in raw mode, independent of piped chat input/output.
/// The reader thread exits at process shutdown; do not join it while blocked on input.
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
        let tty = self.tty.try_clone()?;
        let fd = tty.as_raw_fd();
        // A short poll timeout lets a handoff stop and join this reader without
        // injecting a byte into the terminal or waiting for a keypress.
        let (sender, receiver) = mpsc::channel();
        // Each reader is stopped and joined before handing the tty to Pi. A
        // detached reader could steal Pi's login keystrokes even after raw mode
        // was restored.
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = stop.clone();
        let reader = thread::spawn(move || {
            let mut tty = tty;
            let mut byte = [0];
            while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
                let mut poll_fd = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: fd is owned by the reader thread for the whole poll.
                match unsafe { libc::poll(&mut poll_fd, 1, 50) } {
                    0 => continue,
                    n if n < 0 => break,
                    _ => {}
                }
                match tty.read(&mut byte) {
                    Ok(1) if sender.send(byte[0]).is_err() => break,
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
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
