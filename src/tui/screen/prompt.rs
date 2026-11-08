//! Prompts that bypass the transcript: sign-in answers and line input.
use super::*;
impl Screen {
    /// Read an OAuth answer without adding it to the transcript. The callback
    /// can cancel a manual-code prompt when Hibiscus's browser callback finishes.
    pub(crate) fn auth_prompt(
        &mut self,
        events: &Receiver<u8>,
        title: &str,
        placeholder: &str,
        secret: bool,
        mut cancelled: impl FnMut() -> Result<bool>,
    ) -> Result<Option<String>> {
        if self.full {
            let modal = Modal::input(title, String::new(), placeholder.into(), secret);
            let response = self.run_modal(modal, events, &mut cancelled)?;
            return Ok(response.and_then(|response| response["value"].as_str().map(str::to_owned)));
        }
        self.draft.clear();
        self.draft_cursor = 0;
        self.secret_input = secret;
        self.suggestions = None;
        let result = (|| {
            self.render()?;
            let mut bytes = Vec::new();
            loop {
                if cancelled()? {
                    return Ok(None);
                }
                let byte = match events.recv_timeout(Duration::from_millis(40)) {
                    Ok(byte) => byte,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
                };
                match byte {
                    b'\r' | b'\n' => return Ok(Some(self.draft.clone())),
                    25 => {
                        self.copy_auth_url()?;
                    } // Ctrl+Y, without echoing manual code
                    3 | 4 => return Ok(None),
                    8 | 127 => {
                        bytes.clear();
                        self.draft.pop();
                        self.draft_cursor = self.draft.len();
                        self.render()?;
                    }
                    27 => {
                        if matches!(
                            escape_key(events),
                            Navigation::Escape | Navigation::EscapeWith(_)
                        ) {
                            return Ok(None);
                        }
                    }
                    32..=126 | 128..=255 if self.draft.len() < 8192 => {
                        bytes.push(byte);
                        match std::str::from_utf8(&bytes) {
                            Ok(text) => {
                                self.draft.push_str(text);
                                self.draft_cursor = self.draft.len();
                                bytes.clear();
                                self.render()?;
                            }
                            Err(error) if error.error_len().is_some() => bytes.clear(),
                            Err(_) => {}
                        }
                    }
                    _ => {}
                }
            }
        })();
        self.draft.clear();
        self.draft_cursor = 0;
        self.secret_input = false;
        self.render()?;
        result
    }

    pub(super) fn read_prompt(
        &mut self,
        events: &Receiver<u8>,
        mut updates: Option<&Receiver<Option<String>>>,
    ) -> Result<PromptAction> {
        let mut pending = std::mem::take(&mut self.active_utf8);
        let mut queued = self.pending_input.take();
        loop {
            self.poll_clipboard()?;
            self.poll_file_suggestions()?;
            if self
                .exit_armed
                .is_some_and(|armed| armed.elapsed() >= Duration::from_secs(2))
            {
                self.exit_armed = None;
                if self.input_notice.as_deref() == Some("Press Ctrl+C again within 2s to quit") {
                    self.input_notice = None;
                    self.render()?;
                }
            }
            if self.full
                && !self.suspended
                && (self.painter.resized(self.size()) || self.diagnostics_changed())
            {
                self.render()?;
            }
            // A delayed network check can interrupt idle input, never a draft
            // already being typed. No update result ever blocks terminal input.
            if self.draft.is_empty()
                && self.images.is_empty()
                && self.clipboard.is_none()
                && self.exit_armed.is_none()
                && pending.is_empty()
                && queued.is_none()
                && self.deferred_input.is_empty()
            {
                // Give already queued typing priority over an update notice.
                queued = events.try_recv().ok();
                if queued.is_none() {
                    if let Some(receiver) = updates {
                        match receiver.try_recv() {
                            Ok(Some(version)) => return Ok(PromptAction::Update(version)),
                            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                updates = None
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {}
                        }
                    }
                }
            }
            let byte = match queued
                .take()
                .or_else(|| self.pending_input.take())
                .or_else(|| self.deferred_input.pop_front())
            {
                Some(byte) => byte,
                None => {
                    if !self.full && updates.is_none() && self.clipboard.is_none() {
                        match events.recv() {
                            Ok(byte) => byte,
                            Err(_) => return Ok(PromptAction::Text(None)),
                        }
                    } else {
                        match events.recv_timeout(Duration::from_millis(100)) {
                            Ok(byte) => byte,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                return Ok(PromptAction::Text(None));
                            }
                        }
                    }
                }
            };
            if self.feed_paste(byte)? {
                continue;
            }
            if byte != 3
                && self.exit_armed.take().is_some()
                && self.input_notice.as_deref() == Some("Press Ctrl+C again within 2s to quit")
            {
                self.input_notice = None;
                self.render()?;
            }
            match byte {
                b'\n' => self.input_navigation(Navigation::Newline)?,
                3 if self.idle_ctrl_c(&mut pending)? => return Ok(PromptAction::Text(None)),
                3 => {}
                b'\r' => {
                    if self.complete_file()? {
                        continue;
                    }
                    if self.clipboard.is_some() {
                        self.clipboard_notice = "Reading clipboard… press Enter when ready".into();
                        self.render()?;
                        continue;
                    }
                    if !self.images.is_empty() && self.draft.trim_start().starts_with('/') {
                        self.clipboard_notice =
                            "Ctrl+X: remove images before running a command".into();
                        self.render()?;
                        continue;
                    }
                    let completed = self.suggestions.as_ref().and_then(|picker| {
                        self.command_matches()
                            .get(picker.selected)
                            .map(|cmd| cmd.name.to_owned())
                    });
                    let message = completed.unwrap_or_else(|| std::mem::take(&mut self.draft));
                    self.draft.clear();
                    self.draft_cursor = 0;
                    self.preferred_column = None;
                    self.suggestions = None;
                    self.clear_file_suggestions();
                    self.render()?;
                    return Ok(PromptAction::Text(Some(message)));
                }
                22 => self.paste_image()?, // Ctrl+V: explicit image clipboard access
                24 => {
                    // Ctrl+X: remove pending attachments without changing text
                    self.clear_images();
                    self.refresh_suggestions();
                    self.render()?;
                }
                4 if self.draft.is_empty()
                    && self.images.is_empty()
                    && self.clipboard.is_none() =>
                {
                    return Ok(PromptAction::Text(None));
                }
                8 | 127 => {
                    pending.clear();
                    self.backspace_draft()?;
                }
                4 => self.input_navigation(Navigation::Delete)?,
                1 => self.input_navigation(Navigation::Home)?,
                5 => self.input_navigation(Navigation::End)?,
                2 => self.input_navigation(Navigation::Left)?,
                6 => self.input_navigation(Navigation::Right)?,
                b'\t' => {
                    if self.complete_file()? {
                        continue;
                    }
                    if let Some(picker) = self.suggestions.as_ref() {
                        if let Some(cmd) = self.command_matches().get(picker.selected) {
                            self.draft = cmd.name.to_owned();
                            self.draft_cursor = self.draft.len();
                            self.preferred_column = None;
                            self.suggestions = None;
                            self.suggestions_dismissed = true;
                            self.render()?;
                        }
                    }
                }
                27 => match escape_key(events) {
                    Navigation::EscapeWith(next) => {
                        self.input_navigation(Navigation::Escape)?;
                        queued = Some(next);
                    }
                    Navigation::FollowUp => {
                        // Alt+Enter is a follow-up only during an active run.
                        // At idle it is an ordinary Enter (and still dismisses
                        // a slash picker before running the typed command).
                        self.input_navigation(Navigation::Escape)?;
                        queued = Some(b'\r');
                    }
                    key => self.input_navigation(key)?,
                },
                32..=126 | 128..=255 => {
                    self.insert_input_batch(byte, events, &mut pending);
                    self.suggestions_dismissed = false;
                    self.refresh_suggestions();
                    self.render()?;
                }
                _ => {}
            }
        }
    }
}
