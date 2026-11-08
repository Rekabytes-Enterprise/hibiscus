//! Fullscreen composer: draft editing, cursor movement, paste, suggestions and text selection.
use super::*;
impl Screen {
    pub(crate) fn take_images(&mut self) -> Vec<SharedImage> {
        self.clipboard_notice.clear();
        std::mem::take(&mut self.images)
    }

    pub(super) fn paste_image(&mut self) -> io::Result<()> {
        if self.clipboard.is_some() {
            return Ok(());
        }
        if self.images.len() >= 4 {
            self.clipboard_notice = "Maximum 4 images · Ctrl+X remove".into();
        } else {
            self.clipboard = Some(super::super::clipboard::Job::start());
            self.clipboard_notice = "Reading clipboard…".into();
        }
        self.render()
    }

    pub(super) fn poll_clipboard(&mut self) -> io::Result<()> {
        let Some(receiver) = &self.clipboard else {
            return Ok(());
        };
        let result = match receiver.receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(()),
            Err(_) => Err("Clipboard helper stopped".into()),
        };
        self.clipboard = None;
        match result {
            Ok(image) => {
                self.images.push(image);
                self.clipboard_notice.clear();
                self.suggestions = None;
            }
            Err(message) => self.clipboard_notice = message,
        }
        self.render()
    }

    pub(super) fn clear_images(&mut self) {
        self.images.clear();
        self.clipboard = None;
        self.clipboard_notice.clear();
    }

    pub(super) fn clear_file_suggestions(&mut self) {
        self.file_search = None;
        self.file_prefix = None;
        self.file_matches.clear();
    }

    fn command_matches(&self) -> Vec<commands::Command<'_>> {
        commands::matches(&self.draft, &self.extension_commands)
            .into_iter()
            .filter(|cmd| (!self.has_credentials || cmd.name != "/login") && cmd.name != "/tui")
            .collect()
    }
    pub(super) fn refresh_suggestions(&mut self) {
        if let Some(prefix) = file_mentions::prefix(&self.draft, self.draft_cursor) {
            if self.suggestions_dismissed {
                self.file_search = None;
                self.suggestions = None;
                self.file_matches.clear();
                return;
            }
            if self.file_prefix.as_ref() != Some(&prefix) {
                self.file_search = Some(file_mentions::Search::start(
                    env::current_dir().unwrap_or_else(|_| ".".into()),
                    prefix.query.clone(),
                ));
                self.file_matches.clear();
                self.suggestions = None;
            }
            self.file_prefix = Some(prefix);
            return;
        }
        self.clear_file_suggestions();
        let options =
            if self.activity.is_some() || self.draft.contains('\n') || !self.images.is_empty() {
                Vec::new()
            } else {
                self.command_matches()
            };
        self.suggestions = if self.suggestions_dismissed || options.is_empty() {
            None
        } else {
            let items = options
                .iter()
                .map(|cmd| format!("{:<12}  {}", cmd.name, cmd.description))
                .collect();
            let rows = self.selectable_rows.len().saturating_sub(3).clamp(1, 5);
            Some(Picker::new("Commands".into(), items, None, rows))
        };
    }

    pub(super) fn poll_file_suggestions(&mut self) -> io::Result<()> {
        let Some(entries) = self
            .file_search
            .as_ref()
            .and_then(file_mentions::Search::poll)
        else {
            return Ok(());
        };
        self.file_search = None;
        if self.suggestions_dismissed || self.file_prefix.is_none() {
            return Ok(());
        }
        self.file_matches = entries;
        self.suggestions = if self.file_matches.is_empty() {
            None
        } else {
            let items = self
                .file_matches
                .iter()
                .map(file_mentions::Entry::display)
                .collect();
            let rows = self.selectable_rows.len().saturating_sub(3).clamp(1, 5);
            Some(Picker::new("Files".into(), items, None, rows))
        };
        self.render()
    }

    /// A completion edits only the @token before the cursor, never the rest of
    /// the draft. This does not read the selected file or change RPC payloads.
    pub(super) fn complete_file(&mut self) -> io::Result<bool> {
        let Some((prefix, entry)) = self.file_prefix.as_ref().zip(
            self.suggestions
                .as_ref()
                .and_then(|picker| self.file_matches.get(picker.selected)),
        ) else {
            return Ok(false);
        };
        let (mut replacement, mut cursor) = entry.replacement(prefix.quoted);
        if replacement.ends_with(' ')
            && self.draft[self.draft_cursor..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            replacement.pop();
            cursor -= 1;
        }
        let start = prefix.start;
        let mut end = self.draft_cursor;
        if prefix.quoted && self.draft[end..].starts_with('"') {
            end += 1;
        }
        self.draft.replace_range(start..end, &replacement);
        self.draft_cursor = start + cursor;
        self.suggestions = None;
        self.file_search = None;
        self.file_prefix = None;
        self.file_matches.clear();
        self.suggestions_dismissed = false;
        self.ensure_cursor_visible();
        self.refresh_suggestions();
        self.render()?;
        Ok(true)
    }

    // Only batch already available printable bytes. Control/escape sequences
    // stay ordered and are handled by the existing input state machine.
    pub(super) fn insert_input_batch(
        &mut self,
        first: u8,
        events: &Receiver<u8>,
        pending: &mut Vec<u8>,
    ) {
        let mut text = String::new();
        for index in 0..128 {
            let byte = if index == 0 {
                first
            } else {
                let Some(byte) = self
                    .deferred_input
                    .pop_front()
                    .or_else(|| events.try_recv().ok())
                else {
                    break;
                };
                if !matches!(byte, 32..=126 | 128..=255) {
                    self.pending_input = Some(byte);
                    break;
                }
                byte
            };
            pending.push(byte);
            match std::str::from_utf8(pending) {
                Ok(value) => {
                    text.push_str(value);
                    pending.clear();
                }
                Err(error) if error.error_len().is_some() => pending.clear(),
                Err(_) => {}
            }
        }
        if !text.is_empty() {
            self.insert_draft(&text);
        }
    }

    pub(super) fn insert_draft(&mut self, text: &str) {
        self.input_notice = None;
        self.draft.insert_str(self.draft_cursor, text);
        self.draft_cursor += text.len();
        self.preferred_column = None;
        self.ensure_cursor_visible();
    }

    pub(super) fn ensure_cursor_visible(&mut self) {
        let width = content_width(self.size().0).saturating_sub(5).max(1);
        let total = draft_lines(&self.draft, width).len();
        let visible = total.min(5);
        let row = draft_cursor_position(&self.draft, width, self.draft_cursor).0;
        let end = total - self.draft_scroll.min(total - visible);
        let start = end - visible;
        if row < start {
            self.draft_scroll = total - visible - row;
        } else if row >= end {
            self.draft_scroll = total - row - 1;
        }
        self.draft_scroll = self.draft_scroll.min(total - visible);
    }

    pub(super) fn move_draft(&mut self, key: Navigation) -> io::Result<()> {
        let width = content_width(self.size().0).saturating_sub(5).max(1);
        match key {
            Navigation::Left => {
                if let Some(ch) = self.draft[..self.draft_cursor].chars().next_back() {
                    self.draft_cursor -= ch.len_utf8();
                }
            }
            Navigation::Right => {
                if let Some(ch) = self.draft[self.draft_cursor..].chars().next() {
                    self.draft_cursor += ch.len_utf8();
                }
            }
            Navigation::Home => {
                self.draft_cursor = self.draft[..self.draft_cursor]
                    .rfind('\n')
                    .map_or(0, |i| i + 1);
            }
            Navigation::End => {
                self.draft_cursor += self.draft[self.draft_cursor..]
                    .find('\n')
                    .unwrap_or(self.draft.len() - self.draft_cursor);
            }
            Navigation::Delete => {
                if let Some(ch) = self.draft[self.draft_cursor..].chars().next() {
                    self.draft
                        .drain(self.draft_cursor..self.draft_cursor + ch.len_utf8());
                }
            }
            Navigation::Up | Navigation::Down => {
                let (row, col) = draft_cursor_position(&self.draft, width, self.draft_cursor);
                let target = if key == Navigation::Up {
                    row.checked_sub(1)
                } else {
                    Some(row + 1)
                };
                if let Some(target) = target {
                    let column = *self.preferred_column.get_or_insert(col);
                    if let Some(index) = draft_index_on_row(&self.draft, width, target, column) {
                        self.draft_cursor = index;
                    }
                }
            }
            _ => {}
        }
        if !matches!(key, Navigation::Up | Navigation::Down) {
            self.preferred_column = None;
        }
        if self.file_prefix.is_some() {
            self.suggestions_dismissed = false;
            self.refresh_suggestions();
        } else if key == Navigation::Delete || self.suggestions.is_some() {
            self.suggestions_dismissed = true;
            self.refresh_suggestions();
        }
        self.ensure_cursor_visible();
        self.render()
    }

    pub(super) fn backspace_draft(&mut self) -> io::Result<()> {
        self.active_utf8.clear();
        if let Some(ch) = self.draft[..self.draft_cursor].chars().next_back() {
            let start = self.draft_cursor - ch.len_utf8();
            self.draft.drain(start..self.draft_cursor);
            self.draft_cursor = start;
        }
        self.preferred_column = None;
        self.suggestions_dismissed = false;
        self.refresh_suggestions();
        self.ensure_cursor_visible();
        self.render()
    }

    pub(super) fn send_active_draft(&mut self, mode: QueueMode) -> io::Result<ActiveAction> {
        if self.complete_file()? {
            return Ok(ActiveAction::None);
        }
        if self.manual_compaction {
            self.input_notice =
                Some("Compacting… draft kept · Enter after Runtime finishes · Esc cancel".into());
            self.render()?;
            return Ok(ActiveAction::None);
        }
        if self.clipboard.is_some() {
            self.input_notice = Some("Reading clipboard… press Enter when ready".into());
        } else if self.draft.trim_start().starts_with('/') {
            self.input_notice = Some(
                "Commands are available after Runtime settles; keep this draft or clear it".into(),
            );
        } else if !self.draft.trim().is_empty() || !self.images.is_empty() {
            let submission = DraftSubmission {
                text: self.draft.trim().to_owned(),
                images: std::mem::take(&mut self.images),
                mode,
            };
            self.draft.clear();
            self.draft_cursor = 0;
            self.draft_scroll = 0;
            self.active_utf8.clear();
            self.suggestions = None;
            self.clear_file_suggestions();
            self.input_notice = Some(
                match mode {
                    QueueMode::Steer => "Sending steering message…",
                    QueueMode::FollowUp => "Queueing follow-up…",
                }
                .into(),
            );
            self.render()?;
            return Ok(ActiveAction::Submit(submission));
        }
        self.render()?;
        Ok(ActiveAction::None)
    }

    pub(super) fn input_navigation(&mut self, key: Navigation) -> io::Result<()> {
        if key == Navigation::PasteStart {
            self.paste = Some(PasteInput::default());
            return Ok(());
        }
        if let Navigation::Mouse(event) = key {
            return self.select_mouse(event);
        }
        if key == Navigation::PasteImage {
            return self.paste_image();
        }
        if key == Navigation::Newline {
            self.insert_draft("\n");
            self.refresh_suggestions();
            return self.render();
        }
        if matches!(
            key,
            Navigation::Left
                | Navigation::Right
                | Navigation::Home
                | Navigation::End
                | Navigation::Delete
        ) {
            return self.move_draft(key);
        }
        let (columns, height) = self.size();
        let layout = draft_lines(&self.draft, content_width(columns).saturating_sub(5).max(1));
        let visible = layout.len().min(5);
        let key = match key {
            Navigation::MouseScroll(lines, row)
                if self.suggestions.is_none()
                    && row >= height.saturating_sub(visible + 2)
                    && row <= height.saturating_sub(3) =>
            {
                self.scroll_draft(lines, layout.len());
                return self.render();
            }
            Navigation::MouseScroll(lines, _) => Navigation::ScrollLines(lines),
            Navigation::Up | Navigation::Down if self.suggestions.is_none() => {
                return self.move_draft(key);
            }
            key => key,
        };
        let rows = self.selectable_rows.len().saturating_sub(3).clamp(1, 5);
        if let Some(picker) = self.suggestions.as_mut() {
            match key {
                Navigation::Escape => {
                    self.suggestions_dismissed = true;
                    self.suggestions = None;
                }
                Navigation::Up | Navigation::ScrollLines(3) => picker.move_by(-1, rows),
                Navigation::Down | Navigation::ScrollLines(-3) => picker.move_by(1, rows),
                Navigation::ScrollPage(dir) => picker.page(-(dir as isize), rows),
                Navigation::Home => picker.move_by(-(picker.items.len() as isize), rows),
                Navigation::End => picker.move_by(picker.items.len() as isize, rows),
                _ => {}
            }
            self.render()
        } else {
            self.scroll(key)
        }
    }

    pub(super) fn select_mouse(&mut self, event: MouseEvent) -> io::Result<()> {
        if !self.full || self.suspended {
            return Ok(());
        }
        // The transcript begins below the rendered header. A press elsewhere
        // must not select a dialog, password or unsent composer text.
        let row = event
            .row
            .saturating_sub(self.transcript_top)
            .min(self.selectable_rows.len().saturating_sub(1));
        let (columns, _) = self.size();
        let left = (columns.saturating_sub(content_width(columns))) / 2;
        let prefix = left + transcript_prefix(columns);
        let column = event.column.saturating_sub(prefix);
        let point = (row, column + prefix);
        if event.release {
            if let Some(selection) = self.selection.as_mut() {
                if selection.dragging {
                    selection.focus = point;
                    selection.dragging = false;
                    let text = self.selected_text();
                    if !text.is_empty() {
                        super::super::text_clipboard::copy(&mut self.out, &text)?;
                    }
                    self.render()?;
                }
            }
        } else if event.button & 32 != 0 {
            if let Some(selection) = self.selection.as_mut() {
                if selection.dragging {
                    selection.focus = point;
                    self.render()?;
                }
            }
        } else if event.button & 3 == 0 {
            self.selection = None;
            if event.row >= self.transcript_top
                && event.row < self.transcript_top + self.selectable_rows.len()
                && row < self.selectable_rows.len()
                && !self.selectable_rows[row].is_empty()
            {
                self.selection = Some(TextSelection {
                    anchor: point,
                    focus: point,
                    dragging: true,
                });
            }
            self.render()?;
        }
        Ok(())
    }

    pub(super) fn feed_paste(&mut self, byte: u8) -> io::Result<bool> {
        let Some(paste) = &mut self.paste else {
            return Ok(false);
        };
        if let Some(done) = paste.push(byte) {
            self.paste = None;
            match done {
                Some(text) if self.draft.len() + text.len() <= 8192 => {
                    self.insert_draft(&text);
                    self.refresh_suggestions();
                }
                Some(_) | None => self.input_notice = Some("Paste too large or invalid".into()),
            }
            self.render()?;
        }
        Ok(true)
    }

    pub(super) fn idle_ctrl_c(&mut self, pending: &mut Vec<u8>) -> io::Result<bool> {
        if !self.full {
            if self.draft.is_empty() && self.images.is_empty() && self.clipboard.is_none() {
                return Ok(true);
            }
            self.draft.clear();
            self.draft_cursor = 0;
            self.preferred_column = None;
            self.clear_images();
            pending.clear();
            return Ok(false);
        }
        let now = Instant::now();
        if self
            .exit_armed
            .is_some_and(|armed| now.duration_since(armed) < Duration::from_secs(2))
        {
            self.exit_armed = None;
            return Ok(true);
        }
        let text = self.selected_text();
        if !text.is_empty() {
            super::super::text_clipboard::copy(&mut self.out, &text)?;
            self.selection = None;
        } else {
            self.draft.clear();
            self.draft_cursor = 0;
            self.preferred_column = None;
            self.clear_images();
            pending.clear();
            self.suggestions_dismissed = false;
            self.refresh_suggestions();
        }
        self.exit_armed = Some(now);
        self.input_notice = Some("Press Ctrl+C again within 2s to quit".into());
        self.render()?;
        Ok(false)
    }

    pub(super) fn selected_text(&self) -> String {
        let Some(selection) = &self.selection else {
            return String::new();
        };
        let (columns, _) = self.size();
        let prefix =
            (columns.saturating_sub(content_width(columns))) / 2 + transcript_prefix(columns);
        let (start, end) = selection.bounds();
        if start == end {
            return String::new();
        }
        (start.0..=end.0)
            .filter_map(|row| {
                self.selectable_rows.get(row).map(|line| {
                    let from = if row == start.0 {
                        start.1.saturating_sub(prefix)
                    } else {
                        0
                    };
                    let to = if row == end.0 {
                        end.1.saturating_sub(prefix)
                    } else {
                        line.chars().count()
                    };
                    line.chars()
                        .skip(from)
                        .take(to.saturating_sub(from))
                        .collect::<String>()
                        .trim_end()
                        .to_owned()
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(super) fn scroll_draft(&mut self, lines: i32, total: usize) {
        self.draft_scroll = if lines > 0 {
            self.draft_scroll.saturating_add(lines as usize)
        } else {
            self.draft_scroll
                .saturating_sub(lines.unsigned_abs() as usize)
        }
        .min(total.saturating_sub(5));
    }
}
