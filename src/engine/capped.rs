//! One size limit for what a tool sends the model. Colour codes and redrawn
//! progress lines go first; output still over the limit keeps its start and its
//! end (where errors and summaries are), and the whole of it is saved to a file
//! the agent can read or search. Saved files are removed after a week.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// About what Claude Code sends inline for a command.
pub(in crate::engine) const INLINE: usize = 30_000;
const HEAD: usize = 2_000;
const TAIL: usize = 8_000;
const KEEP: Duration = Duration::from_secs(7 * 24 * 3600);

/// Tools that page their own output.
pub(in crate::engine) fn exempt(tool: &str) -> bool {
    matches!(tool, "read" | "grep" | "glob")
}

/// The text the model gets for a tool's output.
pub(super) fn cap(text: &str) -> String {
    let text = clean(text);
    if text.len() <= INLINE {
        return text;
    }
    let saved = save(&text);
    shorten(&text, saved)
}

/// Smaller contexts keep the complete output in the same recoverable file.
pub(super) fn cap_to(text: &str, limit: usize) -> String {
    let text = clean(text);
    if text.len() <= limit {
        return text;
    }
    let note = match save(&text) {
        Ok(path) => format!(
            "\n[Output shortened for context. Full output saved to {}.]\n",
            path.display()
        ),
        Err(_) => "\n[Output shortened for context. Full output could not be saved.]\n".to_owned(),
    };
    if note.len() > limit {
        let fallback = "[Output shortened for context.]";
        return fallback[..fallback.floor_char_boundary(limit)].to_owned();
    }
    let available = limit - note.len();
    let head = text.floor_char_boundary(available / 3);
    let tail = text.ceil_char_boundary(text.len() - (available - head));
    format!("{}{note}{}", &text[..head], &text[tail..])
}

/// Removes terminal colour and cursor codes, and keeps only the last state of a
/// line redrawn with carriage returns, such as a progress bar.
pub(in crate::engine) fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        let last = line.rsplit('\r').find(|s| !s.is_empty()).unwrap_or("");
        strip_codes(last, &mut out);
    }
    out
}

fn strip_codes(line: &str, out: &mut String) {
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: ESC [ parameters, ending in a byte from @ to ~.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: ESC ] text, ending in BEL or ESC \.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Where the full output went, or why it couldn't be saved.
fn save(text: &str) -> std::result::Result<PathBuf, String> {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let dir = crate::paths::home()
        .map_err(|e| e.to_string())?
        .join("tool-output");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| now.duration_since(t).unwrap_or_default() > KEEP);
            if old {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let file = dir.join(format!(
        "{stamp}-{}-{}.txt",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&file, text).map_err(|e| e.to_string())?;
    Ok(file)
}

fn boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The start and end of `text` on whole lines where they fit, with a note on
/// what was left out and where the rest is.
pub(in crate::engine) fn shorten(
    text: &str,
    saved: std::result::Result<PathBuf, String>,
) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let total = lines.len();
    let (mut head, mut used) = (0, 0);
    while head < total && used + lines[head].len() < HEAD {
        used += lines[head].len() + 1;
        head += 1;
    }
    let (mut tail, mut used) = (0, 0);
    while tail < total - head && used + lines[total - 1 - tail].len() < TAIL {
        used += lines[total - 1 - tail].len() + 1;
        tail += 1;
    }
    // A single line longer than the space shows its start, or its end.
    let start = if head == 0 {
        text[..boundary(text, HEAD)].to_string()
    } else {
        lines[..head].join("\n")
    };
    let end = if tail == 0 {
        let last = lines[total - 1];
        last[boundary(last, last.len() - TAIL.min(last.len()))..].to_string()
    } else {
        lines[total - tail..].join("\n")
    };
    let shown = if total == 1 {
        "the start and end of its one line".to_string()
    } else if head == 0 || tail == 0 {
        format!("the start and end of {total} lines")
    } else {
        format!("lines 1-{head} and {}-{total} of {total}", total - tail + 1)
    };
    let rest = match saved {
        Ok(path) => format!(
            "full output ({} KB) saved to {}; read or search that file for the rest",
            text.len() / 1024,
            path.display()
        ),
        Err(e) => format!("the full output could not be saved: {e}"),
    };
    format!("{start}\n[Output too long: showing {shown}; {rest}.]\n{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_codes_and_progress_redraws_are_removed() {
        let text = "\u{1b}[32mok\u{1b}[0m\r\n 10%\r 50%\r100%\ndone\u{1b}]0;title\u{7}\n";
        assert_eq!(clean(text), "ok\n100%\ndone\n");
    }

    #[test]
    fn short_output_is_only_cleaned() {
        assert_eq!(cap("a\r\nb"), "a\nb");
    }

    #[test]
    fn long_output_keeps_its_start_and_end() {
        let text: String = (1..=5000).map(|i| format!("line {i}\n")).collect();
        let out = shorten(&text, Ok(PathBuf::from("/tmp/out.txt")));
        assert!(out.starts_with("line 1\nline 2\n"));
        assert!(out.trim_end().ends_with("line 5000"));
        assert!(out.contains("[Output too long: showing lines 1-"), "{out}");
        assert!(out.contains("of 5001; full output (") && out.contains("saved to /tmp/out.txt"));
        assert!(out.len() < HEAD + TAIL + 300);
        // An error at the very end survives.
        let failing = format!("{text}Error: build failed");
        assert!(shorten(&failing, Err("disk full".into())).ends_with("Error: build failed"));
    }

    #[test]
    fn one_huge_line_shows_its_start_and_end() {
        let text = format!("é{}END", "x".repeat(100_000));
        let out = shorten(&text, Ok(PathBuf::from("/tmp/o")));
        assert!(out.starts_with('é') && out.ends_with("END"));
        assert!(out.contains("showing the start and end of its one line"));
        assert!(out.len() < HEAD + TAIL + 300);
    }
}
