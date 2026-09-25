//! Row-diff terminal output. Build the view separately, then commit the minimum
//! changes in one synchronized update with the cursor parked at the composer.
use std::{
    fmt::Write as _,
    io::{self, Write},
};

pub(crate) const SYNC_END: &str = "\x1b[?2026l";
const SYNC_BEGIN: &str = "\x1b[?2026h";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cursor {
    pub(crate) row: usize,
    pub(crate) column: usize,
    pub(crate) visible: bool,
}

pub(crate) struct Painter {
    lines: Vec<String>,
    dimensions: Option<(usize, usize)>,
    cursor: Option<Cursor>,
    synchronized: bool,
}

impl Painter {
    pub(crate) fn new(synchronized: bool) -> Self {
        Self {
            lines: Vec::new(),
            dimensions: None,
            cursor: None,
            synchronized,
        }
    }
    pub(crate) fn invalidate(&mut self) {
        self.lines.clear();
        self.dimensions = None;
        self.cursor = None;
    }
    pub(crate) fn resized(&self, dimensions: (usize, usize)) -> bool {
        self.dimensions != Some(dimensions)
    }
    pub(crate) fn paint(
        &mut self,
        out: &mut impl Write,
        lines: Vec<String>,
        dimensions: (usize, usize),
        cursor: Cursor,
    ) -> io::Result<()> {
        let full = self.resized(dimensions);
        let changed: Vec<_> = lines
            .iter()
            .enumerate()
            .filter(|(index, line)| full || self.lines.get(*index) != Some(*line))
            .collect();
        if changed.is_empty() && self.cursor == Some(cursor) {
            return Ok(());
        }
        let mut frame = String::new();
        if self.synchronized {
            frame.push_str(SYNC_BEGIN);
        }
        // Visibility changes only for actual focus/viewport transitions, never
        // just because another text delta or spinner frame arrived.
        if !cursor.visible && self.cursor.is_none_or(|old| old.visible) {
            frame.push_str("\x1b[?25l");
        }
        for (index, line) in &changed {
            let _ = write!(frame, "\x1b[{};1H{line}", index + 1);
        }
        if !changed.is_empty() || self.cursor != Some(cursor) {
            let _ = write!(frame, "\x1b[{};{}H", cursor.row, cursor.column);
        }
        if cursor.visible && self.cursor.is_none_or(|old| !old.visible) {
            frame.push_str("\x1b[?25h");
        }
        if self.synchronized {
            frame.push_str(SYNC_END);
        }
        if let Err(error) = out.write_all(frame.as_bytes()).and_then(|_| out.flush()) {
            // Release a partially written synchronized frame even on failure.
            if self.synchronized {
                let _ = out.write_all(SYNC_END.as_bytes());
                let _ = out.flush();
            }
            self.invalidate();
            return Err(error);
        }
        self.lines = lines;
        self.dimensions = Some(dimensions);
        self.cursor = Some(cursor);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view(reply: &str, footer: &str) -> Vec<String> {
        ["header", reply, "composer", footer, ""]
            .map(|line| format!("{line}\x1b[K"))
            .to_vec()
    }
    const CURSOR: Cursor = Cursor {
        row: 3,
        column: 7,
        visible: true,
    };
    #[test]
    fn streaming_only_repaints_changed_rows_and_parks_visible_cursor() {
        let mut painter = Painter::new(true);
        let mut bytes = Vec::new();
        painter
            .paint(&mut bytes, view("a", "Working"), (80, 5), CURSOR)
            .unwrap();
        bytes.clear();
        painter
            .paint(&mut bytes, view("ab", "Working"), (80, 5), CURSOR)
            .unwrap();
        let frame = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(frame, "\x1b[?2026h\x1b[2;1Hab\x1b[K\x1b[3;7H\x1b[?2026l");
        assert!(!frame.contains("composer") && !frame.contains("?25"));
        bytes.clear();
        painter
            .paint(&mut bytes, view("ab", "Working"), (80, 5), CURSOR)
            .unwrap();
        assert!(bytes.is_empty());
        painter
            .paint(&mut bytes, view("ab", "Thinking"), (80, 5), CURSOR)
            .unwrap();
        assert!(!String::from_utf8(bytes).unwrap().contains("ab"));
    }
    #[test]
    fn resize_invalidation_and_focus_transitions_restore_the_view() {
        let mut painter = Painter::new(false);
        let mut bytes = Vec::new();
        painter
            .paint(&mut bytes, view("long reply", "Working"), (80, 5), CURSOR)
            .unwrap();
        bytes.clear();
        painter
            .paint(&mut bytes, view("short", "Idle"), (40, 5), CURSOR)
            .unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("composer"));
        assert!(!String::from_utf8_lossy(&bytes).contains("2026"));
        painter.invalidate();
        bytes.clear();
        painter
            .paint(
                &mut bytes,
                view("short", "Idle"),
                (40, 5),
                Cursor {
                    visible: false,
                    ..CURSOR
                },
            )
            .unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("?25l"));
        bytes.clear();
        painter
            .paint(&mut bytes, view("short", "Idle"), (40, 5), CURSOR)
            .unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "\x1b[3;7H\x1b[?25h");
    }
    #[test]
    fn failed_output_releases_sync_and_does_not_cache_a_partial_frame() {
        struct FailsOnce {
            failed: bool,
            bytes: Vec<u8>,
        }
        impl Write for FailsOnce {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                if !self.failed {
                    self.failed = true;
                    return Err(io::Error::other("fixture"));
                }
                self.bytes.extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut out = FailsOnce {
            failed: false,
            bytes: Vec::new(),
        };
        let mut painter = Painter::new(true);
        assert!(painter
            .paint(&mut out, view("a", "Idle"), (80, 5), CURSOR)
            .is_err());
        assert_eq!(out.bytes, SYNC_END.as_bytes());
        assert!(painter.resized((80, 5)));
    }
}
