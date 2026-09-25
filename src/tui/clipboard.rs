//! Explicit clipboard image reads. No image files or clipboard contents are
//! logged. Platform helpers run with bounded output and a deadline.
use serde_json::{json, Value};
use std::{
    env,
    io::{self, Read},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_IMAGE: usize = 10 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);

fn is_wsl() -> bool {
    cfg!(target_os = "linux")
        && (env::var_os("WSL_DISTRO_NAME").is_some() || env::var_os("WSL_INTEROP").is_some())
}

pub(super) fn paste_key() -> &'static str {
    if is_wsl() {
        "Alt+V"
    } else {
        "Ctrl+V"
    }
}

pub(super) struct Job {
    pub(super) receiver: Receiver<Result<Value, String>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Job {
    pub(super) fn start() -> Self {
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let worker = thread::spawn(move || {
            let _ = sender.send(read_image(&stop));
        });
        Self {
            receiver,
            cancelled,
            worker: Some(worker),
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn read_image(cancelled: &AtomicBool) -> Result<Value, String> {
    let capture =
        |command: &mut Command, timeout, limit| capture(command, timeout, limit, cancelled);
    let bytes = if cfg!(target_os = "macos") {
        // AppKit converts screenshot/TIFF clipboard data to PNG in memory.
        capture(
            Command::new("osascript").args([
                "-l",
                "JavaScript",
                "-e",
                r#"
ObjC.import('AppKit'); ObjC.import('Foundation');
const image = $.NSImage.alloc.initWithPasteboard($.NSPasteboard.generalPasteboard);
if (!image.isNil()) {
    const rep = $.NSBitmapImageRep.imageRepWithData(image.TIFFRepresentation);
    const png = rep.representationUsingTypeProperties($.NSBitmapImageFileTypePNG, $({}));
    $.NSFileHandle.fileHandleWithStandardOutput.writeData(png);
}
void 0;
"#,
            ]),
            TIMEOUT,
            MAX_IMAGE,
        )?
    } else if is_wsl() {
        capture(
            Command::new("powershell.exe").args([
                "-NoProfile",
                "-NonInteractive",
                "-STA",
                "-Command",
                r#"
Add-Type -AssemblyName System.Windows.Forms;
Add-Type -AssemblyName System.Drawing;
$img = [System.Windows.Forms.Clipboard]::GetImage();
if ($img) {
    $stream = New-Object System.IO.MemoryStream;
    try {
        $img.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png);
        $bytes = $stream.ToArray();
        [Console]::OpenStandardOutput().Write($bytes, 0, $bytes.Length);
    } finally { $stream.Dispose(); $img.Dispose(); }
}
"#,
            ]),
            TIMEOUT,
            MAX_IMAGE,
        )?
    } else if env::var_os("WAYLAND_DISPLAY").is_some()
        || env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
    {
        // Do not fall back to an unrelated/stale X11 clipboard if Wayland has no image.
        let types = capture(Command::new("wl-paste").arg("--list-types"), TIMEOUT, 8192)?;
        let mime = preferred_type(&types).ok_or("No supported image in clipboard")?;
        capture(
            Command::new("wl-paste").args(["--type", mime, "--no-newline"]),
            TIMEOUT,
            MAX_IMAGE,
        )?
    } else if env::var_os("DISPLAY").is_some() {
        let types = capture(
            Command::new("xclip").args(["-selection", "clipboard", "-t", "TARGETS", "-o"]),
            TIMEOUT,
            8192,
        )?;
        let mime = preferred_type(&types).ok_or("No supported image in clipboard")?;
        capture(
            Command::new("xclip").args(["-selection", "clipboard", "-t", mime, "-o"]),
            TIMEOUT,
            MAX_IMAGE,
        )?
    } else {
        return Err("Clipboard unavailable: use a local desktop (wl-paste/xclip on Linux)".into());
    };
    image_content(&bytes)
}

fn preferred_type(types: &[u8]) -> Option<&'static str> {
    let types = String::from_utf8_lossy(types);
    ["image/png", "image/jpeg", "image/webp", "image/gif"]
        .into_iter()
        .find(|mime| types.lines().any(|line| line.trim() == *mime))
}

fn image_content(bytes: &[u8]) -> Result<Value, String> {
    if bytes.is_empty() {
        return Err("No image in clipboard".into());
    }
    if bytes.len() > MAX_IMAGE {
        return Err("Clipboard image exceeds 10 MiB".into());
    }
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        return Err("Clipboard does not contain a PNG, JPEG, GIF, or WebP image".into());
    };
    let mut image = json!({"type":"image", "mimeType":mime});
    // Move the encoded buffer into the value; json!(base64(bytes)) serializes
    // the temporary String by reference and allocates another full copy.
    image["data"] = Value::String(super::screen::base64(bytes));
    Ok(image)
}

fn capture(
    command: &mut Command,
    timeout: Duration,
    limit: usize,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Clipboard read cancelled".into());
    }
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped())
        .stderr(Stdio::null()).process_group(0).spawn()
        .map_err(|_| "Clipboard helper unavailable (macOS: osascript; WSL: powershell.exe; Linux: wl-paste/xclip)".to_owned())?;
    let result = (|| {
        let mut stdout = child.stdout.take().ok_or("Clipboard output unavailable")?;
        let fd = stdout.as_raw_fd();
        // SAFETY: the pipe is owned here and remains open throughout this call.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err("Cannot read clipboard pipe".to_owned());
            }
        }
        let deadline = Instant::now() + timeout;
        let mut bytes = Vec::new();
        let mut eof = false;
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err("Clipboard read cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("Clipboard read timed out".into());
            }
            let mut chunk = [0; 8192];
            match stdout.read(&mut chunk) {
                Ok(0) => eof = true,
                Ok(n) => {
                    if bytes.len() + n > limit {
                        return Err(
                            "Clipboard image/output exceeds size limit (10 MiB per image)".into(),
                        );
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("Clipboard read failed".into()),
            }
            if eof {
                if let Some(status) = child.try_wait().map_err(|_| "Clipboard helper failed")? {
                    return if status.success() {
                        Ok(bytes)
                    } else {
                        Err("Clipboard helper failed or no image is available".into())
                    };
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    })();
    // SAFETY: the child was started in its own process group. Also terminate
    // descendants retaining stdout, so a broken clipboard helper cannot hang UI.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(command: &mut Command, timeout: Duration, limit: usize) -> Result<Vec<u8>, String> {
        super::capture(command, timeout, limit, &AtomicBool::new(false))
    }

    #[test]
    fn dropping_a_clipboard_job_cancels_and_reaps_its_helper() {
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let worker = thread::spawn(move || {
            let result = super::capture(
                Command::new("sh").args(["-c", "sleep 10"]),
                TIMEOUT,
                100,
                &stop,
            );
            let _ = sender.send(result.map(|_| Value::Null));
        });
        let job = Job {
            receiver,
            cancelled,
            worker: Some(worker),
        };
        thread::sleep(Duration::from_millis(30));
        let start = Instant::now();
        drop(job);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn image_blocks_are_typed_and_never_contain_raw_bytes() {
        let block = image_content(b"\x89PNG\r\n\x1a\n").unwrap();
        assert_eq!(block["type"], "image");
        assert_eq!(block["mimeType"], "image/png");
        assert_eq!(block["data"], "iVBORw0KGgo=");
        assert!(image_content(b"not an image").is_err());
        assert!(image_content(b"").is_err());
        assert!(image_content(&vec![0; MAX_IMAGE + 1]).is_err());
        assert_eq!(
            preferred_type(b"text/plain\nimage/jpeg\nimage/png\n"),
            Some("image/png")
        );
        assert_eq!(preferred_type(b"text/plain"), None);
    }

    #[test]
    fn clipboard_helpers_have_deadlines_and_output_limits() {
        assert_eq!(
            capture(
                Command::new("sh").args(["-c", "printf ok"]),
                Duration::from_secs(1),
                8
            )
            .unwrap(),
            b"ok"
        );
        assert!(capture(
            Command::new("sh").args(["-c", "printf toolong"]),
            Duration::from_secs(1),
            2
        )
        .is_err());
        let start = Instant::now();
        assert!(capture(
            Command::new("sh").args(["-c", "sleep 10"]),
            Duration::from_millis(40),
            8
        )
        .is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(capture(
            Command::new("sh").args(["-c", "exit 1"]),
            Duration::from_secs(1),
            8
        )
        .is_err());
    }
}
