//! Copy selected UI text, never draft/image payloads. Prefer local desktop
//! clipboard tools; OSC 52 addresses the terminal-side clipboard (WSL/SSH).
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn desktop_copy(text: &str) -> bool {
    // Avoid blocking the UI on a full pipe to an unresponsive clipboard tool.
    if text.len() > 2048 {
        return false;
    }
    let mut candidates: Vec<(&str, &[&str])> = Vec::new();
    #[cfg(target_os = "macos")]
    candidates.push(("pbcopy", &[]));
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            candidates.push(("wl-copy", &[]));
        }
        if std::env::var_os("DISPLAY").is_some() {
            candidates.push(("xclip", &["-selection", "clipboard"]));
            candidates.push(("xsel", &["--clipboard", "--input"]));
        }
    }
    for (program, args) in candidates {
        let Ok(mut child) = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        let written = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
        if !written {
            let _ = child.kill();
            let _ = child.wait();
            continue;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        return true;
                    }
                    break;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
    }
    false
}

pub(super) fn copy(out: &mut impl Write, text: &str) -> io::Result<()> {
    if text.is_empty() || desktop_copy(text) {
        return Ok(());
    }
    // Unsupported terminals may ignore OSC 52. Do not claim success in the UI.
    // Bound the encoded escape (Pi also caps OSC 52 payloads).
    if text.len() <= 75_000 {
        write!(
            out,
            "\x1b]52;c;{}\x07",
            super::screen::base64(text.as_bytes())
        )?;
        out.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_copy_is_bounded_and_uses_terminal_clipboard_without_a_desktop() {
        // Test the fallback without touching an actual system clipboard.
        let mut output = Vec::new();
        let text = "x".repeat(75_001);
        copy(&mut output, &text).unwrap();
        assert!(output.is_empty());
    }
}
