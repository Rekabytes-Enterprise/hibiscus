//! Pi stderr is not RPC and must never write over the alternate-screen UI.
//! Keep only a bounded, sanitized recent summary; never block the pipe on UI.
use super::error::safe_message;
use std::{
    collections::VecDeque,
    io::{self, Read},
    sync::{Arc, Mutex},
};

const MAX_LINE: usize = 4096;
const MAX_RECENT: usize = 4;

#[derive(Default)]
struct State {
    recent: VecDeque<(String, u64)>,
    earlier: u64,
    revision: u64,
}

#[derive(Clone, Default)]
pub(crate) struct Diagnostics(Arc<Mutex<State>>);

impl Diagnostics {
    pub(crate) fn revision(&self) -> u64 {
        self.0.lock().unwrap().revision
    }

    fn record(&self, line: &[u8], truncated: bool) {
        let text = String::from_utf8_lossy(line);
        if text.trim().is_empty() && !truncated {
            return;
        }
        let mut text = safe_message(&text);
        if truncated {
            text.push_str(" … [truncated]");
        }
        let mut state = self.0.lock().unwrap();
        let repeats = state
            .recent
            .iter()
            .position(|(old, _)| *old == text)
            .map(|i| state.recent.remove(i).unwrap().1.saturating_add(1))
            .unwrap_or(1);
        if state.recent.len() == MAX_RECENT {
            state.recent.pop_front();
            state.earlier = state.earlier.saturating_add(1);
        }
        state.recent.push_back((text, repeats));
        state.revision = state.revision.wrapping_add(1);
    }

    pub(crate) fn snapshot(&self) -> (u64, String) {
        let state = self.0.lock().unwrap();
        if state.recent.is_empty() {
            return (state.revision, String::new());
        }
        // Newest first so narrow terminals do not hide the latest diagnostic.
        let messages = state
            .recent
            .iter()
            .rev()
            .map(|(text, repeats)| {
                if *repeats > 1 {
                    format!("{text} (×{repeats})")
                } else {
                    text.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" · ");
        let older = if state.earlier > 0 {
            format!(" · {} earlier omitted", state.earlier)
        } else {
            String::new()
        };
        (state.revision, format!("Pi: {messages}{older}"))
    }

    pub(crate) fn drain(&self, mut reader: impl Read) -> io::Result<u64> {
        let mut parser = Lines::default();
        let mut bytes = [0; 8192];
        let mut total = 0u64;
        loop {
            match reader.read(&mut bytes) {
                Ok(0) => {
                    parser.finish(self);
                    super::transport::signal_activity();
                    return Ok(total);
                }
                Ok(n) => {
                    total = total.saturating_add(n as u64);
                    for byte in &bytes[..n] {
                        parser.byte(*byte, self);
                    }
                    super::transport::signal_activity();
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

#[derive(Default)]
enum Escape {
    #[default]
    Text,
    Start,
    Csi,
    String,
    StringEnd,
}
#[derive(Default)]
struct Lines {
    text: Vec<u8>,
    truncated: bool,
    escape: Escape,
}
impl Lines {
    fn finish(&mut self, target: &Diagnostics) {
        target.record(&self.text, self.truncated);
        self.text.clear();
        self.truncated = false;
    }
    fn byte(&mut self, byte: u8, target: &Diagnostics) {
        match self.escape {
            Escape::Start => {
                self.escape = match byte {
                    b'[' => Escape::Csi,
                    b']' | b'P' | b'X' | b'^' | b'_' => Escape::String,
                    _ => Escape::Text,
                }
            }
            Escape::Csi => {
                if (0x40..=0x7e).contains(&byte) {
                    self.escape = Escape::Text;
                }
            }
            Escape::String => match byte {
                7 => self.escape = Escape::Text,
                27 => self.escape = Escape::StringEnd,
                _ => {}
            },
            Escape::StringEnd => {
                self.escape = if byte == b'\\' || byte == 7 {
                    Escape::Text
                } else {
                    Escape::String
                }
            }
            Escape::Text => match byte {
                27 => self.escape = Escape::Start,
                b'\n' | b'\r' => self.finish(target),
                byte if byte >= 32 || byte == b'\t' => {
                    if self.text.len() < MAX_LINE {
                        self.text.push(byte);
                    } else {
                        self.truncated = true;
                    }
                }
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_ansi_and_credentials_are_sanitized_before_storage() {
        let feed = Diagnostics::default();
        let mut parser = Lines::default();
        for byte in b"\x1b[31mWarning: no models\x1b[0m\r\n\x1b]0;PRIVATE_TITLE\x07Authorization: Bearer PRIVATE_TOKEN\nWarning: sk-private\n" {
            parser.byte(*byte, &feed);
        }
        let (_, text) = feed.snapshot();
        assert!(text.contains("Warning: no models"));
        assert!(text.contains("[redacted]"));
        for private in [
            "PRIVATE_TITLE",
            "PRIVATE_TOKEN",
            "sk-private",
            "\x1b",
            "31m",
        ] {
            assert!(!text.contains(private));
        }
    }
    #[test]
    fn bursts_and_unterminated_lines_have_bounded_storage_and_deduplication() {
        let feed = Diagnostics::default();
        let wire = format!(
            "{}{}",
            "Warning: repeated\n".repeat(10000),
            "x".repeat(1024 * 1024)
        );
        assert_eq!(feed.drain(wire.as_bytes()).unwrap(), wire.len() as u64);
        let (_, text) = feed.snapshot();
        assert!(text.contains("×10000"));
        assert!(text.contains("[truncated]"));
        for n in 0..20 {
            feed.record(format!("warning {n}").as_bytes(), false);
        }
        let state = feed.0.lock().unwrap();
        assert_eq!(state.recent.len(), MAX_RECENT);
        assert!(state.earlier > 0);
        assert!(state.recent.iter().all(|(text, _)| text.len() < 4096));
    }
}
