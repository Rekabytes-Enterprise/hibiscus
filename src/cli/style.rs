//! Inline terminal colors and bounded streaming diff recognition.
use std::borrow::Cow;

pub(super) const BASE: &str = "\x1b[48;2;18;18;18m\x1b[38;2;232;232;232m";
pub(super) const TEXT: &str = "\x1b[38;2;232;232;232m";
pub(super) const ADDED: &str = "\x1b[38;2;110;210;135m";
pub(super) const REMOVED: &str = "\x1b[38;2;245;110;115m";

/// Keep the CLI background after short label styles reset SGR. No OSC palette
/// changes are made, so the terminal's configured colors remain untouched.
pub(super) fn background(bytes: &[u8]) -> Cow<'_, [u8]> {
    const RESET: &[u8] = b"\x1b[0m";
    if !bytes.windows(RESET.len()).any(|w| w == RESET) {
        return Cow::Borrowed(bytes);
    }
    let mut out = Vec::with_capacity(bytes.len() + BASE.len());
    let mut rest = bytes;
    while let Some(index) = rest.windows(RESET.len()).position(|w| w == RESET) {
        out.extend_from_slice(&rest[..index + RESET.len()]);
        out.extend_from_slice(BASE.as_bytes());
        rest = &rest[index + RESET.len()..];
    }
    out.extend_from_slice(rest);
    Cow::Owned(out)
}

pub(super) fn patch_line(line: &str) -> String {
    match line.chars().next() {
        Some('+') => format!("{ADDED}{line}{TEXT}"),
        Some('-') => format!("{REMOVED}{line}{TEXT}"),
        _ => line.to_owned(),
    }
}

#[derive(Clone, Copy, Default)]
enum Mode {
    #[default]
    Plain,
    Git,
    Fenced,
}
#[derive(Clone, Copy)]
enum Tone {
    Plain,
    Added,
    Removed,
}
impl Tone {
    fn code(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Added => ADDED,
            Self::Removed => REMOVED,
        }
    }
}
enum Line {
    Prefix(String),
    Body(Tone),
}
impl Default for Line {
    fn default() -> Self {
        Self::Prefix(String::new())
    }
}

#[derive(Default)]
pub(super) struct Diff {
    mode: Mode,
    line: Line,
}
impl Diff {
    /// Retain at most an opening marker, never the reply or a complete line.
    pub(super) fn push(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            let line = std::mem::take(&mut self.line);
            if ch == '\n' {
                match line {
                    Line::Prefix(prefix) => {
                        if matches!(self.mode, Mode::Plain)
                            && matches!(prefix.as_str(), "```diff" | "```patch")
                        {
                            self.mode = Mode::Fenced;
                        } else if matches!(self.mode, Mode::Git) && prefix.is_empty() {
                            self.mode = Mode::Plain;
                        }
                        out.push_str(&prefix);
                    }
                    Line::Body(Tone::Added | Tone::Removed) => out.push_str(TEXT),
                    Line::Body(Tone::Plain) => {}
                }
                out.push(ch);
                continue;
            }
            match line {
                Line::Body(tone) => {
                    out.push(ch);
                    self.line = Line::Body(tone);
                }
                Line::Prefix(mut prefix) => {
                    prefix.push(ch);
                    if let Some((mode, tone)) = self.classify(&prefix) {
                        self.mode = mode;
                        out.push_str(tone.code());
                        out.push_str(&prefix);
                        self.line = Line::Body(tone);
                    } else {
                        self.line = Line::Prefix(prefix);
                    }
                }
            }
        }
        out
    }
    fn classify(&self, prefix: &str) -> Option<(Mode, Tone)> {
        match self.mode {
            Mode::Plain => {
                if prefix.starts_with("diff --git ") {
                    Some((Mode::Git, Tone::Plain))
                } else if matches!(prefix, "```diff " | "```patch ") {
                    Some((Mode::Fenced, Tone::Plain))
                } else if ["diff --git ", "```diff ", "```patch "]
                    .iter()
                    .any(|marker| marker.starts_with(prefix))
                {
                    None
                } else {
                    Some((Mode::Plain, Tone::Plain))
                }
            }
            Mode::Git | Mode::Fenced => {
                let tone = match prefix.chars().next() {
                    Some('+') => Some(Tone::Added),
                    Some('-') => Some(Tone::Removed),
                    _ => None,
                };
                if let Some(tone) = tone {
                    return Some((self.mode, tone));
                }
                if matches!(self.mode, Mode::Fenced) {
                    if prefix == "```" {
                        Some((Mode::Plain, Tone::Plain))
                    } else if "```".starts_with(prefix) {
                        None
                    } else {
                        Some((Mode::Fenced, Tone::Plain))
                    }
                } else if prefix.starts_with([' ', '@', '\\'])
                    || prefix.starts_with("diff --git ")
                    || prefix.starts_with("index ")
                {
                    Some((Mode::Git, Tone::Plain))
                } else if ["diff --git ", "index "]
                    .iter()
                    .any(|p| p.starts_with(prefix))
                {
                    None
                } else {
                    Some((Mode::Plain, Tone::Plain))
                }
            }
        }
    }
    pub(super) fn finish(&mut self) -> String {
        let out = match std::mem::take(&mut self.line) {
            Line::Prefix(prefix) => prefix,
            Line::Body(Tone::Added | Tone::Removed) => TEXT.to_owned(),
            Line::Body(Tone::Plain) => String::new(),
        };
        self.mode = Mode::Plain;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn unstyle(text: &str) -> String {
        [ADDED, REMOVED, TEXT]
            .iter()
            .fold(text.to_owned(), |s, code| s.replace(code, ""))
    }
    #[test]
    fn fragmented_git_diff_keeps_exact_text_and_restores_normal_prose() {
        let source = "diff --git a/f b/f\nindex 123..456\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new 🦀\n\n- normal bullet\n";
        for chunk in 1..16 {
            let mut diff = Diff::default();
            let chars = source.chars().collect::<Vec<_>>();
            let mut out = String::new();
            for part in chars.chunks(chunk) {
                out.push_str(&diff.push(&part.iter().collect::<String>()));
            }
            out.push_str(&diff.finish());
            assert_eq!(unstyle(&out), source);
            assert!(out.contains(&format!("{REMOVED}-old{TEXT}")));
            assert!(out.contains(&format!("{ADDED}+new 🦀{TEXT}")));
            assert!(out.ends_with("- normal bullet\n"));
        }
    }
    #[test]
    fn fenced_diff_closes_and_partial_markers_are_not_lost() {
        let source = "```diff\n-old\n+new\n```\n- normal bullet\ndiff";
        let mut diff = Diff::default();
        let mut out = String::new();
        for ch in source.chars() {
            out.push_str(&diff.push(&ch.to_string()));
        }
        out.push_str(&diff.finish());
        assert_eq!(unstyle(&out), source);
        assert!(out.contains(&format!("{ADDED}+new{TEXT}")));
        assert!(out.ends_with("- normal bullet\ndiff"));
    }
    #[test]
    fn label_resets_restore_only_the_local_dark_background() {
        let styled = background(b"\x1b[1mhibi\x1b[0m ");
        assert!(styled.windows(BASE.len()).any(|w| w == BASE.as_bytes()));
        assert!(!styled.windows(2).any(|w| w == b"\x1b]"));
        assert_eq!(background(b"plain").as_ref(), b"plain");
    }
}
