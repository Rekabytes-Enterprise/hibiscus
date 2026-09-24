use serde_json::Value;
use std::env;
use std::io::{self, IsTerminal, Write};
use std::os::fd::AsRawFd;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::{
    markdown::{self, Role, Tone},
    picker::Picker,
};
use crate::{chat::commands, Result};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PromptAction {
    Text(Option<String>),
    Update(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Navigation {
    Escape,
    EscapeWith(u8),
    ScrollPage(i32),
    ScrollLines(i32),
    Up,
    Down,
    Home,
    End,
    Newline,
    PasteImage,
    MouseScroll(i32, usize),
    Other,
}

pub(crate) fn escape_key(events: &Receiver<u8>) -> Navigation {
    match events.recv_timeout(std::time::Duration::from_millis(25)) {
        Ok(b'[') => {}
        Ok(b'v') => return Navigation::PasteImage, // legacy Alt+V (Pi's WSL default)
        Ok(next) => return Navigation::EscapeWith(next),
        Err(_) => return Navigation::Escape,
    }
    let mut sequence = String::new();
    while let Ok(next) = events.recv_timeout(std::time::Duration::from_millis(25)) {
        sequence.push(next as char);
        if next.is_ascii_alphabetic() || next == b'~' || sequence.len() >= 32 {
            break;
        }
    }
    match sequence.as_str() {
        "118;3u" | "118;5u" | "27;3;118~" | "27;5;118~" | "118;3:1u" | "118;5:1u" | "118;3:2u"
        | "118;5:2u" => Navigation::PasteImage,
        "13;2u" | "27;2;13~" | "13;2~" | "13;5u" | "27;5;13~" | "13;5~" => Navigation::Newline,
        "5~" => Navigation::ScrollPage(1),
        "6~" => Navigation::ScrollPage(-1),
        "A" => Navigation::Up,
        "B" => Navigation::Down,
        "H" | "1~" => Navigation::Home,
        "F" | "4~" => Navigation::End,
        seq if (seq.starts_with("<64;") || seq.starts_with("<65;")) && seq.ends_with('M') => {
            let row = seq
                .trim_end_matches('M')
                .rsplit(';')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            Navigation::MouseScroll(if seq.starts_with("<64;") { 3 } else { -3 }, row)
        }
        _ => Navigation::Other,
    }
}

pub(crate) trait ScrollDisplay: Write {
    fn scroll(&mut self, _navigation: Navigation) -> io::Result<()> {
        Ok(())
    }
    fn start_work(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn set_work(&mut self, _phase: &str) -> io::Result<()> {
        Ok(())
    }
    fn tick_work(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn stop_work(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl ScrollDisplay for io::Stdout {}
impl ScrollDisplay for Vec<u8> {}

const FLOWERS: [&str; 4] = ["✿", "❀", "✾", "❁"];

struct Activity {
    phase: String,
    started: Instant,
    last_frame: Instant,
    frame: usize,
}

/// An alternate-screen transcript with a persistent composer. Non-terminal
/// streams pass through untouched; RPC responses are never written here.
pub(crate) struct Screen {
    out: io::Stdout,
    full: bool,
    suspended: bool,
    color: bool,
    transcript: Vec<u8>,
    draft: String,
    draft_scroll: usize,
    images: Vec<Value>,
    clipboard: Option<super::clipboard::Job>,
    clipboard_notice: String,
    model: String,
    session: String,
    scroll: usize,
    picker: Option<Picker>,
    suggestions: Option<Picker>,
    suggestions_dismissed: bool,
    pending_input: Option<u8>,
    secret_input: bool,
    auth_url: Option<String>,
    activity: Option<Activity>,
}

impl Screen {
    pub(crate) fn new(interactive: bool) -> io::Result<Self> {
        let out = io::stdout();
        let full =
            interactive && out.is_terminal() && env::var("TERM").is_ok_and(|term| term != "dumb");
        let color = full && env::var_os("NO_COLOR").is_none();
        let mut screen = Self {
            out,
            full,
            suspended: true,
            color,
            transcript: Vec::new(),
            draft: String::new(),
            draft_scroll: 0,
            images: Vec::new(),
            clipboard: None,
            clipboard_notice: String::new(),
            model: "no model".into(),
            session: "new chat".into(),
            scroll: 0,
            picker: None,
            suggestions: None,
            suggestions_dismissed: false,
            pending_input: None,
            secret_input: false,
            auth_url: None,
            activity: None,
        };
        if screen.size().0 < 40 || screen.size().1 < 14 {
            screen.full = false;
            screen.color = false;
        }
        if screen.full {
            screen.resume()?;
        }
        Ok(screen)
    }

    pub(crate) fn is_full(&self) -> bool {
        self.full
    }

    /// Keep the full authorization URL out of transcript text; render only a
    /// short OSC 8 hyperlink and offer an explicit clipboard request.
    pub(crate) fn set_auth_url(&mut self, url: &str) -> io::Result<bool> {
        if !url.starts_with("https://") || url.len() > 16_384 || url.chars().any(char::is_control) {
            return Ok(false);
        }
        self.auth_url = Some(url.to_owned());
        self.render()?;
        Ok(true)
    }

    pub(crate) fn clear_auth_url(&mut self) -> io::Result<()> {
        self.auth_url = None;
        self.render()
    }

    pub(crate) fn copy_auth_url(&mut self) -> io::Result<bool> {
        let Some(url) = self.auth_url.as_ref() else {
            return Ok(false);
        };
        write!(self.out, "\x1b]52;c;{}\x07", base64(url.as_bytes()))?;
        self.out.flush()?;
        writeln!(
            self,
            "  · Sent sign-in URL to terminal clipboard (if supported)."
        )?;
        self.flush()?;
        Ok(true)
    }

    pub(crate) fn suspend(&mut self) -> io::Result<()> {
        if self.full && !self.suspended {
            write!(
                self.out,
                "\x1b[<u\x1b[?1000l\x1b[?1006l\x1b[?25h\x1b[?1049l"
            )?;
            self.suspended = true;
            self.out.flush()?;
        }
        Ok(())
    }

    pub(crate) fn resume(&mut self) -> io::Result<()> {
        if self.full && self.suspended {
            write!(
                self.out,
                "\x1b[?1049h\x1b[>1u\x1b[2J\x1b[?1000h\x1b[?1006h\x1b[?25l"
            )?;
            self.suspended = false;
            self.render()?;
        }
        Ok(())
    }

    pub(crate) fn status(&mut self, state: &Value) -> io::Result<()> {
        let provider = state["model"]["provider"].as_str().unwrap_or("");
        let model = state["model"]["id"].as_str().unwrap_or("no model");
        self.model = if provider.is_empty() {
            clean(model)
        } else {
            format!("{}/{}", clean(provider), clean(model))
        };
        self.session = state["sessionName"]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(clean)
            .or_else(|| {
                state["sessionId"]
                    .as_str()
                    .map(|id| clean(&id.chars().take(8).collect::<String>()))
            })
            .unwrap_or_else(|| "new chat".into());
        self.render()
    }

    pub(crate) fn user(&mut self, text: &str) -> io::Result<()> {
        self.draft.clear();
        self.suggestions = None;
        if self.full {
            while self.transcript.last() == Some(&b'\n') {
                self.transcript.pop();
            }
            self.transcript
                .extend_from_slice(format!("\n\nyou › {text}\n").as_bytes());
        } else {
            writeln!(self, "\nyou › {text}")?;
        }
        self.scroll = 0;
        self.render()
    }

    #[cfg(test)]
    pub(crate) fn prompt(&mut self, events: &Receiver<u8>) -> Result<Option<String>> {
        match self.prompt_with_update(events, None)? {
            PromptAction::Text(text) => Ok(text),
            PromptAction::Update(_) => unreachable!(),
        }
    }

    pub(crate) fn prompt_with_update(
        &mut self,
        events: &Receiver<u8>,
        updates: Option<&Receiver<Option<String>>>,
    ) -> Result<PromptAction> {
        self.draft.clear();
        self.suggestions = None;
        self.suggestions_dismissed = false;
        self.render()?;
        self.read_prompt(events, updates)
    }

    pub(crate) fn select(
        &mut self,
        title: &str,
        items: &[String],
        current: Option<usize>,
        events: &Receiver<u8>,
    ) -> Result<Option<usize>> {
        if !self.full || items.is_empty() {
            return Ok(None);
        }
        let rows = self.size().1.saturating_sub(12).clamp(1, 5);
        self.picker = Some(Picker::new(clean(title), items.to_vec(), current, rows));
        let result = (|| {
            self.render()?;
            while let Ok(byte) = events.recv() {
                let movement = match byte {
                    b'\r' | b'\n' => return Ok(self.picker.as_ref().map(|picker| picker.selected)),
                    3 | 4 => return Ok(None),
                    27 => match escape_key(events) {
                        Navigation::Escape => return Ok(None),
                        Navigation::EscapeWith(next) => {
                            self.pending_input = Some(next);
                            return Ok(None);
                        }
                        key => key,
                    },
                    b'j' => Navigation::Down,
                    b'k' => Navigation::Up,
                    _ => Navigation::Other,
                };
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

    pub(crate) fn take_images(&mut self) -> Vec<Value> {
        self.clipboard_notice.clear();
        std::mem::take(&mut self.images)
    }

    fn paste_image(&mut self) -> io::Result<()> {
        if self.clipboard.is_some() {
            return Ok(());
        }
        if self.images.len() >= 4 {
            self.clipboard_notice = "Maximum 4 images · Ctrl+X remove".into();
        } else {
            self.clipboard = Some(super::clipboard::Job::start());
            self.clipboard_notice = "Reading clipboard…".into();
        }
        self.render()
    }

    fn poll_clipboard(&mut self) -> io::Result<()> {
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

    fn clear_images(&mut self) {
        self.images.clear();
        self.clipboard = None;
        self.clipboard_notice.clear();
    }

    fn refresh_suggestions(&mut self) {
        self.draft_scroll = 0;
        let options = if self.draft.contains('\n') || !self.images.is_empty() {
            Vec::new()
        } else {
            commands::matches(&self.draft)
        };
        self.suggestions = if self.suggestions_dismissed || options.is_empty() {
            None
        } else {
            let items = options
                .iter()
                .map(|cmd| format!("{:<12}  {}", cmd.name, cmd.description))
                .collect();
            let rows = self.size().1.saturating_sub(12).clamp(1, 5);
            Some(Picker::new("Commands".into(), items, None, rows))
        };
    }

    fn input_navigation(&mut self, key: Navigation) -> io::Result<()> {
        if key == Navigation::PasteImage {
            return self.paste_image();
        }
        if key == Navigation::Newline {
            self.draft.push('\n');
            self.refresh_suggestions();
            return self.render();
        }
        let (columns, height) = self.size();
        let layout = draft_lines(
            &self.draft,
            columns.saturating_sub(4).clamp(1, 100).saturating_sub(5),
        );
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
            Navigation::Up if self.suggestions.is_none() => {
                self.scroll_draft(1, layout.len());
                return self.render();
            }
            Navigation::Down if self.suggestions.is_none() => {
                self.scroll_draft(-1, layout.len());
                return self.render();
            }
            key => key,
        };
        let rows = self.size().1.saturating_sub(12).clamp(1, 5);
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

    fn scroll_draft(&mut self, lines: i32, total: usize) {
        self.draft_scroll = if lines > 0 {
            self.draft_scroll.saturating_add(lines as usize)
        } else {
            self.draft_scroll
                .saturating_sub(lines.unsigned_abs() as usize)
        }
        .min(total.saturating_sub(5));
    }

    /// Read an OAuth answer without adding it to the transcript. The callback
    /// can cancel a manual-code prompt when Pi's browser callback finishes.
    pub(crate) fn auth_input(
        &mut self,
        events: &Receiver<u8>,
        secret: bool,
        mut cancelled: impl FnMut() -> Result<bool>,
    ) -> Result<Option<String>> {
        self.draft.clear();
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
        self.secret_input = false;
        self.render()?;
        result
    }

    fn read_prompt(
        &mut self,
        events: &Receiver<u8>,
        mut updates: Option<&Receiver<Option<String>>>,
    ) -> Result<PromptAction> {
        let mut pending = Vec::new();
        let mut queued = self.pending_input.take();
        loop {
            self.poll_clipboard()?;
            // A delayed network check can interrupt idle input, never a draft
            // already being typed. No update result ever blocks terminal input.
            if self.draft.is_empty()
                && self.images.is_empty()
                && self.clipboard.is_none()
                && pending.is_empty()
                && queued.is_none()
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
            let byte = match queued.take() {
                Some(byte) => byte,
                None => {
                    if updates.is_none() && self.clipboard.is_none() {
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
            match byte {
                b'\n' => self.input_navigation(Navigation::Newline)?,
                b'\r' => {
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
                        commands::matches(&self.draft)
                            .get(picker.selected)
                            .map(|cmd| cmd.name.to_owned())
                    });
                    let message = completed.unwrap_or_else(|| std::mem::take(&mut self.draft));
                    self.draft.clear();
                    self.suggestions = None;
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
                3 | 4
                    if self.draft.is_empty()
                        && self.images.is_empty()
                        && self.clipboard.is_none() =>
                {
                    return Ok(PromptAction::Text(None))
                }
                3 => {
                    self.draft.clear();
                    self.clear_images();
                    pending.clear();
                    self.suggestions_dismissed = false;
                    self.refresh_suggestions();
                    self.render()?;
                }
                8 | 127 => {
                    pending.clear();
                    self.draft.pop();
                    self.suggestions_dismissed = false;
                    self.refresh_suggestions();
                    self.render()?;
                }
                b'\t' => {
                    if let Some(picker) = self.suggestions.as_ref() {
                        if let Some(cmd) = commands::matches(&self.draft).get(picker.selected) {
                            self.draft = cmd.name.to_owned();
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
                    key => self.input_navigation(key)?,
                },
                32..=126 | 128..=255 => {
                    pending.push(byte);
                    match std::str::from_utf8(&pending) {
                        Ok(s) => {
                            self.draft.push_str(s);
                            pending.clear();
                            self.suggestions_dismissed = false;
                            self.refresh_suggestions();
                            self.render()?;
                        }
                        Err(error) if error.error_len().is_some() => pending.clear(),
                        Err(_) => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn size(&self) -> (usize, usize) {
        let mut size = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: stdout is open and ioctl writes the supplied winsize.
        if unsafe { libc::ioctl(self.out.as_raw_fd(), libc::TIOCGWINSZ, &mut size) } == 0
            && size.ws_col > 0
            && size.ws_row > 0
        {
            (usize::from(size.ws_col), usize::from(size.ws_row))
        } else {
            (80, 24)
        }
    }

    fn accent(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    fn status_style(&self) -> &'static str {
        if self.activity.is_some() {
            "1;38;2;236;74;125"
        } else {
            "2"
        }
    }

    fn styled_line(&self, row: &markdown::Row, width: usize) -> String {
        let mut out = String::new();
        let mut previous = Tone::Plain;
        for cell in &row.cells {
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
            out.push(cell.ch);
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
                .starts_with("· Open Codex sign-in ↗");
        if auth_notice {
            let label = "Open Codex sign-in ↗";
            if let (Some(url), Some(start)) = (self.auth_url.as_deref(), out.find(label)) {
                out.insert_str(start + label.len(), "\x1b]8;;\x1b\\");
                out.insert_str(start, &format!("\x1b]8;;{url}\x1b\\"));
            }
        }
        out
    }

    fn transcript_line(
        &self,
        row: &markdown::Row,
        width: usize,
        columns: usize,
        left: &str,
    ) -> String {
        let marker = match row.cells.first().map(|cell| cell.tone) {
            Some(Tone::DiffAdded) => self.accent("38;2;169;224;184", "┃"),
            Some(Tone::DiffRemoved) => self.accent("38;2;255;134;153", "┃"),
            _ if row.role == Role::User => self.accent("1;38;2;236;74;125", "┃"),
            _ => self.accent("2;38;2;184;57;101", "│"),
        };
        let text = format!(
            "{left}{marker}  {}",
            self.styled_line(row, width.saturating_sub(3).max(1))
        );
        if self.color && row.role == Role::User {
            // Reapply the tint after foreground/style resets, and paint the
            // margins and unused columns too. Reset before the next row.
            let background = "\x1b[48;2;57;30;46m";
            let text = text.replace("\x1b[0m", &format!("\x1b[0m{background}"));
            let padding = columns.saturating_sub(left.len() + 3 + row.cells.len());
            format!("{background}{text}{}\x1b[0m", " ".repeat(padding))
        } else {
            text
        }
    }

    fn picker_rows(&self, width: usize, height: usize, left: &str) -> Option<(usize, Vec<String>)> {
        let picker = self.picker.as_ref().or(self.suggestions.as_ref())?;
        let panel_width = width;
        let body = picker.visible(height.saturating_sub(3));
        let title = clip(
            &format!("✿ {}", picker.title),
            panel_width.saturating_sub(5),
        );
        let top = format!(
            "╭─ {title} {}╮",
            "─".repeat(panel_width.saturating_sub(title.chars().count() + 5))
        );
        let mut rows = vec![format!("{left}{}", self.accent("1;38;2;236;74;125", &top))];
        for line in 0..body {
            let index = picker.first + line;
            let selected = index == picker.selected;
            let mark = if selected { "❯" } else { " " };
            let suffix = if picker.current == Some(index) {
                "  ● current"
            } else {
                ""
            };
            let reserved = panel_width.saturating_sub(6 + suffix.chars().count());
            let label = clip(&clean(&picker.items[index]), reserved);
            let inner = format!("{mark} {label}{suffix}");
            let slider = if picker.items.len() <= body {
                '│'
            } else if line == picker.thumb(height.saturating_sub(3)) {
                '█'
            } else {
                '│'
            };
            let plain = format!(
                "│ {:<space$} {slider}",
                inner,
                space = panel_width.saturating_sub(4)
            );
            let styled = if selected {
                self.accent("1;38;2;255;155;187;48;2;77;27;51", &plain)
            } else {
                self.accent("38;2;255;220;230", &plain)
            };
            rows.push(format!("{left}{styled}"));
        }
        let range = format!("{}/{}", picker.selected + 1, picker.items.len());
        let instructions = if self.picker.is_some() {
            format!("↑↓ move · Enter select · Esc cancel   {range}")
        } else {
            format!("↑↓ choose · Tab complete · Enter run · Esc dismiss   {range}")
        };
        let hint = clip(&instructions, panel_width.saturating_sub(4));
        rows.push(format!(
            "{left}│ {:<space$}│",
            hint,
            space = panel_width.saturating_sub(3)
        ));
        rows.push(format!(
            "{left}{}",
            self.accent(
                "38;2;184;57;101",
                &format!("╰{}╯", "─".repeat(panel_width.saturating_sub(2)))
            )
        ));
        let top_row = height.saturating_sub(rows.len());
        Some((top_row, rows))
    }

    pub(crate) fn render(&mut self) -> io::Result<()> {
        if !self.full || self.suspended {
            return Ok(());
        }
        let (columns, rows) = self.size();
        let width = columns.saturating_sub(4).clamp(1, 100);
        let left = " ".repeat(columns.saturating_sub(width) / 2);
        let workspace = env::current_dir()
            .ok()
            .and_then(|path| path.file_name().map(|part| clean(&part.to_string_lossy())))
            .unwrap_or_else(|| "workspace".into());
        let flower = self
            .activity
            .as_ref()
            .map_or(FLOWERS[0], |activity| FLOWERS[activity.frame]);
        let title = clip(&format!("{flower} hibiscus   /   {workspace}"), width);
        let metadata = format!("{}  ·  {}", self.model, self.session);
        let header = clip(&metadata, width);
        let text = strip_ansi(&String::from_utf8_lossy(&self.transcript));
        let lines = markdown::format(&text, width.saturating_sub(3).max(1));
        let draft_width = width.saturating_sub(5).max(1);
        let draft_rows = if self.secret_input {
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
        let available = rows.saturating_sub(8 + draft_height);
        let modal = self.picker_rows(width, available, &left);
        let transcript_rows = modal.as_ref().map_or(available, |(top, _)| *top);
        let viewport = transcript_viewport(lines.len(), transcript_rows, self.scroll);
        self.scroll = viewport.scroll;
        let start = viewport.start;
        let end = viewport.end;
        let mut frame = String::from("\x1b[H\x1b[?25l");
        frame.push_str(&format!(
            "{left}{}\x1b[K\r\n",
            self.accent("1;38;2;236;74;125", &title)
        ));
        frame.push_str(&format!("{left}{}\x1b[K\r\n", self.accent("2", &header)));
        frame.push_str("\x1b[K\r\n");
        for row in 0..available {
            if let Some((top, panel)) = &modal {
                if let Some(line) = row.checked_sub(*top).and_then(|offset| panel.get(offset)) {
                    frame.push_str(&format!("{line}\x1b[K\r\n"));
                    continue;
                }
            }
            if let Some(line) = lines.get(start + row).filter(|_| start + row < end) {
                frame.push_str(&format!(
                    "{}\x1b[K\r\n",
                    self.transcript_line(line, width, columns, &left)
                ));
            } else {
                frame.push_str("\x1b[K\r\n");
            }
        }
        frame.push_str("\x1b[K\r\n");
        let border = format!("╭{}╮", "─".repeat(width.saturating_sub(2)));
        let attachment = if !self.clipboard_notice.is_empty() {
            self.clipboard_notice.clone()
        } else if !self.images.is_empty() {
            format!("{} image(s) attached · Ctrl+X remove", self.images.len())
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
        let hint = if self.suggestions.is_some() {
            "↑↓ choose  ·  Tab complete  ·  Enter run  ·  Esc dismiss".into()
        } else if self.auth_url.is_some() {
            if cfg!(target_os = "macos") {
                "Browser sign-in  ·  Ctrl+Y copy link  ·  Esc cancel".into()
            } else {
                "Open Codex sign-in ↗  ·  Ctrl+Y copy link  ·  Esc cancel".into()
            }
        } else if let Some(activity) = &self.activity {
            let scrolled = if self.scroll > 0 {
                format!("  ·  ↑ {} lines", self.scroll)
            } else {
                String::new()
            };
            format!(
                "{flower} {}  ·  {}s  ·  Esc stop{scrolled}",
                activity.phase,
                activity.started.elapsed().as_secs()
            )
        } else if self.scroll > 0 {
            format!(
                "↑ {} lines  ·  Wheel / PgDn to return to latest",
                self.scroll
            )
        } else {
            format!(
                "Enter send · Ctrl+Enter newline · {} image · ↑↓ input · /help",
                super::clipboard::paste_key()
            )
        };
        let hint = clip(&hint, width);
        frame.push_str(&format!(
            "{left}{}\x1b[J",
            self.accent(self.status_style(), &hint)
        ));
        // Place the cursor after the visible draft inside the composer.
        let cursor_row = rows.saturating_sub(3).max(1);
        let cursor_col = left.len() + 5 + draft_rows.last().map_or(0, |line| line.chars().count());
        if self.picker.is_none() && self.draft_scroll == 0 {
            frame.push_str(&format!(
                "\x1b[{cursor_row};{}H\x1b[?25h",
                cursor_col.min(columns)
            ));
        } else {
            frame.push_str("\x1b[?25l");
        }
        self.out.write_all(frame.as_bytes())?;
        self.out.flush()
    }
}

impl ScrollDisplay for Screen {
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
            self.render()?;
        }
        Ok(())
    }

    fn start_work(&mut self) -> io::Result<()> {
        if !self.full {
            return Ok(());
        }
        let now = Instant::now();
        self.activity = Some(Activity {
            phase: "Working…".into(),
            started: now,
            last_frame: now,
            frame: 0,
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
        if let Some(activity) = self.activity.as_mut() {
            let now = Instant::now();
            if now.duration_since(activity.last_frame) >= Duration::from_millis(180) {
                activity.last_frame = now;
                activity.frame = (activity.frame + 1) % FLOWERS.len();
                self.render()?;
            }
        }
        Ok(())
    }

    fn stop_work(&mut self) -> io::Result<()> {
        if self.activity.take().is_some() {
            self.render()?;
        }
        Ok(())
    }
}

impl Write for Screen {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.full && !self.suspended {
            let old_lines = if self.scroll > 0 {
                let width = self
                    .size()
                    .0
                    .saturating_sub(4)
                    .clamp(1, 100)
                    .saturating_sub(3)
                    .max(1);
                markdown::format(
                    &strip_ansi(&String::from_utf8_lossy(&self.transcript)),
                    width,
                )
                .len()
            } else {
                0
            };
            self.transcript.extend_from_slice(buf);
            if old_lines > 0 {
                let width = self
                    .size()
                    .0
                    .saturating_sub(4)
                    .clamp(1, 100)
                    .saturating_sub(3)
                    .max(1);
                let new_lines = markdown::format(
                    &strip_ansi(&String::from_utf8_lossy(&self.transcript)),
                    width,
                )
                .len();
                self.scroll = self
                    .scroll
                    .saturating_add(new_lines.saturating_sub(old_lines));
            }
            // Rendering always reflows the visible transcript; bound retained
            // screen text independently of Pi's persistent session history.
            const MAX_TRANSCRIPT: usize = 256 * 1024;
            if self.transcript.len() > MAX_TRANSCRIPT {
                let excess = self.transcript.len() - MAX_TRANSCRIPT;
                let cut = self.transcript[excess..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map(|offset| excess + offset + 1)
                    .unwrap_or(excess);
                self.transcript.drain(..cut);
                self.scroll = 0;
            }
            Ok(buf.len())
        } else {
            self.out.write(buf)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.full && !self.suspended {
            self.render()
        } else {
            self.out.flush()
        }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.suspend();
    }
}

/// Keep a full page of transcript visible at the oldest position. Limiting
/// scrolling to `total - 1` instead would leave a mostly empty viewport.
#[derive(Debug, PartialEq, Eq)]
struct Viewport {
    start: usize,
    end: usize,
    scroll: usize,
}

fn transcript_viewport(total: usize, visible: usize, requested_scroll: usize) -> Viewport {
    if visible == 0 {
        return Viewport {
            start: 0,
            end: 0,
            scroll: 0,
        };
    }
    let scroll = requested_scroll.min(total.saturating_sub(visible));
    let end = total - scroll;
    Viewport {
        start: end.saturating_sub(visible),
        end,
        scroll,
    }
}

pub(super) fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for part in bytes.chunks(3) {
        let a = part[0];
        let b = *part.get(1).unwrap_or(&0);
        let c = *part.get(2).unwrap_or(&0);
        encoded.push(TABLE[(a >> 2) as usize] as char);
        encoded.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        encoded.push(if part.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        encoded.push(if part.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}
fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    text.chars().take(width - 1).collect::<String>() + "…"
}
fn clip_tail(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_owned();
    }
    chars[chars.len() - width..].iter().collect()
}
fn strip_ansi(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.next() == Some('[') {
                for part in chars.by_ref() {
                    if ('@'..='~').contains(&part) {
                        break;
                    }
                }
            }
        } else if c != '\r' {
            result.push(c)
        }
    }
    result
}

/// Visual input rows, preserving explicit newlines and the insertion row at
/// an exact wrap boundary. Draft bytes sent to Pi are never reflowed.
fn draft_lines(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = vec![String::new()];
    let mut column = 0;
    for ch in text.chars() {
        if ch == '\n' {
            rows.push(String::new());
            column = 0;
        } else {
            rows.last_mut().unwrap().push(ch);
            column += 1;
            if column == width {
                rows.push(String::new());
                column = 0;
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_paste_recognizes_alt_and_ctrl_terminal_encodings() {
        for sequence in [
            b"v".as_slice(),
            b"[118;3u",
            b"[118;5u",
            b"[27;3;118~",
            b"[27;5;118~",
            b"[118;3:1u",
        ] {
            let (send, recv) = std::sync::mpsc::channel();
            for byte in sequence {
                send.send(*byte).unwrap();
            }
            assert_eq!(escape_key(&recv), Navigation::PasteImage);
        }
        let (send, recv) = std::sync::mpsc::channel();
        for byte in b"[118;3:3u" {
            send.send(*byte).unwrap();
        }
        assert_eq!(escape_key(&recv), Navigation::Other); // key release must not paste again
    }

    #[test]
    fn multiline_input_preserves_newlines_and_backspace() {
        for newline in [
            b"\x1b[13;2u".as_slice(),
            b"\x1b[27;2;13~",
            b"\x1b[13;5u",
            b"\x1b[27;5;13~",
            b"\n",
        ] {
            let (send, recv) = std::sync::mpsc::channel();
            for byte in b"first".iter().chain(newline).chain(b"second\x7f!\r") {
                send.send(*byte).unwrap();
            }
            let mut screen = Screen::new(false).unwrap();
            assert_eq!(
                screen.prompt(&recv).unwrap().as_deref(),
                Some("first\nsecon!")
            );
        }
    }

    #[test]
    fn attachment_cancellation_limits_and_command_guard_preserve_drafts() {
        let mut screen = Screen::new(false).unwrap();
        let image = serde_json::json!({"type":"image","mimeType":"image/png","data":"test"});
        screen.images = vec![image.clone(); 4];
        screen.paste_image().unwrap();
        assert!(screen.clipboard.is_none());
        assert!(screen.clipboard_notice.contains("Maximum 4"));
        screen.clear_images();
        screen.poll_clipboard().unwrap();
        assert!(screen.images.is_empty());

        screen.images.push(image.clone());
        let (send, recv) = std::sync::mpsc::channel();
        for byte in b"/new\r\x18\r" {
            send.send(*byte).unwrap();
        }
        assert_eq!(screen.prompt(&recv).unwrap().as_deref(), Some("/new"));
        assert!(screen.images.is_empty());

        screen.images.push(image);
        let (send, recv) = std::sync::mpsc::channel();
        for byte in b"\x03kept\r" {
            send.send(*byte).unwrap();
        }
        assert_eq!(screen.prompt(&recv).unwrap().as_deref(), Some("kept"));
        assert!(screen.images.is_empty());
    }

    #[test]
    fn composer_wraps_grows_to_five_rows_and_scrolls_independently() {
        assert_eq!(draft_lines("abc\ndef", 4), ["abc", "def"]);
        assert_eq!(draft_lines("abcdef", 3), ["abc", "def", ""]);
        let mut screen = Screen::new(false).unwrap();
        screen.draft = "1\n2\n3\n4\n5\n6\n7".into();
        screen.input_navigation(Navigation::Up).unwrap();
        assert_eq!(screen.draft_scroll, 1);
        screen
            .input_navigation(Navigation::MouseScroll(3, 19))
            .unwrap();
        assert_eq!(screen.draft_scroll, 2);
        assert_eq!(screen.scroll, 0);
        screen
            .input_navigation(Navigation::MouseScroll(3, 5))
            .unwrap();
        assert_eq!(screen.scroll, 3);
        screen.input_navigation(Navigation::Down).unwrap();
        assert_eq!(screen.draft_scroll, 1);
        screen.input_navigation(Navigation::Newline).unwrap();
        assert_eq!(screen.draft_scroll, 0);
        assert!(screen.draft.ends_with('\n'));
    }

    #[test]
    fn next_user_turn_has_exactly_one_blank_separator() {
        let mut screen = Screen::new(false).unwrap();
        screen.full = true;
        screen.suspended = true;
        screen.transcript = "hibi › answer\n\n\n".as_bytes().to_vec();
        screen.user("next").unwrap();
        assert_eq!(
            String::from_utf8(screen.transcript.clone()).unwrap(),
            "hibi › answer\n\nyou › next\n"
        );
    }

    #[test]
    fn transcript_scroll_stops_at_a_full_page() {
        assert_eq!(
            transcript_viewport(120, 40, 500),
            Viewport {
                start: 0,
                end: 40,
                scroll: 80
            }
        );
        assert_eq!(
            transcript_viewport(120, 40, 30),
            Viewport {
                start: 50,
                end: 90,
                scroll: 30
            }
        );
        assert_eq!(
            transcript_viewport(40, 40, 1),
            Viewport {
                start: 0,
                end: 40,
                scroll: 0
            }
        );
        assert_eq!(
            transcript_viewport(6, 40, 500),
            Viewport {
                start: 0,
                end: 6,
                scroll: 0
            }
        );
        assert_eq!(
            transcript_viewport(120, 0, 500),
            Viewport {
                start: 0,
                end: 0,
                scroll: 0
            }
        );
    }

    #[test]
    fn flower_heartbeat_advances_without_provider_output_and_clears_on_stop() {
        let mut screen = Screen::new(false).unwrap();
        screen.full = true;
        screen.suspended = true; // test the state machine without writing ANSI to stdout
        screen.start_work().unwrap();
        assert_eq!(screen.activity.as_ref().unwrap().phase, "Working…");
        screen.activity.as_mut().unwrap().last_frame -= Duration::from_millis(200);
        screen.tick_work().unwrap();
        assert_eq!(screen.activity.as_ref().unwrap().frame, 1);
        screen.set_work("Running read…").unwrap();
        assert_eq!(screen.activity.as_ref().unwrap().phase, "Running read…");
        screen.stop_work().unwrap();
        assert!(screen.activity.is_none());
    }

    #[test]
    fn clips_long_status_and_wraps_transcript() {
        assert_eq!(clip("abcdefgh", 5), "abcd…");
        assert_eq!(markdown::format("abcdef", 3).len(), 2);
        assert_eq!(strip_ansi("\x1b[35mhello\x1b[0m"), "hello");
    }

    #[test]
    fn unnamed_session_shows_short_id_instead_of_long_filename() {
        let mut screen = Screen::new(false).unwrap();
        screen
            .status(&serde_json::json!({
                "model": {"provider":"openai-codex","id":"gpt-5.5"},
                "sessionFile":"/tmp/2026-09-24T04-02-39-644Z_01a0d19f-699b-72d7-88e2-4.jsonl",
                "sessionId":"01a0d19f-699b-72d7-88e2-4"
            }))
            .unwrap();
        assert_eq!(screen.session, "01a0d19f");
        assert_eq!(screen.model, "openai-codex/gpt-5.5");
    }

    #[test]
    fn slash_suggestions_complete_and_follow_arrows() {
        fn input(bytes: &[u8]) -> String {
            let (send, recv) = std::sync::mpsc::channel();
            for key in bytes {
                send.send(*key).unwrap();
            }
            Screen::new(false).unwrap().prompt(&recv).unwrap().unwrap()
        }
        assert_eq!(input(b"/m\r"), "/models");
        assert_eq!(input(b"/lo\x1b[B\r"), "/logout");
        assert_eq!(input(b"/mo\t\r"), "/models");
        assert_eq!(input(b"/mx\x7f\r"), "/models");
        assert_eq!(input(b"hello\r"), "hello");
    }

    #[test]
    fn escape_dismisses_suggestions_without_discarding_input() {
        let (quick, quick_events) = std::sync::mpsc::channel();
        for key in b"/m\x1b\r" {
            quick.send(*key).unwrap();
        }
        assert_eq!(
            Screen::new(false)
                .unwrap()
                .prompt(&quick_events)
                .unwrap()
                .as_deref(),
            Some("/m")
        );
        let (send, recv) = std::sync::mpsc::channel();
        for key in b"/m\x1b" {
            send.send(*key).unwrap();
        }
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            send.send(b'\r').unwrap();
        });
        let mut screen = Screen::new(false).unwrap();
        assert_eq!(screen.prompt(&recv).unwrap().as_deref(), Some("/m"));
    }

    #[test]
    fn update_notice_waits_for_a_started_draft_to_finish() {
        let (keys, events) = std::sync::mpsc::channel();
        let (notify, updates) = std::sync::mpsc::channel();
        let mut screen = Screen::new(false).unwrap();
        screen.draft = "half-written".into();
        notify.send(Some("v0.2.0".into())).unwrap();
        keys.send(b'\r').unwrap();
        assert_eq!(
            screen.read_prompt(&events, Some(&updates)).unwrap(),
            PromptAction::Text(Some("half-written".into()))
        );
        assert_eq!(
            screen.prompt_with_update(&events, Some(&updates)).unwrap(),
            PromptAction::Update("v0.2.0".into())
        );
    }

    #[test]
    fn page_keys_do_not_appear_in_prompt() {
        let (send, recv) = std::sync::mpsc::channel();
        for key in b"\x1b[5~\x1b[<64;10;10Mhello\r" {
            send.send(*key).unwrap();
        }
        let mut screen = Screen::new(false).unwrap();
        assert_eq!(screen.prompt(&recv).unwrap().as_deref(), Some("hello"));
        assert!(screen.scroll > 0);
    }

    #[test]
    fn oauth_url_is_short_clickable_label_and_clipboard_encoding_is_complete() {
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
        let mut screen = Screen::new(false).unwrap();
        assert!(!screen
            .set_auth_url("https://bad.example/\u{1b}]52;")
            .unwrap());
        screen
            .set_auth_url("https://auth.openai.com/oauth/authorize?state=test")
            .unwrap();
        let line = &markdown::format("  · Open Codex sign-in ↗ (Ctrl+Y to copy)", 80)[0];
        let styled = screen.styled_line(line, 80);
        assert!(styled.contains("\x1b]8;;https://auth.openai.com/oauth/authorize?state=test\x1b\\Open Codex sign-in ↗\x1b]8;;\x1b\\"));
        assert!(!screen
            .transcript
            .windows(b"state=test".len())
            .any(|part| part == b"state=test"));
        let unrelated = &markdown::format("hibi › Open Codex sign-in ↗", 80)[1];
        assert!(!screen.styled_line(unrelated, 80).contains("\x1b]8;;"));
        screen.clear_auth_url().unwrap();
        assert!(!screen.styled_line(line, 80).contains("\x1b]8;;"));
    }

    #[test]
    fn activity_and_diff_use_theme_colors_when_enabled() {
        let mut screen = Screen::new(false).unwrap();
        screen.color = true;
        screen.full = true;
        screen.suspended = true;
        screen.start_work().unwrap();
        assert!(screen
            .accent(screen.status_style(), "✾ Thinking…")
            .contains("\x1b[1;38;2;236;74;125m"));
        let rows = markdown::format(
            "  · reasoning · 3s\n  · diff · updated.md\n    -12 old\n    +12 new",
            80,
        );
        assert!(screen
            .styled_line(&rows[0], 80)
            .contains("\x1b[1;38;2;236;74;125m"));
        assert!(screen.styled_line(&rows[2], 80).contains("48;2;65;35;49"));
        assert!(screen.styled_line(&rows[3], 80).contains("48;2;36;55;44"));
        screen.stop_work().unwrap();
        assert_eq!(screen.status_style(), "2");
    }

    #[test]
    fn picker_fits_narrow_terminal_and_marks_active_model() {
        let mut screen = Screen::new(false).unwrap();
        let items = (0..8)
            .map(|n| format!("very-long-model-identifier-{n}"))
            .collect();
        screen.picker = Some(Picker::new("Models · demo".into(), items, Some(6), 5));
        let (top, rows) = screen.picker_rows(36, 12, "  ").unwrap();
        assert_eq!(top, 4); // panel docks to the bottom, not over the middle of chat
        assert_eq!(rows.len(), 8);
        assert!(rows
            .iter()
            .all(|line| line.starts_with("  ") && line.chars().count() <= 38));
        assert!(rows.iter().any(|line| line.contains("● current")));
        assert!(rows.iter().any(|line| line.contains('█')));
    }

    #[test]
    fn user_tint_covers_wrapped_rows_and_margins_without_leaking() {
        let mut screen = Screen::new(false).unwrap();
        screen.color = true;
        let rows = markdown::format(
            "you › a long user message\nsecond line\nhibi › **reply**",
            10,
        );
        for row in rows.iter().filter(|row| row.role == Role::User) {
            let rendered = screen.transcript_line(row, 13, 19, "   ");
            assert!(rendered.starts_with("\x1b[48;2;57;30;46m   "));
            assert!(rendered.ends_with("\x1b[0m"));
            assert_eq!(strip_ansi(&rendered).chars().count(), 19);
        }
        for row in rows.iter().filter(|row| row.role == Role::Assistant) {
            assert!(!screen.transcript_line(row, 13, 19, "   ").contains("48;2"));
        }
        assert!(rows
            .iter()
            .any(|row| screen.styled_line(row, 10).contains("hibi")));
        screen.color = false;
        assert!(!screen
            .transcript_line(&rows[0], 13, 19, "   ")
            .contains('\x1b'));
    }

    #[test]
    fn pink_highlights_markdown_but_no_color_keeps_clean_text() {
        let line = &markdown::format("hibi › **pink** `code`", 80)[1];
        let mut screen = Screen::new(false).unwrap();
        assert_eq!(screen.styled_line(line, 80), "pink code");
        screen.color = true;
        assert!(screen
            .styled_line(line, 80)
            .contains("\x1b[1;38;2;255;220;230m"));
        assert!(screen
            .styled_line(line, 80)
            .contains("\x1b[38;2;255;173;197m"));
    }
}
