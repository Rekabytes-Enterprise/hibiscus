//! Runtime display events applied to the fullscreen state.
use super::*;
impl ScrollDisplay for Screen {
    fn extension_dialog(&mut self, event: &Value, events: &Receiver<u8>) -> Result<Option<Value>> {
        if !self.full || self.suspended {
            return Ok(None);
        }
        let Some(modal) = Modal::from_rpc(event) else {
            return Ok(Some(serde_json::json!({"cancelled":true})));
        };
        let response = self.run_modal(modal, events, || Ok(false))?;
        Ok(Some(
            response.unwrap_or_else(|| serde_json::json!({"cancelled":true})),
        ))
    }
    fn mouse(&mut self, event: MouseEvent) -> io::Result<()> {
        self.select_mouse(event)
    }
    fn paste_start(&mut self) {
        self.paste = Some(PasteInput::default());
    }
    fn paste_byte(&mut self, byte: u8) -> io::Result<bool> {
        self.feed_paste(byte)
    }
    fn scroll(&mut self, navigation: Navigation) -> io::Result<()> {
        let step = self.size().1.saturating_sub(10).max(1);
        let offset = match navigation {
            Navigation::ScrollPage(direction) => direction * step as i32,
            Navigation::ScrollLines(lines) | Navigation::MouseScroll(lines, _) => lines,
            _ => 0,
        };
        if offset > 0 {
            self.scroll = self.scroll.saturating_add(offset as usize);
        } else {
            self.scroll = self.scroll.saturating_sub(offset.unsigned_abs() as usize);
        }
        if offset != 0 {
            self.selection = None;
            self.render()?;
        }
        Ok(())
    }

    fn start_work(&mut self) -> io::Result<()> {
        self.exit_armed = None;
        if self.input_notice.as_deref() == Some("Press Ctrl+C again within 2s to quit") {
            self.input_notice = None;
        }
        self.current_timeline = None;
        if !self.full {
            return Ok(());
        }
        let now = Instant::now();
        self.activity = Some(Activity {
            phase: "Working…".into(),
            started: now,
            last_refresh: now,
            last_spinner: now,
            flower_frame: 0,
        });
        self.render()
    }

    fn set_work(&mut self, phase: &str) -> io::Result<()> {
        if let Some(activity) = self.activity.as_mut() {
            let phase: String = phase
                .chars()
                .filter(|ch| !ch.is_control())
                .take(90)
                .collect();
            if activity.phase != phase {
                activity.phase = phase;
                self.render()?;
            }
        }
        Ok(())
    }

    fn tick_work(&mut self) -> io::Result<()> {
        self.poll_clipboard()?;
        self.poll_file_suggestions()?;
        let mut changed = false;
        if let Some(activity) = self.activity.as_mut() {
            let now = Instant::now();
            if now.duration_since(activity.last_refresh) >= Duration::from_secs(1) {
                activity.last_refresh = now;
                changed = true;
            }
            if now.duration_since(activity.last_spinner) >= Duration::from_millis(320) {
                activity.last_spinner = now;
                activity.flower_frame = (activity.flower_frame + 1) % FLOWERS.len();
                changed = true;
                if let Some(index) = self.current_timeline {
                    if self.timelines[index].advance_explore() {
                        self.dirty_transcript();
                        changed = true;
                    }
                }
            }
        }
        if changed
            || self.painter.resized(self.size())
            || self.diagnostics_changed()
            || (self.paint_pending
                && self
                    .last_paint
                    .is_none_or(|last| last.elapsed() >= Duration::from_millis(33)))
        {
            self.render()?;
        }
        Ok(())
    }

    fn compaction_mode(&mut self, active: bool) -> io::Result<()> {
        self.manual_compaction = active;
        if !active
            && self
                .input_notice
                .as_deref()
                .is_some_and(|text| text.starts_with("Compacting…"))
        {
            self.input_notice = None;
        }
        self.render()
    }
    fn complete_diagrams(&mut self) -> io::Result<()> {
        if !self.full {
            return Ok(());
        }
        let text = self.rendered_transcript();
        let pending: Vec<_> = markdown::mermaid_sources(&text)
            .into_iter()
            .filter(|source| !self.diagrams.contains_key(source))
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        let rendered = super::super::mermaid::render(&pending);
        if self.diagrams.len() > 64 {
            self.diagrams.clear();
        }
        for source in pending {
            self.diagrams.insert(
                source.clone(),
                rendered.get(&source).cloned().unwrap_or_default(),
            );
        }
        self.dirty_transcript();
        self.layout.invalidate();
        self.render()
    }
    fn stop_work(&mut self) -> io::Result<()> {
        self.freeze_timeline();
        self.queue_steering = 0;
        self.queue_follow_up = 0;
        self.pending_queue.clear();
        self.delivered_before_ack.clear();
        self.initial_user_pending = None;
        if self.input_notice.as_deref().is_some_and(|notice| {
            notice.contains("accepted by runtime")
                || notice.starts_with("Sending")
                || notice.starts_with("Queueing")
        }) {
            self.input_notice = None;
        }
        if self.full && self.transcript.len() > 256 * 1024 {
            self.dirty_transcript();
            let excess = self.transcript.len() - 256 * 1024;
            let cut = self.transcript[excess..]
                .iter()
                .position(|b| *b == b'\n')
                .map_or(excess, |index| excess + index + 1);
            self.transcript.drain(..cut);
            self.scroll = 0;
        }
        if self.activity.take().is_some() {
            self.render()?;
        }
        Ok(())
    }

    fn thinking_start(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn thinking_end(&mut self) -> io::Result<bool> {
        Ok(self.terminal)
    }
    fn tool_start(&mut self, id: &str, call: &tool::Call) -> io::Result<bool> {
        let path = if call.name == "bash" {
            ""
        } else {
            &call.target
        };
        self.update_timeline(|timeline| timeline.tool_start(id, &call.name, path))?;
        Ok(true)
    }
    fn tool_end(
        &mut self,
        id: &str,
        call: &tool::Call,
        result: &tool::Outcome<'_>,
    ) -> io::Result<bool> {
        let path = if call.name == "bash" {
            ""
        } else {
            &call.target
        };
        self.update_timeline(|timeline| {
            timeline.tool_end(id, &call.name, path, result.failed, result.diff)
        })?;
        Ok(true)
    }
    fn split_timeline(&mut self) -> io::Result<()> {
        self.freeze_timeline();
        Ok(())
    }
    fn live_input(&self) -> bool {
        self.full && self.activity.is_some()
    }
    fn pending_key(&mut self) -> Option<u8> {
        self.pending_input.take()
    }
    fn active_key(&mut self, byte: u8, events: &Receiver<u8>) -> Result<ActiveAction> {
        self.poll_clipboard()?;
        if self.feed_paste(byte)? {
            return Ok(ActiveAction::None);
        }
        match byte {
            b'\r' => return Ok(self.send_active_draft(QueueMode::Steer)?),
            b'\t' => {
                self.complete_file()?;
            }
            17 => return Ok(self.send_active_draft(QueueMode::FollowUp)?), // Ctrl+Q (WSL)
            b'\n' => self.input_navigation(Navigation::Newline)?,
            22 => self.paste_image()?,
            24 => {
                self.clear_images();
                self.render()?;
            }
            3 => {
                let text = self.selected_text();
                if !text.is_empty() {
                    super::super::text_clipboard::copy(&mut self.out, &text)?;
                    self.selection = None;
                } else {
                    self.draft.clear();
                    self.draft_cursor = 0;
                    self.preferred_column = None;
                    self.clear_images();
                    self.active_utf8.clear();
                    self.suggestions = None;
                    self.clear_file_suggestions();
                }
                self.render()?;
            }
            8 | 127 => self.backspace_draft()?,
            4 => self.input_navigation(Navigation::Delete)?,
            1 => self.input_navigation(Navigation::Home)?,
            5 => self.input_navigation(Navigation::End)?,
            2 => self.input_navigation(Navigation::Left)?,
            6 => self.input_navigation(Navigation::Right)?,
            27 => match escape_key(events) {
                Navigation::Escape if !self.file_matches.is_empty() => {
                    self.input_navigation(Navigation::Escape)?
                }
                Navigation::Escape => return Ok(ActiveAction::Stop),
                Navigation::EscapeWith(next) => {
                    self.pending_input = Some(next);
                    return Ok(ActiveAction::Stop);
                }
                Navigation::FollowUp => return Ok(self.send_active_draft(QueueMode::FollowUp)?),
                nav => self.input_navigation(nav)?,
            },
            32..=126 | 128..=255 => {
                let mut pending = std::mem::take(&mut self.active_utf8);
                self.insert_input_batch(byte, events, &mut pending);
                self.active_utf8 = pending;
                self.suggestions_dismissed = false;
                self.refresh_suggestions();
                self.render()?;
            }
            _ => {}
        }
        Ok(ActiveAction::None)
    }
    fn hold_queue(&mut self, submission: DraftSubmission) -> io::Result<()> {
        if self.draft.is_empty() && self.images.is_empty() {
            self.draft = submission.text;
            self.draft_cursor = self.draft.len();
            self.images = submission.images;
            self.ensure_cursor_visible();
        }
        self.input_notice =
            Some("Runtime is starting; press Enter again after it accepts the prompt".into());
        self.render()
    }
    fn expect_initial_user(&mut self, message: &str) {
        if self.full {
            self.initial_user_pending = Some(message.to_owned());
        }
    }
    fn queued(&mut self, submission: &DraftSubmission) -> io::Result<()> {
        if let Some(index) = self
            .delivered_before_ack
            .iter()
            .position(|text| text == &submission.text)
        {
            self.delivered_before_ack.remove(index);
        } else if let Some(pending) = self.pending_queue.iter_mut().find(|item| {
            item.text == submission.text && item.mode == submission.mode && !item.accepted
        }) {
            pending.accepted = true;
            pending.images = submission.images.len();
        } else {
            self.pending_queue.push(PendingQueue {
                text: submission.text.clone(),
                mode: submission.mode,
                images: submission.images.len(),
                accepted: true,
                sending: false,
            });
        }
        let kind = match submission.mode {
            QueueMode::Steer => "steering",
            QueueMode::FollowUp => "follow-up",
        };
        self.input_notice = Some(format!("{kind} accepted by Runtime · waiting for delivery"));
        self.render()
    }
    fn delivered_user(&mut self, message: &Value) -> io::Result<bool> {
        if !self.full {
            return Ok(false);
        }
        let (text, images) = user_content(&message["content"]);
        if self.initial_user_pending.as_deref() == Some(text.as_str()) {
            self.initial_user_pending = None;
            return Ok(false);
        }
        let index = self
            .pending_queue
            .iter()
            .position(|item| item.text == text && item.sending)
            .or_else(|| self.pending_queue.iter().position(|item| item.text == text));
        let unacknowledged = index
            .map(|index| !self.pending_queue.remove(index).accepted)
            .unwrap_or(true);
        if unacknowledged && !text.is_empty() {
            self.delivered_before_ack.push(text.clone());
            if self.delivered_before_ack.len() > 16 {
                self.delivered_before_ack.remove(0);
            }
        }
        let image_note = if images > 0 {
            format!(" [{} image(s) attached]", images)
        } else {
            String::new()
        };
        let shown = format!("\nyou › {}{image_note}\n", text.trim_start_matches('\n'));
        if self.suspended {
            self.dirty_transcript();
            self.transcript.extend_from_slice(shown.as_bytes());
        } else {
            self.write_all(shown.as_bytes())?;
        }
        self.render()?;
        Ok(true)
    }
    fn rejected_queue(&mut self, submission: DraftSubmission, error: &str) -> io::Result<()> {
        if let Some(index) = self.pending_queue.iter().position(|item| {
            item.text == submission.text && item.mode == submission.mode && !item.accepted
        }) {
            self.pending_queue.remove(index);
        }
        if self.draft.is_empty() && self.images.is_empty() {
            self.draft = submission.text.clone();
            self.draft_cursor = self.draft.len();
            self.images = submission.images.clone();
            self.ensure_cursor_visible();
        }
        self.rejected_queued = Some((submission.text, submission.images));
        let label = if error.contains("uncertain") {
            "Queued delivery uncertain"
        } else {
            "Runtime rejected queued message"
        };
        self.input_notice = Some(format!(
            "{label}: {} · /restore after run",
            crate::runtime::error::safe_message(error)
        ));
        self.render()
    }
    fn queue_update(&mut self, update: &Value) -> io::Result<()> {
        self.queue_steering = update["steering"].as_array().map_or(0, Vec::len);
        self.queue_follow_up = update["followUp"].as_array().map_or(0, Vec::len);
        let mut old = std::mem::take(&mut self.pending_queue);
        let mut current = Vec::new();
        for (mode, field) in [
            (QueueMode::Steer, "steering"),
            (QueueMode::FollowUp, "followUp"),
        ] {
            if let Some(messages) = update[field].as_array() {
                let mut kept = Vec::new();
                // Keep the newest matching entries when identical queued texts
                // shrink: Runtime drains the oldest first, and events have no IDs.
                for text in messages.iter().filter_map(Value::as_str).rev() {
                    let mut item = if let Some(index) = old
                        .iter()
                        .rposition(|item| item.mode == mode && item.text == text && !item.sending)
                    {
                        old.remove(index)
                    } else {
                        PendingQueue {
                            text: text.to_owned(),
                            mode,
                            images: 0,
                            accepted: false,
                            sending: false,
                        }
                    };
                    item.sending = false;
                    kept.push(item);
                }
                kept.reverse();
                current.extend(kept);
            }
        }
        for mut item in old {
            item.sending = true; // removed from Hibiscus's queue, awaiting message_start
            current.push(item);
        }
        self.pending_queue = current;
        self.render()
    }
    fn clear_queue_display(&mut self) -> io::Result<()> {
        self.queue_steering = 0;
        self.queue_follow_up = 0;
        self.pending_queue.clear();
        self.delivered_before_ack.clear();
        self.render()
    }
    fn goal(&mut self, details: &Value) -> io::Result<()> {
        if self.full {
            self.set_goal(details);
            self.render()?;
        }
        Ok(())
    }
}
