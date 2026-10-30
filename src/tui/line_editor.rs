//! CLI draft editing. Repaint only the bounded input region, never chat history.
use super::{
    inline,
    paste::Input as Paste,
    screen::{escape_key, Navigation},
    terminal::RawMode,
};
use crate::Result;
use std::{ops::Range, sync::mpsc::Receiver};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const LIMIT: usize = 65536;
const ENABLE_PASTE: &str = "\x1b[?2004h";
const DISABLE_PASTE: &str = "\x1b[?2004l";

#[derive(Default)]
struct Draft {
    text: String,
    cursor: usize,
    preferred_column: Option<usize>,
}
impl Draft {
    fn insert(&mut self, text: &str) -> bool {
        if self.text.len() + text.len() > LIMIT {
            return false;
        }
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.preferred_column = None;
        true
    }
    fn left(&mut self) {
        if let Some((at, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = at;
        }
        self.preferred_column = None;
    }
    fn right(&mut self) {
        if let Some(ch) = self.text[self.cursor..].chars().next() {
            self.cursor += ch.len_utf8();
        }
        self.preferred_column = None;
    }
    fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.replace_range(self.cursor..end, "");
    }
    fn delete(&mut self) {
        if let Some(ch) = self.text[self.cursor..].chars().next() {
            self.text.drain(self.cursor..self.cursor + ch.len_utf8());
        }
        self.preferred_column = None;
    }
    fn home(&mut self) {
        self.cursor = self.text[..self.cursor].rfind('\n').map_or(0, |at| at + 1);
        self.preferred_column = None;
    }
    fn end(&mut self) {
        self.cursor += self.text[self.cursor..]
            .find('\n')
            .unwrap_or(self.text.len() - self.cursor);
        self.preferred_column = None;
    }
    fn vertical(&mut self, direction: isize, columns: usize) {
        let layout = Layout::new(&self.text, columns);
        let (row, col) = layout.cursor(&self.text, self.cursor);
        let desired = *self.preferred_column.get_or_insert(col.saturating_sub(2));
        let target = row
            .saturating_add_signed(direction)
            .min(layout.rows.len() - 1);
        let range = &layout.rows[target];
        let mut used = 0;
        self.cursor = range.start;
        for (at, ch) in self.text[range.clone()].char_indices() {
            let width = ch.width().unwrap_or(0);
            if used + width > desired {
                break;
            }
            used += width;
            self.cursor = range.start + at + ch.len_utf8();
        }
    }
}

struct Layout {
    rows: Vec<Range<usize>>,
}
impl Layout {
    fn new(text: &str, columns: usize) -> Self {
        let available = columns.saturating_sub(3).max(2);
        let mut rows = Vec::new();
        let (mut start, mut used) = (0, 0);
        for (at, ch) in text.char_indices() {
            if ch == '\n' {
                rows.push(start..at);
                start = at + 1;
                used = 0;
                continue;
            }
            let width = ch.width().unwrap_or(0);
            if used + width > available {
                rows.push(start..at);
                start = at;
                used = 0;
            }
            used += width;
        }
        rows.push(start..text.len());
        Self { rows }
    }
    fn cursor(&self, text: &str, cursor: usize) -> (usize, usize) {
        let row = self
            .rows
            .iter()
            .rposition(|r| r.start <= cursor)
            .unwrap_or(0);
        let range = &self.rows[row];
        (row, 2 + text[range.start..cursor.min(range.end)].width())
    }
}

struct Frame {
    rows: Vec<String>,
    cursor: (usize, usize),
    footer: bool,
}
impl Frame {
    fn new(draft: &Draft, size: (usize, usize), notice: Option<&str>) -> Self {
        let layout = Layout::new(&draft.text, size.0);
        let cursor = layout.cursor(&draft.text, draft.cursor);
        let limit = size.1.saturating_sub(3).clamp(1, 6);
        let start = cursor.0.saturating_sub(limit - 1);
        let end = (start + limit).min(layout.rows.len());
        let mut rows = layout.rows[start..end]
            .iter()
            .enumerate()
            .map(|(i, range)| {
                format!(
                    "{}{}",
                    if start + i == 0 { "❯ " } else { "  " },
                    &draft.text[range.clone()]
                )
            })
            .collect::<Vec<_>>();
        let footer = notice.is_some() || start > 0 || end < layout.rows.len();
        if footer {
            let label = notice.map(str::to_owned).unwrap_or_else(|| {
                format!(
                    "  {start} rows above · {} below · Enter send · Alt+Enter newline",
                    layout.rows.len() - end
                )
            });
            let mut used = 0;
            rows.push(
                label
                    .chars()
                    .take_while(|ch| {
                        used += ch.width().unwrap_or(0);
                        used < size.0
                    })
                    .collect(),
            );
        }
        Self {
            rows,
            cursor: (cursor.0 - start, cursor.1),
            footer,
        }
    }
    fn initial() -> Self {
        Self {
            rows: vec!["❯ ".into()],
            cursor: (0, 2),
            footer: false,
        }
    }
}

fn row(text: &str, color: bool, muted: bool) -> String {
    if !color {
        return text.to_owned();
    }
    if muted {
        return format!("\x1b[38;2;155;155;155m{text}{}", inline::BASE);
    }
    match text.strip_prefix("❯ ") {
        Some(rest) => format!("\x1b[1;38;2;255;155;187m❯\x1b[0m{} {rest}", inline::BASE),
        None => text.to_owned(),
    }
}
fn repaint(raw: &mut RawMode, previous: &Frame, next: &Frame, color: bool) -> Result<()> {
    let mut out = String::from("\r");
    if previous.cursor.0 > 0 {
        out.push_str(&format!("\x1b[{}A", previous.cursor.0));
    }
    let height = previous.rows.len().max(next.rows.len());
    for at in 0..height {
        out.push_str("\x1b[2K");
        if let Some(text) = next.rows.get(at) {
            out.push_str(&row(text, color, next.footer && at + 1 == next.rows.len()));
        }
        if at + 1 < height {
            out.push_str("\r\n");
        }
    }
    let up = height - 1 - next.cursor.0;
    out.push('\r');
    if up > 0 {
        out.push_str(&format!("\x1b[{up}A"));
    }
    if next.cursor.1 > 0 {
        out.push_str(&format!("\x1b[{}C", next.cursor.1));
    }
    raw.write(&out)?;
    Ok(())
}
fn commit(
    raw: &mut RawMode,
    frame: &Frame,
    draft: &Draft,
    columns: usize,
    color: bool,
) -> Result<()> {
    let simple = frame.rows.len() == 1 && draft.cursor == draft.text.len();
    if simple {
        raw.write("\r\n")?;
        return Ok(());
    }
    let mut out = String::from("\r");
    if frame.cursor.0 > 0 {
        out.push_str(&format!("\x1b[{}A", frame.cursor.0));
    }
    out.push_str("\x1b[J");
    for (i, range) in Layout::new(&draft.text, columns).rows.iter().enumerate() {
        let text = format!(
            "{}{}",
            if i == 0 { "❯ " } else { "  " },
            &draft.text[range.clone()]
        );
        out.push_str(&row(&text, color, false));
        out.push_str("\r\n");
    }
    raw.write(&out)?;
    Ok(())
}

/// The prompt has already been printed by Ui. One terminal reader remains in
/// charge; this editor never reads stdin or starts an input/animation thread.
pub(crate) fn read(
    events: &Receiver<u8>,
    raw: &mut RawMode,
    color: bool,
) -> Result<Option<String>> {
    raw.write(ENABLE_PASTE)?;
    let result = read_draft(events, raw, color);
    let cleanup = raw.write(DISABLE_PASTE);
    match result {
        Err(error) => Err(error),
        Ok(value) => {
            cleanup?;
            Ok(value)
        }
    }
}
fn read_draft(events: &Receiver<u8>, raw: &mut RawMode, color: bool) -> Result<Option<String>> {
    let mut draft = Draft::default();
    let mut frame = Frame::initial();
    let mut pending = Vec::new();
    let mut paste: Option<Paste> = None;
    let mut notice = None;
    let mut size = raw.size();
    while let Ok(byte) = events.recv() {
        if let Some(input) = paste.as_mut() {
            if let Some(result) = input.push(byte) {
                paste = None;
                match result {
                    Some(text) if draft.insert(&text) => {
                        notice = None;
                    }
                    _ => notice = Some("  Paste rejected: invalid UTF-8 or draft exceeds 64 KiB."),
                }
                let next = Frame::new(&draft, size, notice);
                repaint(raw, &frame, &next, color)?;
                frame = next;
            }
            continue;
        }
        let before = draft.cursor;
        let old_len = draft.text.len();
        let had_notice = notice.take().is_some();
        let mut appended = None;
        let new_size = raw.size();
        if new_size != size {
            // Terminal reflow rules vary. Start a fresh owned region rather
            // than erase rows using coordinates from the previous width.
            raw.write("\r\n\x1b[J")?;
            frame = Frame::initial();
            size = new_size;
            let next = Frame::new(&draft, size, notice);
            repaint(raw, &frame, &next, color)?;
            frame = next;
        }
        match byte {
            b'\r' => {
                commit(raw, &frame, &draft, size.0, color)?;
                return Ok(Some(draft.text));
            }
            b'\n' => {
                if !draft.insert("\n") {
                    notice = Some("  Draft limit is 64 KiB; text was not inserted.");
                }
            }
            3 => {
                if draft.text.is_empty() {
                    return Ok(None);
                }
                draft = Draft::default();
                pending.clear();
            }
            4 if draft.text.is_empty() => return Ok(None),
            4 => draft.delete(),
            8 | 127 => {
                pending.clear();
                draft.backspace();
            }
            1 => draft.home(),
            5 => draft.end(),
            b'\t' => {
                if !draft.insert("    ") {
                    notice = Some("  Draft limit is 64 KiB; text was not inserted.");
                }
            }
            27 => {
                pending.clear();
                match escape_key(events) {
                    Navigation::Newline | Navigation::FollowUp => {
                        if !draft.insert("\n") {
                            notice = Some("  Draft limit is 64 KiB; text was not inserted.");
                        }
                    }
                    Navigation::Left => draft.left(),
                    Navigation::Right => draft.right(),
                    Navigation::Up => draft.vertical(-1, size.0),
                    Navigation::Down => draft.vertical(1, size.0),
                    Navigation::Home => draft.home(),
                    Navigation::End => draft.end(),
                    Navigation::Delete => draft.delete(),
                    Navigation::PasteStart => {
                        paste = Some(Paste::with_limit(LIMIT));
                        continue;
                    }
                    _ => {}
                }
            }
            32..=126 | 128..=255 => {
                pending.push(byte);
                match std::str::from_utf8(&pending) {
                    Ok(text) => {
                        if draft.insert(text) {
                            appended = Some(text.to_owned());
                        } else {
                            notice = Some("  Draft limit is 64 KiB; text was not inserted.");
                        }
                        pending.clear();
                    }
                    Err(error) if error.error_len().is_none() => continue,
                    Err(_) => {
                        pending.clear();
                        continue;
                    }
                }
            }
            _ => {}
        }
        let next = Frame::new(&draft, size, notice);
        if let Some(text) = appended.filter(|_| {
            before == old_len && !had_notice && frame.rows.len() == 1 && next.rows.len() == 1
        }) {
            raw.write(&text)?;
        } else if before != draft.cursor
            || old_len != draft.text.len()
            || had_notice
            || notice.is_some()
            || frame.rows != next.rows
        {
            repaint(raw, &frame, &next, color)?;
        }
        frame = next;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_stay_on_utf8_boundaries_and_join_lines() {
        let mut draft = Draft::default();
        assert!(draft.insert("café\n世界"));
        draft.left();
        draft.backspace();
        assert_eq!(draft.text, "café\n界");
        draft.home();
        draft.backspace();
        assert_eq!(draft.text, "café界");
        draft.delete();
        assert_eq!(draft.text, "café");
        draft.home();
        draft.right();
        draft.insert("X");
        assert_eq!(draft.text, "cXafé");
        draft.end();
        assert_eq!(draft.cursor, draft.text.len());
    }
    #[test]
    fn vertical_motion_keeps_a_preferred_column_and_large_drafts_have_a_bounded_view() {
        let mut draft = Draft::default();
        draft.insert("abcde\nx\nabcde");
        draft.vertical(-1, 80);
        assert_eq!(draft.cursor, 7);
        draft.vertical(-1, 80);
        assert_eq!(draft.cursor, 5);
        draft.vertical(1, 80);
        draft.vertical(1, 80);
        assert_eq!(draft.cursor, draft.text.len());
        draft = Draft::default();
        draft.insert(&"世界 text\n".repeat(100));
        let frame = Frame::new(&draft, (20, 14), None);
        assert!(frame.rows.len() <= 7 && frame.footer);
        assert!(frame.rows.iter().all(|line| line.width() < 20));
        let layout = Layout::new(&draft.text, 20);
        for at in draft
            .text
            .char_indices()
            .map(|(at, _)| at)
            .chain([draft.text.len()])
        {
            let (row, col) = layout.cursor(&draft.text, at);
            assert!(row < layout.rows.len() && col < 20);
        }
    }
    #[test]
    fn draft_limit_rejects_a_whole_insertion_without_mutating_it() {
        let mut draft = Draft::default();
        draft.insert("keep");
        assert!(!draft.insert(&"x".repeat(LIMIT)));
        assert_eq!((draft.text.as_str(), draft.cursor), ("keep", 4));
        let mut input = Paste::with_limit(8);
        for byte in b"123456789\x1b[201~" {
            if let Some(result) = input.push(*byte) {
                assert!(result.is_none());
            }
        }
    }
}
