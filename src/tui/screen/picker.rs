//! List pickers and modal dialogs in the fullscreen view.
use super::*;
impl Screen {
    pub(crate) fn select(
        &mut self,
        title: &str,
        items: &[String],
        current: Option<usize>,
        events: &Receiver<u8>,
    ) -> Result<Option<usize>> {
        self.select_inner(false, title, items, current, None, events)
    }

    pub(crate) fn select_searchable(
        &mut self,
        title: &str,
        items: &[String],
        current: Option<usize>,
        tabs: Option<Tabs>,
        events: &Receiver<u8>,
    ) -> Result<Option<usize>> {
        self.select_inner(true, title, items, current, tabs, events)
    }

    pub(super) fn select_inner(
        &mut self,
        searchable: bool,
        title: &str,
        items: &[String],
        current: Option<usize>,
        tabs: Option<Tabs>,
        events: &Receiver<u8>,
    ) -> Result<Option<usize>> {
        if !self.full || items.is_empty() {
            return Ok(None);
        }
        let rows = self.selectable_rows.len().saturating_sub(3).clamp(1, 5);
        self.picker = Some(Picker::new(clean(title), items.to_vec(), current, rows));
        if searchable {
            self.picker.as_mut().unwrap().enable_search();
        }
        if let Some(tabs) = tabs {
            let rows = self.selectable_rows.len().saturating_sub(7).max(1);
            let picker = self.picker.as_mut().unwrap();
            picker.enable_tabs(&tabs.names, tabs.groups, tabs.prefixes, tabs.tab, rows);
            if !tabs.query.is_empty() {
                picker.search(&tabs.query, false, rows);
            }
        }
        let mut utf8 = Vec::new();
        let mut paste: Option<PasteInput> = None;
        let result = (|| {
            self.render()?;
            loop {
                let chrome = self.picker.as_ref().map_or(3, Picker::chrome);
                let rows = self.selectable_rows.len().saturating_sub(chrome).max(1);
                let byte = match events.recv_timeout(Duration::from_millis(100)) {
                    Ok(byte) => byte,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if self.painter.resized(self.size()) || self.diagnostics_changed() {
                            self.render()?;
                        }
                        continue;
                    }
                    Err(_) => break,
                };
                if let Some(input) = paste.as_mut() {
                    if let Some(text) = input.push(byte) {
                        paste = None;
                        if let Some(text) = text {
                            self.picker.as_mut().unwrap().search(
                                &text.replace(['\r', '\n'], " "),
                                false,
                                rows,
                            );
                            self.render()?;
                        }
                    }
                    continue;
                }
                if searchable && matches!(byte, 8 | 127 | 32..=126 | 128..=255) {
                    if matches!(byte, 8 | 127) {
                        utf8.clear();
                        self.picker.as_mut().unwrap().search("", true, rows);
                    } else {
                        utf8.push(byte);
                        match std::str::from_utf8(&utf8) {
                            Ok(text) => {
                                self.picker.as_mut().unwrap().search(text, false, rows);
                                utf8.clear();
                            }
                            Err(error) if error.error_len().is_some() => utf8.clear(),
                            Err(_) => {}
                        }
                    }
                    self.render()?;
                    continue;
                }
                utf8.clear();
                let movement = match byte {
                    b'\r' | b'\n' => {
                        if let Some(index) = self.picker.as_ref().and_then(Picker::selected_index) {
                            return Ok(Some(index));
                        }
                        continue;
                    }
                    3 | 4 => return Ok(None),
                    27 => match escape_key(events) {
                        Navigation::Escape => return Ok(None),
                        Navigation::EscapeWith(next) => {
                            self.pending_input = Some(next);
                            return Ok(None);
                        }
                        key => key,
                    },
                    b'\t' => Navigation::Right,
                    b'j' => Navigation::Down,
                    b'k' => Navigation::Up,
                    _ => Navigation::Other,
                };
                if searchable && movement == Navigation::PasteStart {
                    paste = Some(PasteInput::default());
                    continue;
                }
                let movement = match movement {
                    Navigation::MouseScroll(lines, _) => Navigation::ScrollLines(lines),
                    key => key,
                };
                if let Some(picker) = self.picker.as_mut() {
                    match movement {
                        Navigation::Up | Navigation::ScrollLines(3) => picker.move_by(-1, rows),
                        Navigation::Down | Navigation::ScrollLines(-3) => picker.move_by(1, rows),
                        Navigation::ScrollPage(dir) => picker.page(-(dir as isize), rows),
                        Navigation::Home => picker.move_by(-(picker.items.len() as isize), rows),
                        Navigation::End => picker.move_by(picker.items.len() as isize, rows),
                        Navigation::Left if !picker.tabs.is_empty() => picker.switch_tab(-1, rows),
                        Navigation::Right if !picker.tabs.is_empty() => picker.switch_tab(1, rows),
                        _ => continue,
                    }
                }
                self.render()?;
            }
            Ok(None)
        })();
        self.picker = None;
        self.render()?;
        result
    }

    pub(super) fn run_modal(
        &mut self,
        modal: Modal,
        events: &Receiver<u8>,
        mut cancelled: impl FnMut() -> Result<bool>,
    ) -> Result<Option<Value>> {
        let saved_scroll = self.draft_scroll;
        self.modal = Some(modal);
        let result = (|| {
            self.render()?;
            loop {
                if cancelled()? || self.modal.as_ref().is_none_or(Modal::expired) {
                    return Ok(None);
                }
                let (columns, rows) = self.size();
                if columns < 20 || rows < 14 {
                    return Ok(None);
                }
                let byte = match events.recv_timeout(Duration::from_millis(40)) {
                    Ok(byte) => byte,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if self.painter.resized((columns, rows)) || self.diagnostics_changed() {
                            self.render()?;
                        }
                        continue;
                    }
                    Err(_) => return Ok(None),
                };
                if self.modal.as_ref().is_some_and(Modal::expired) {
                    return Ok(None);
                }
                if byte == 25 && self.auth_url.is_some() {
                    self.copy_auth_url()?;
                } else if byte == 27 {
                    match escape_key(events) {
                        Navigation::Escape => return Ok(None),
                        Navigation::EscapeWith(next) => {
                            self.defer_input(next);
                            return Ok(None);
                        }
                        key => self.modal.as_mut().unwrap().navigate(key),
                    }
                } else if let Some(response) = self.modal.as_mut().unwrap().key(byte) {
                    return Ok(Some(response));
                }
                self.render()?;
            }
        })();
        self.modal = None;
        self.draft_scroll = saved_scroll;
        let cleanup = self.render();
        cleanup?;
        result
    }
}
