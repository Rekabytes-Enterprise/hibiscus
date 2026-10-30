//! Bounded bracketed-paste decoding shared by both terminal input views.
pub(super) struct Input {
    bytes: Vec<u8>,
    overflow: bool,
    limit: usize,
}

impl Default for Input {
    fn default() -> Self {
        Self::with_limit(8192)
    }
}
impl Input {
    pub(super) fn with_limit(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            overflow: false,
            limit,
        }
    }
    const END: &'static [u8] = b"\x1b[201~";
    pub(super) fn push(&mut self, byte: u8) -> Option<Option<String>> {
        self.bytes.push(byte);
        if self.bytes.ends_with(Self::END) {
            self.bytes.truncate(self.bytes.len() - Self::END.len());
            if self.overflow || self.bytes.len() > self.limit {
                return Some(None);
            }
            let text = match std::str::from_utf8(&self.bytes) {
                Ok(text) => text,
                Err(_) => return Some(None),
            };
            let mut clean = String::new();
            // tmux can re-encode pasted Ctrl+J as CSI-u; treat it as text
            // rather than allowing it to become a submit/keyboard event.
            let decoded = text.replace("\x1b[106;5u", "\n");
            let mut chars = decoded.chars().peekable();
            while let Some(ch) = chars.next() {
                match ch {
                    '\r' => {
                        if chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                        clean.push('\n');
                    }
                    '\n' => clean.push('\n'),
                    '\t' => clean.push_str("    "),
                    ch if !ch.is_control() => clean.push(ch),
                    _ => {}
                }
            }
            return Some(Some(clean));
        }
        if self.bytes.len() > self.limit + Self::END.len() {
            self.overflow = true;
            self.bytes.drain(..self.bytes.len() - Self::END.len());
        }
        None
    }
}
