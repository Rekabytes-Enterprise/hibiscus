//! Fullscreen layout and painting.
use super::*;
impl Screen {
    pub(super) fn size(&self) -> (usize, usize) {
        crate::ui::terminal::size()
    }

    pub(super) fn accent(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    pub(super) fn status_style(&self) -> &'static str {
        if self.activity.is_some() {
            "1;38;2;236;74;125"
        } else {
            "2"
        }
    }

    #[cfg(test)]
    pub(super) fn styled_line(&self, row: &markdown::Row, width: usize) -> String {
        self.styled_line_selected(row, width, None)
    }

    pub(super) fn styled_line_selected(
        &self,
        row: &markdown::Row,
        width: usize,
        selected: Option<(usize, usize)>,
    ) -> String {
        let mut out = String::new();
        let mut previous = Tone::Plain;
        for (index, cell) in row.cells.iter().enumerate() {
            if self.color && cell.tone != previous {
                out.push_str("\x1b[0m");
                let code = match cell.tone {
                    Tone::Plain => "",
                    Tone::Strong => "1;38;2;255;220;230",
                    Tone::Emphasis => "3;38;2;255;183;206",
                    Tone::Code => "38;2;255;173;197",
                    Tone::Heading => "1;38;2;236;74;125",
                    Tone::Link => "4;38;2;236;74;125",
                    Tone::Muted => "2",
                    Tone::TableBorder => "38;2;184;57;101",
                    Tone::Activity => "1;38;2;236;74;125",
                    Tone::Success => "38;2;169;224;184",
                    Tone::Error => "38;2;255;120;149",
                    Tone::DiffAdded => "38;2;169;224;184;48;2;36;55;44",
                    Tone::DiffRemoved => "38;2;255;134;153;48;2;65;35;49",
                    Tone::DiffContext => "38;2;180;172;183;48;2;41;35;46",
                };
                if !code.is_empty() {
                    out.push_str(&format!("\x1b[{code}m"));
                }
                previous = cell.tone;
            }
            let selected = selected.is_some_and(|(start, end)| (start..end).contains(&index));
            if selected {
                out.push_str("\x1b[7m");
            }
            out.push(cell.ch);
            if selected {
                out.push_str("\x1b[27m");
            }
        }
        if self.color && row.role == Role::Diff {
            out.push_str(&" ".repeat(width.saturating_sub(row.cells.len())));
        }
        if self.color && previous != Tone::Plain {
            out.push_str("\x1b[0m");
        }
        let auth_notice = row.role == Role::Note
            && row
                .cells
                .iter()
                .map(|cell| cell.ch)
                .collect::<String>()
                .trim_start()
                .starts_with("· Open sign-in ↗");
        if auth_notice {
            let label = "Open sign-in ↗";
            if let (Some(url), Some(start)) = (self.auth_url.as_deref(), out.find(label)) {
                out.insert_str(start + label.len(), "\x1b]8;;\x1b\\");
                out.insert_str(start, &format!("\x1b]8;;{url}\x1b\\"));
            }
        }
        out
    }

    #[cfg(test)]
    pub(super) fn transcript_line(
        &self,
        row: &markdown::Row,
        width: usize,
        columns: usize,
        left: &str,
    ) -> String {
        self.transcript_line_selected(row, width, columns, left, usize::MAX)
    }

    pub(super) fn transcript_line_selected(
        &self,
        row: &markdown::Row,
        width: usize,
        columns: usize,
        left: &str,
        row_index: usize,
    ) -> String {
        let selected = self.selection.as_ref().and_then(|selection| {
            selection.columns(
                row_index,
                row.cells.len(),
                left.len() + transcript_prefix(columns),
            )
        });
        let marker = match row.cells.first().map(|cell| cell.tone) {
            Some(Tone::DiffAdded) => self.accent("38;2;169;224;184", "┃"),
            Some(Tone::DiffRemoved) => self.accent("38;2;255;134;153", "┃"),
            _ if row.role == Role::User => self.accent("1;38;2;236;74;125", "┃"),
            _ => self.accent("2;38;2;184;57;101", "│"),
        };
        let gap = " ".repeat(transcript_prefix(columns) - 1);
        let text = format!(
            "{left}{marker}{gap}{}",
            self.styled_line_selected(
                row,
                width.saturating_sub(transcript_prefix(columns)).max(1),
                selected
            )
        );
        if self.color && row.role == Role::User {
            // Reapply the tint after foreground/style resets, and paint the
            // margins and unused columns too. Reset before the next row.
            let background = "\x1b[48;2;57;30;46m";
            let text = text.replace("\x1b[0m", &format!("\x1b[0m{background}"));
            let padding =
                columns.saturating_sub(left.len() + transcript_prefix(columns) + row.cells.len());
            format!("{background}{text}{}\x1b[0m", " ".repeat(padding))
        } else {
            text
        }
    }

    pub(super) fn picker_rows(
        &mut self,
        width: usize,
        height: usize,
        left: &str,
    ) -> Option<(usize, Vec<String>)> {
        let rows = if let Some(modal) = &mut self.modal {
            modal.rows(width, height)
        } else {
            let suggestions = self.picker.is_none();
            let picker = self.picker.as_mut().or(self.suggestions.as_mut())?;
            picker.move_by(
                0,
                height.saturating_sub(3 + 3 * usize::from(picker.query.is_some())),
            );
            modal::picker_panel(picker, width, height, suggestions)
        };
        let top = height.saturating_sub(rows.len());
        Some((
            top,
            rows.into_iter()
                .map(|row| format!("{left}{}", self.accent(row.style, &row.text)))
                .collect(),
        ))
    }

    pub(super) fn render_stream(&mut self) -> io::Result<()> {
        self.paint_pending = true;
        if self
            .last_paint
            .is_none_or(|last| last.elapsed() >= Duration::from_millis(33))
        {
            self.render()?;
        }
        Ok(())
    }

    pub(crate) fn render(&mut self) -> io::Result<()> {
        if !self.full || self.suspended {
            return Ok(());
        }
        let (columns, rows) = self.size();
        if self.painter.resized((columns, rows)) {
            self.selection = None;
            if !self.secret_input {
                self.ensure_cursor_visible();
            }
        }
        let width = content_width(columns);
        let left = " ".repeat(columns.saturating_sub(width) / 2);
        let flower = self
            .activity
            .as_ref()
            .map_or(FLOWERS[0], |activity| FLOWERS[activity.flower_frame]);
        let lines = self.transcript_layout(width.saturating_sub(transcript_prefix(columns)).max(1));
        let draft_width = width.saturating_sub(5).max(1);
        let draft_rows = if self.modal.is_some() {
            vec![clip("Respond in the dialog above", draft_width)]
        } else if self.secret_input {
            vec![clip_tail(
                &"•".repeat(self.draft.chars().count()),
                draft_width,
            )]
        } else {
            draft_lines(&self.draft, draft_width)
        };
        let draft_height = draft_rows.len().min(5);
        self.draft_scroll = self
            .draft_scroll
            .min(draft_rows.len().saturating_sub(draft_height));
        let draft_end = draft_rows.len() - self.draft_scroll;
        let draft_start = draft_end - draft_height;
        let pending_height = if self.modal.is_some() {
            0
        } else {
            self.pending_queue.len().min(3) + usize::from(self.pending_queue.len() > 3)
        };
        // Preserve enough space for active dialogs before choosing the header.
        // The bottom owns queue rows, a spacer, composer, status and diagnostics.
        let bottom = 5 + draft_height + pending_height;
        let body_minimum = if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.query.is_some())
        {
            7
        } else if self.modal.is_some() || self.picker.is_some() || self.suggestions.is_some() {
            5
        } else {
            4
        };
        // Keep a one-line HIBI header even when a multiline draft needs
        // most of a short terminal. The transcript gives up one row first.
        let remaining = rows.saturating_sub(bottom);
        let header_budget = remaining
            .saturating_sub(body_minimum)
            .max(usize::from(remaining > 0));
        let header_gap = usize::from(header_budget > 3);
        let info = crate::ui::welcome::Context {
            workspace: &self.workspace,
            model: &self.model,
            mode: Mode::Tui,
            has_credentials: self.has_credentials,
        };
        let header = header::rows(
            width,
            header_budget.saturating_sub(header_gap),
            header::Header {
                info,
                thinking: self.thinking.as_deref(),
                session: &self.session,
            },
        );
        let transcript_top = header.len() + header_gap;
        if self.transcript_top != transcript_top {
            self.selection = None;
        }
        self.transcript_top = transcript_top;
        let available = rows.saturating_sub(bottom + transcript_top);
        let modal = self.picker_rows(width, available, &left);
        let transcript_rows = modal.as_ref().map_or(available, |(top, _)| *top);
        let welcome = if self.transcript.is_empty() && self.activity.is_none() {
            header::tips(
                width,
                transcript_rows,
                &crate::ui::welcome::Context {
                    workspace: &self.workspace,
                    model: &self.model,
                    mode: Mode::Tui,
                    has_credentials: self.has_credentials,
                },
            )
        } else {
            Vec::new()
        };
        let viewport = transcript_viewport(lines.len(), transcript_rows, self.scroll);
        self.scroll = viewport.scroll;
        let start = viewport.start;
        let end = viewport.end;
        // Keep the cursor visible during routine redraws. Hiding it on every
        // animation frame causes a visible blink even with a steady style.
        let (cursor_line, cursor_column) = if self.secret_input {
            (draft_start, draft_rows[0].chars().count())
        } else {
            draft_cursor_position(&self.draft, draft_width, self.draft_cursor)
        };
        let modal_cursor = self
            .modal
            .as_ref()
            .and_then(|dialog| dialog.cursor)
            .zip(modal.as_ref())
            .map(|((row, column), (top, _))| {
                (top + row + transcript_top + 1, left.len() + column + 1)
            });
        let cursor_visible = if self.modal.is_some() {
            modal_cursor.is_some()
        } else {
            self.picker.is_none() && (draft_start..draft_end).contains(&cursor_line)
        };
        let mut frame = String::new();
        for row in &header {
            frame.push_str(&format!("{left}{}\x1b[K\r\n", row.render(self.color)));
        }
        if header_gap > 0 {
            frame.push_str("\x1b[K\r\n");
        }
        self.selectable_rows = vec![String::new(); available];
        for row in 0..available {
            if let Some((top, panel)) = &modal {
                if let Some(line) = row.checked_sub(*top).and_then(|offset| panel.get(offset)) {
                    frame.push_str(&format!("{line}\x1b[K\r\n"));
                    continue;
                }
            }
            if let Some(line) = lines.get(start + row).filter(|_| start + row < end) {
                self.selectable_rows[row] = line.cells.iter().map(|cell| cell.ch).collect();
                frame.push_str(&format!(
                    "{}\x1b[K\r\n",
                    self.transcript_line_selected(line, width, columns, &left, row)
                ));
            } else if let Some(line) = welcome.get(row) {
                frame.push_str(&format!("{left}{}\x1b[K\r\n", line.render(self.color)));
            } else {
                frame.push_str("\x1b[K\r\n");
            }
        }
        for pending in self
            .pending_queue
            .iter()
            .take(if self.modal.is_some() { 0 } else { 3 })
        {
            let mode = match pending.mode {
                QueueMode::Steer => "Steering",
                QueueMode::FollowUp => "Follow-up",
            };
            let state = if pending.sending {
                "Sending"
            } else {
                "Waiting"
            };
            let images = if pending.images == 0 {
                String::new()
            } else {
                format!(" · {} image(s)", pending.images)
            };
            let label = clip(
                &format!("↳ {state} · {mode}: {}{images}", clean(&pending.text)),
                width,
            );
            frame.push_str(&format!(
                "{left}{}\x1b[K\r\n",
                self.accent("2;38;2;255;183;206", &label)
            ));
        }
        if self.modal.is_none() && self.pending_queue.len() > 3 {
            let label = format!("↳ {} more queued", self.pending_queue.len() - 3);
            frame.push_str(&format!("{left}{}\x1b[K\r\n", self.accent("2", &label)));
        }
        frame.push_str("\x1b[K\r\n");
        let border = format!("╭{}╮", "─".repeat(width.saturating_sub(2)));
        let attachment = if !self.clipboard_notice.is_empty() {
            self.clipboard_notice.clone()
        } else if let Some(notice) = &self.input_notice {
            notice.clone()
        } else if self.manual_compaction {
            format!(
                "Compacting… {} image(s) attached · Enter after Runtime finishes · Esc cancel",
                self.images.len()
            )
        } else if !self.images.is_empty() && self.activity.is_some() {
            format!(
                "{} image(s) attached · Enter steer · {} later",
                self.images.len(),
                follow_up_key()
            )
        } else if !self.images.is_empty() {
            format!("{} image(s) attached · Ctrl+X remove", self.images.len())
        } else if self.activity.is_some() && !self.draft.is_empty() {
            format!(
                "Enter steer · {} follow-up · Ctrl+Enter newline",
                follow_up_key()
            )
        } else {
            String::new()
        };
        let top = if attachment.is_empty() {
            border.clone()
        } else {
            let label = clip(&attachment, width.saturating_sub(6));
            format!(
                "╭─ {label} {}╮",
                "─".repeat(width.saturating_sub(label.chars().count() + 5))
            )
        };
        frame.push_str(&format!(
            "{left}{}\x1b[K\r\n",
            self.accent("38;2;184;57;101", &top)
        ));
        for (offset, draft) in draft_rows[draft_start..draft_end].iter().enumerate() {
            let marker = if draft_start + offset == 0 {
                "❯"
            } else {
                " "
            };
            let edge = if offset == 0 && draft_start > 0 {
                "↑"
            } else if offset + 1 == draft_height && draft_end < draft_rows.len() {
                "↓"
            } else {
                "│"
            };
            let input = format!(
                "│ {marker} {draft}{}{edge}",
                " ".repeat(draft_width.saturating_sub(draft.chars().count()))
            );
            frame.push_str(&format!(
                "{left}{}\x1b[K\r\n",
                self.accent("38;2;255;155;187", &input)
            ));
        }
        frame.push_str(&format!(
            "{left}{}\x1b[K\r\n",
            self.accent(
                "38;2;184;57;101",
                &border.replace('╭', "╰").replace('╮', "╯")
            )
        ));
        let hint = if self.modal.is_some() {
            "Dialog · Esc cancels · your draft is preserved".into()
        } else if self.disconnected {
            "Runtime disconnected · /reconnect · /restore · /quit".into()
        } else if self.suggestions.is_some() && !self.file_matches.is_empty() {
            "↑↓ choose  ·  Enter / Tab insert path  ·  Esc dismiss".into()
        } else if self.suggestions.is_some() {
            "↑↓ choose  ·  Tab complete  ·  Enter run  ·  Esc dismiss".into()
        } else if self.auth_url.is_some() {
            if cfg!(target_os = "macos") {
                "Browser sign-in  ·  Ctrl+Y copy link  ·  Esc cancel".into()
            } else {
                "Open sign-in ↗  ·  Ctrl+Y copy link  ·  Esc cancel".into()
            }
        } else if let Some(activity) = &self.activity {
            let queue_hint = match (self.queue_steering, self.queue_follow_up) {
                (0, 0) => String::new(),
                (steer, later) => format!(" · ↪{steer} ▷{later}"),
            };
            let scrolled = if self.scroll > 0 {
                format!("  ·  ↑ {} lines", self.scroll)
            } else {
                String::new()
            };
            working_status(
                width,
                flower,
                &activity.phase,
                activity.started.elapsed().as_secs(),
                &format!("{queue_hint}{scrolled}"),
                self.goal,
                self.usage.as_ref(),
            )
        } else if self.scroll > 0 {
            format!(
                "↑ {} lines  ·  Wheel / PgDn to return to latest",
                self.scroll
            )
        } else if self.usage.is_some() {
            with_usage("Enter send · /help".into(), width, self.usage.as_ref())
        } else {
            format!(
                "Enter send · Ctrl+Enter newline · {} image · ↑↓ input · /help",
                super::super::clipboard::paste_key()
            )
        };
        let hint = clip(&hint, width);
        frame.push_str(&format!(
            "{left}{}\x1b[K",
            self.accent(self.status_style(), &hint)
        ));
        // The existing spare bottom row is reserved for diagnostics. Neither
        // the composer nor its status/goal row moves when a warning arrives.
        let (diagnostic_revision, diagnostic) = self.diagnostics.snapshot();
        frame.push_str(&format!(
            "\r\n{left}{}\x1b[K",
            self.accent("38;2;255;183;90", &clip(&diagnostic, width))
        ));
        let cursor_row = if let Some((row, _)) = modal_cursor {
            row
        } else if cursor_visible {
            rows.saturating_sub(3 + draft_height - 1 - (cursor_line - draft_start))
                .max(1)
        } else {
            rows.saturating_sub(3).max(1)
        };
        let cursor = Cursor {
            row: cursor_row.min(rows.max(1)),
            column: modal_cursor
                .map_or(
                    left.len() + 5 + cursor_column.min(draft_width),
                    |(_, col)| col,
                )
                .min(columns.max(1)),
            visible: cursor_visible,
        };
        let mut rows_to_paint = frame
            .split("\r\n")
            .map(|line| {
                if self.color {
                    paint_background(&format!("\x1b[0m{line}"), true)
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>();
        // Include the spare bottom row so removed panels cannot leave artifacts.
        rows_to_paint.resize(rows, paint_background("\x1b[K", self.color));
        self.painter
            .paint(&mut self.out, rows_to_paint, (columns, rows), cursor)?;
        self.last_paint = Some(Instant::now());
        self.diagnostic_revision = diagnostic_revision;
        self.paint_pending = false;
        Ok(())
    }
}
