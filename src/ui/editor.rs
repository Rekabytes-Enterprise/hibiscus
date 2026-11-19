//! Shared bounded text editing and terminal-width layout.
use std::ops::Range;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
const LIMIT: usize = 65536;

#[derive(Default)]
pub(crate) struct Draft {
    pub(crate) text: String,
    pub(crate) cursor: usize,
    preferred_column: Option<usize>,
}
impl Draft {
    pub(crate) fn insert(&mut self, text: &str) -> bool {
        if self.text.len() + text.len() > LIMIT {
            return false;
        }
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.preferred_column = None;
        true
    }
    pub(crate) fn left(&mut self) {
        if let Some((at, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = at;
        }
        self.preferred_column = None;
    }
    pub(crate) fn right(&mut self) {
        if let Some(ch) = self.text[self.cursor..].chars().next() {
            self.cursor += ch.len_utf8();
        }
        self.preferred_column = None;
    }
    pub(crate) fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.replace_range(self.cursor..end, "");
    }
    pub(crate) fn delete(&mut self) {
        if let Some(ch) = self.text[self.cursor..].chars().next() {
            self.text.drain(self.cursor..self.cursor + ch.len_utf8());
        }
        self.preferred_column = None;
    }
    pub(crate) fn home(&mut self) {
        self.cursor = self.text[..self.cursor].rfind('\n').map_or(0, |at| at + 1);
        self.preferred_column = None;
    }
    pub(crate) fn end(&mut self) {
        self.cursor += self.text[self.cursor..]
            .find('\n')
            .unwrap_or(self.text.len() - self.cursor);
        self.preferred_column = None;
    }
    pub(crate) fn vertical(&mut self, direction: isize, columns: usize) {
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

pub(crate) struct Layout {
    pub(crate) rows: Vec<Range<usize>>,
}
impl Layout {
    pub(crate) fn new(text: &str, columns: usize) -> Self {
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
    pub(crate) fn cursor(&self, text: &str, cursor: usize) -> (usize, usize) {
        let row = self
            .rows
            .iter()
            .rposition(|r| r.start <= cursor)
            .unwrap_or(0);
        let range = &self.rows[row];
        (row, 2 + text[range.start..cursor.min(range.end)].width())
    }
}
