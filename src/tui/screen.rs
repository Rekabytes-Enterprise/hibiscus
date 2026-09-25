use serde_json::Value;
use std::env;
use std::io::{self, IsTerminal, Write};
use std::os::fd::AsRawFd;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::{
    activity::Timeline,
    markdown::{self, Role, Tone},
    modal::{self, Modal},
    paint::{Cursor, Painter, SYNC_END},
    picker::Picker,
};
use crate::{chat::commands, Result};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PromptAction {
    Text(Option<String>),
    Update(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueueMode {
    Steer,
    FollowUp,
}

pub(crate) struct DraftSubmission {
    pub(crate) text: String,
    pub(crate) images: Vec<Value>,
    pub(crate) mode: QueueMode,
}

pub(crate) enum ActiveAction {
    Stop,
    Submit(DraftSubmission),
    None,
}

struct PendingQueue {
    text: String,
    mode: QueueMode,
    images: usize,
    accepted: bool,
    sending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Navigation {
    Escape,
    EscapeWith(u8),
    ScrollPage(i32),
    ScrollLines(i32),
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    Newline,
    FollowUp,
    PasteImage,
    MouseScroll(i32, usize),
    Other,
}

pub(crate) fn escape_key(events: &Receiver<u8>) -> Navigation {
    match events.recv_timeout(std::time::Duration::from_millis(25)) {
        Ok(b'[') => {}
        Ok(b'v') => return Navigation::PasteImage, // legacy Alt+V (Pi's WSL default)
        Ok(b'\r' | b'\n') => return Navigation::FollowUp, // legacy Alt+Enter
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
        "13;3u" | "27;3;13~" | "13;3~" => Navigation::FollowUp,
        "3~" => Navigation::Delete,
        "C" => Navigation::Right,
        "D" => Navigation::Left,
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
    fn thinking_start(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn thinking_end(&mut self) -> io::Result<bool> {
        Ok(false)
    }
    fn tool_start(&mut self, _id: &str, _name: &str, _path: &str) -> io::Result<bool> {
        Ok(false)
    }
    fn tool_end(
        &mut self,
        _id: &str,
        _name: &str,
        _path: &str,
        _failed: bool,
        _diff: Option<&str>,
    ) -> io::Result<bool> {
        Ok(false)
    }
    fn goal(&mut self, _details: &Value) -> io::Result<()> {
        Ok(())
    }
    fn split_timeline(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn live_input(&self) -> bool {
        false
    }
    fn pending_key(&mut self) -> Option<u8> {
        None
    }
    fn active_key(&mut self, _byte: u8, _events: &Receiver<u8>) -> Result<ActiveAction> {
        Ok(ActiveAction::None)
    }
    fn queued(&mut self, _submission: &DraftSubmission) -> io::Result<()> {
        Ok(())
    }
    fn expect_initial_user(&mut self, _message: &str) {}
    fn delivered_user(&mut self, _message: &Value) -> io::Result<bool> {
        Ok(false)
    }
    fn hold_queue(&mut self, _submission: DraftSubmission) -> io::Result<()> {
        Ok(())
    }
    fn rejected_queue(&mut self, _submission: DraftSubmission, _error: &str) -> io::Result<()> {
        Ok(())
    }
    fn queue_update(&mut self, _update: &Value) -> io::Result<()> {
        Ok(())
    }
    fn clear_queue_display(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn invalidate_screen(&mut self) {}
    fn extension_dialog(
        &mut self,
        _event: &Value,
        _events: &Receiver<u8>,
    ) -> Result<Option<Value>> {
        Ok(None)
    }
}
impl ScrollDisplay for io::Stdout {}
impl ScrollDisplay for Vec<u8> {}

const FLOWERS: [&str; 4] = ["✿", "❀", "✾", "❁"];
// Ubuntu's aubergine terminal background. Set SGR only inside the alternate
// screen; never change the terminal's configured background with OSC 11.
const BASE_BACKGROUND: &str = "\x1b[48;2;48;10;36m";

fn paint_background(frame: &str, color: bool) -> String {
    if color {
        // The theme's accent and Markdown renderers reset foreground styles.
        // Restore the base background after each reset so blank columns and
        // subsequent erase-to-end sequences never expose the terminal default.
        format!(
            "{BASE_BACKGROUND}{}",
            frame.replace("\x1b[0m", &format!("\x1b[0m{BASE_BACKGROUND}"))
        )
    } else {
        frame.to_owned()
    }
}

struct Activity {
    phase: String,
    started: Instant,
    last_refresh: Instant,
    last_spinner: Instant,
    flower_frame: usize,
}

/// An alternate-screen transcript with a persistent composer. Non-terminal
/// streams pass through untouched; RPC responses are never written here.
pub(crate) struct Screen {
    out: io::Stdout,
    full: bool,
    suspended: bool,
    color: bool,
    painter: Painter,
    last_paint: Option<Instant>,
    paint_pending: bool,
    transcript: Vec<u8>,
    layout: markdown::Layout,
    layout_dirty: bool,
    scroll_anchor: Option<usize>,
    workspace: String,
    draft: String,
    draft_cursor: usize,
    preferred_column: Option<usize>,
    restored_draft: Option<(String, Vec<Value>)>,
    disconnected: bool,
    draft_scroll: usize,
    images: Vec<Value>,
    clipboard: Option<super::clipboard::Job>,
    clipboard_notice: String,
    input_notice: Option<String>,
    queue_steering: usize,
    queue_follow_up: usize,
    pending_queue: Vec<PendingQueue>,
    delivered_before_ack: Vec<String>,
    initial_user_pending: Option<String>,
    rejected_queued: Option<(String, Vec<Value>)>,
    active_utf8: Vec<u8>,
    model: String,
    session: String,
    scroll: usize,
    picker: Option<Picker>,
    modal: Option<Modal>,
    suggestions: Option<Picker>,
    suggestions_dismissed: bool,
    pending_input: Option<u8>,
    secret_input: bool,
    auth_url: Option<String>,
    activity: Option<Activity>,
    timelines: Vec<Timeline>,
    current_timeline: Option<usize>,
    timeline_marked: bool,
    goal: Option<(usize, usize)>,
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
            painter: Painter::new(env::var_os("HIBISCUS_NO_SYNC_UPDATE").is_none()),
            last_paint: None,
            paint_pending: false,
            transcript: Vec::new(),
            layout: markdown::Layout::default(),
            layout_dirty: true,
            scroll_anchor: None,
            workspace: env::current_dir()
                .ok()
                .and_then(|path| path.file_name().map(|part| clean(&part.to_string_lossy())))
                .unwrap_or_else(|| "workspace".into()),
            draft: String::new(),
            draft_cursor: 0,
            preferred_column: None,
            restored_draft: None,
            disconnected: false,
            draft_scroll: 0,
            images: Vec::new(),
            clipboard: None,
            clipboard_notice: String::new(),
            input_notice: None,
            queue_steering: 0,
            queue_follow_up: 0,
            pending_queue: Vec::new(),
            delivered_before_ack: Vec::new(),
            initial_user_pending: None,
            rejected_queued: None,
            active_utf8: Vec::new(),
            model: "no model".into(),
            session: "new chat".into(),
            scroll: 0,
            picker: None,
            modal: None,
            suggestions: None,
            suggestions_dismissed: false,
            pending_input: None,
            secret_input: false,
            auth_url: None,
            activity: None,
            timelines: Vec::new(),
            current_timeline: None,
            timeline_marked: false,
            goal: None,
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
            self.painter.invalidate();
            self.last_paint = None;
            write!(self.out, "{SYNC_END}\x1b[<u\x1b[?1000l\x1b[?1006l")?;
            if self.color {
                write!(self.out, "\x1b[0m")?;
            }
            write!(self.out, "\x1b[?25h\x1b[?1049l\x1b[0 q")?;
            self.suspended = true;
            self.out.flush()?;
        }
        Ok(())
    }

    pub(crate) fn resume(&mut self) -> io::Result<()> {
        if self.full && self.suspended {
            self.painter.invalidate();
            self.last_paint = None;
            // DECSCUSR 2 requests a steady block cursor. Restore the
            // terminal's preferred cursor style when leaving Hibiscus.
            write!(self.out, "\x1b[?1049h\x1b[2 q\x1b[>1u")?;
            if self.color {
                self.out.write_all(BASE_BACKGROUND.as_bytes())?;
            }
            write!(self.out, "\x1b[2J\x1b[?1000h\x1b[?1006h\x1b[?25l")?;
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
        self.draft_cursor = 0;
        self.preferred_column = None;
        self.suggestions = None;
        if self.full {
            self.dirty_transcript();
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

    /// Reset only the local view after Pi has created a new session. Persisted
    /// history belongs to Pi and is not deleted by clearing the screen.
    pub(crate) fn clear_session(&mut self) -> io::Result<()> {
        self.dirty_transcript();
        self.transcript.clear();
        self.draft.clear();
        self.draft_cursor = 0;
        self.preferred_column = None;
        self.restored_draft = None;
        self.draft_scroll = 0;
        self.scroll = 0;
        self.clear_images();
        self.suggestions = None;
        self.suggestions_dismissed = false;
        self.picker = None;
        self.modal = None;
        self.auth_url = None;
        self.secret_input = false;
        self.activity = None;
        self.timelines.clear();
        self.current_timeline = None;
        self.timeline_marked = false;
        self.goal = None;
        self.queue_steering = 0;
        self.queue_follow_up = 0;
        self.pending_queue.clear();
        self.delivered_before_ack.clear();
        self.initial_user_pending = None;
        self.rejected_queued = None;
        self.active_utf8.clear();
        self.input_notice = None;
        self.session = "new chat".into();
        self.render()
    }

    pub(crate) fn defer_input(&mut self, byte: u8) {
        self.pending_input = Some(byte);
    }

    pub(crate) fn take_rejected_queue(&mut self) -> Option<(String, Vec<Value>)> {
        self.rejected_queued.take()
    }

    pub(crate) fn restore_draft(&mut self, text: String, images: Vec<Value>) {
        self.restored_draft = Some((text, images));
    }

    pub(crate) fn restore_goal(&mut self, messages: &[Value]) -> io::Result<()> {
        self.goal = None;
        for message in messages {
            if message["role"] == "toolResult" && message["toolName"] == "goal" {
                self.set_goal(&message["details"]);
            }
        }
        self.render()
    }

    fn set_goal(&mut self, details: &Value) {
        let details = &details["hibiscusGoal"];
        if details.get("error").is_some() {
            return;
        }
        let (Some(completed), Some(total)) =
            (details["completed"].as_u64(), details["total"].as_u64())
        else {
            return;
        };
        if total <= 50 && completed <= total {
            self.goal = (total > 0).then_some((completed as usize, total as usize));
        }
    }

    fn dirty_transcript(&mut self) {
        if self.scroll > 0 && self.scroll_anchor.is_none() {
            self.scroll_anchor = Some(self.layout.len());
        }
        self.layout_dirty = true;
    }

    fn transcript_layout(&mut self, width: usize) -> std::rc::Rc<Vec<std::rc::Rc<markdown::Row>>> {
        if self.layout_dirty || self.layout.width() != width {
            self.layout.update(&self.rendered_transcript(), width);
            self.layout_dirty = false;
            if let Some(before) = self.scroll_anchor.take() {
                if self.scroll > 0 {
                    self.scroll = self
                        .scroll
                        .saturating_add(self.layout.len().saturating_sub(before));
                }
            }
        }
        self.layout.rows()
    }

    fn rendered_transcript(&self) -> String {
        let mut text = strip_ansi(&String::from_utf8_lossy(&self.transcript));
        for (index, timeline) in self.timelines.iter().enumerate() {
            text = text.replace(
                &format!("· __hibiscus_activity_{index}__"),
                &timeline.lines(),
            );
        }
        text
    }

    fn freeze_timeline(&mut self) {
        if !self.timelines.is_empty() {
            self.dirty_transcript();
        }
        if let Some(index) = self.current_timeline {
            self.timelines[index].finish();
        }
        self.current_timeline = None;
        self.timeline_marked = false;
        if self.full && !self.timelines.is_empty() {
            self.transcript = self.rendered_transcript().into_bytes();
            self.timelines.clear();
        }
    }

    fn update_timeline(&mut self, f: impl FnOnce(&mut Timeline)) -> io::Result<()> {
        if !self.full {
            return Ok(());
        }
        self.dirty_transcript();
        let index = match self.current_timeline {
            Some(index) => index,
            None => {
                let index = self.timelines.len();
                self.timelines.push(Timeline::default());
                self.current_timeline = Some(index);
                self.timeline_marked = false;
                index
            }
        };
        f(&mut self.timelines[index]);
        if !self.timeline_marked && !self.timelines[index].lines().is_empty() {
            self.transcript
                .extend_from_slice(format!("\n· __hibiscus_activity_{index}__\n").as_bytes());
            self.timeline_marked = true;
        }
        self.render()
    }

    pub(crate) fn set_disconnected(&mut self, disconnected: bool) -> io::Result<()> {
        self.disconnected = disconnected;
        self.activity = None;
        self.render()
    }

    pub(crate) fn prompt_with_update(
        &mut self,
        events: &Receiver<u8>,
        updates: Option<&Receiver<Option<String>>>,
    ) -> Result<PromptAction> {
        if let Some((text, images)) = self.restored_draft.take() {
            if !self.draft.is_empty() || !self.images.is_empty() {
                self.restored_draft = Some((text, images));
            } else {
                self.draft = text;
                self.draft_cursor = self.draft.len();
                self.images = images;
            }
        }
        self.ensure_cursor_visible();
        self.suggestions_dismissed = false;
        self.refresh_suggestions();
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
            loop {
                let byte = match events.recv_timeout(Duration::from_millis(100)) {
                    Ok(byte) => byte,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if self.painter.resized(self.size()) {
                            self.render()?;
                        }
                        continue;
                    }
                    Err(_) => break,
                };
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

    fn run_modal(
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
                        if self.painter.resized((columns, rows)) {
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
        let options =
            if self.activity.is_some() || self.draft.contains('\n') || !self.images.is_empty() {
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

    // Only batch already available printable bytes. Control/escape sequences
    // stay ordered and are handled by the existing input state machine.
    fn insert_input_batch(&mut self, first: u8, events: &Receiver<u8>, pending: &mut Vec<u8>) {
        let mut text = String::new();
        for index in 0..128 {
            let byte = if index == 0 {
                first
            } else {
                let Ok(byte) = events.try_recv() else { break };
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

    fn insert_draft(&mut self, text: &str) {
        self.input_notice = None;
        self.draft.insert_str(self.draft_cursor, text);
        self.draft_cursor += text.len();
        self.preferred_column = None;
        self.ensure_cursor_visible();
    }

    fn ensure_cursor_visible(&mut self) {
        let width = self
            .size()
            .0
            .saturating_sub(4)
            .clamp(1, 100)
            .saturating_sub(5)
            .max(1);
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

    fn move_draft(&mut self, key: Navigation) -> io::Result<()> {
        let width = self
            .size()
            .0
            .saturating_sub(4)
            .clamp(1, 100)
            .saturating_sub(5)
            .max(1);
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
        if key == Navigation::Delete || self.suggestions.is_some() {
            self.suggestions_dismissed = true;
            self.refresh_suggestions();
        }
        self.ensure_cursor_visible();
        self.render()
    }

    fn backspace_draft(&mut self) -> io::Result<()> {
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

    fn send_active_draft(&mut self, mode: QueueMode) -> io::Result<ActiveAction> {
        if self.clipboard.is_some() {
            self.input_notice = Some("Reading clipboard… press Enter when ready".into());
        } else if self.draft.trim_start().starts_with('/') {
            self.input_notice =
                Some("Commands are available after Pi settles; keep this draft or clear it".into());
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

    fn input_navigation(&mut self, key: Navigation) -> io::Result<()> {
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
            Navigation::Up | Navigation::Down if self.suggestions.is_none() => {
                return self.move_draft(key);
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
        if self.full {
            let modal = Modal::input(
                "Codex authorization",
                String::new(),
                "Enter authorization response".into(),
                secret,
            );
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

    fn read_prompt(
        &mut self,
        events: &Receiver<u8>,
        mut updates: Option<&Receiver<Option<String>>>,
    ) -> Result<PromptAction> {
        let mut pending = std::mem::take(&mut self.active_utf8);
        let mut queued = self.pending_input.take();
        loop {
            self.poll_clipboard()?;
            if self.full && !self.suspended && self.painter.resized(self.size()) {
                self.render()?;
            }
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
            let byte = match queued.take().or_else(|| self.pending_input.take()) {
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
                    self.draft_cursor = 0;
                    self.preferred_column = None;
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
                    self.draft_cursor = 0;
                    self.preferred_column = None;
                    self.clear_images();
                    pending.clear();
                    self.suggestions_dismissed = false;
                    self.refresh_suggestions();
                    self.render()?;
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
                    if let Some(picker) = self.suggestions.as_ref() {
                        if let Some(cmd) = commands::matches(&self.draft).get(picker.selected) {
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

    fn picker_rows(
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
            picker.move_by(0, height.saturating_sub(3));
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

    fn render_stream(&mut self) -> io::Result<()> {
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
        if self.painter.resized((columns, rows)) && !self.secret_input {
            self.ensure_cursor_visible();
        }
        let width = columns.saturating_sub(4).clamp(1, 100);
        let left = " ".repeat(columns.saturating_sub(width) / 2);
        let workspace = &self.workspace;
        let flower = self
            .activity
            .as_ref()
            .map_or(FLOWERS[0], |activity| FLOWERS[activity.flower_frame]);
        let title = clip(&format!("{} hibiscus   /   {workspace}", FLOWERS[0]), width);
        let metadata = format!("{}  ·  {}", self.model, self.session);
        let header = clip(&metadata, width);
        let lines = self.transcript_layout(width.saturating_sub(3).max(1));
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
        let available = rows.saturating_sub(8 + draft_height + pending_height);
        let modal = self.picker_rows(width, available, &left);
        let transcript_rows = modal.as_ref().map_or(available, |(top, _)| *top);
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
            .map(|((row, column), (top, _))| (top + row + 4, left.len() + column + 1));
        let cursor_visible = if self.modal.is_some() {
            modal_cursor.is_some()
        } else {
            self.picker.is_none() && (draft_start..draft_end).contains(&cursor_line)
        };
        let mut frame = String::new();
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
            "Pi disconnected · /reconnect · /restore · /quit".into()
        } else if self.suggestions.is_some() {
            "↑↓ choose  ·  Tab complete  ·  Enter run  ·  Esc dismiss".into()
        } else if self.auth_url.is_some() {
            if cfg!(target_os = "macos") {
                "Browser sign-in  ·  Ctrl+Y copy link  ·  Esc cancel".into()
            } else {
                "Open Codex sign-in ↗  ·  Ctrl+Y copy link  ·  Esc cancel".into()
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
            "{left}{}\x1b[K",
            self.accent(self.status_style(), &hint)
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
        self.paint_pending = false;
        Ok(())
    }
}

impl ScrollDisplay for Screen {
    fn invalidate_screen(&mut self) {
        self.painter.invalidate();
        self.last_paint = None;
    }
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
            || (self.paint_pending
                && self
                    .last_paint
                    .is_none_or(|last| last.elapsed() >= Duration::from_millis(33)))
        {
            self.render()?;
        }
        Ok(())
    }

    fn stop_work(&mut self) -> io::Result<()> {
        self.freeze_timeline();
        self.queue_steering = 0;
        self.queue_follow_up = 0;
        self.pending_queue.clear();
        self.delivered_before_ack.clear();
        self.initial_user_pending = None;
        if self.input_notice.as_deref().is_some_and(|notice| {
            notice.contains("accepted by Pi")
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
        Ok(self.full)
    }
    fn tool_start(&mut self, id: &str, name: &str, path: &str) -> io::Result<bool> {
        if !self.full {
            return Ok(false);
        }
        self.update_timeline(|timeline| timeline.tool_start(id, name, path))?;
        Ok(true)
    }
    fn tool_end(
        &mut self,
        id: &str,
        name: &str,
        path: &str,
        failed: bool,
        diff: Option<&str>,
    ) -> io::Result<bool> {
        if !self.full {
            return Ok(false);
        }
        self.update_timeline(|timeline| timeline.tool_end(id, name, path, failed, diff))?;
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
        match byte {
            b'\r' => return Ok(self.send_active_draft(QueueMode::Steer)?),
            17 => return Ok(self.send_active_draft(QueueMode::FollowUp)?), // Ctrl+Q (WSL)
            b'\n' => self.input_navigation(Navigation::Newline)?,
            22 => self.paste_image()?,
            24 => {
                self.clear_images();
                self.render()?;
            }
            3 => {
                self.draft.clear();
                self.draft_cursor = 0;
                self.preferred_column = None;
                self.clear_images();
                self.active_utf8.clear();
                self.render()?;
            }
            8 | 127 => self.backspace_draft()?,
            4 => self.input_navigation(Navigation::Delete)?,
            1 => self.input_navigation(Navigation::Home)?,
            5 => self.input_navigation(Navigation::End)?,
            2 => self.input_navigation(Navigation::Left)?,
            6 => self.input_navigation(Navigation::Right)?,
            27 => match escape_key(events) {
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
            Some("Pi is starting; press Enter again after it accepts the prompt".into());
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
        self.input_notice = Some(format!("{kind} accepted by Pi · waiting for delivery"));
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
        self.input_notice = Some(format!(
            "Pi rejected queued message: {} · /restore after run",
            crate::pi::error::safe_message(error)
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
                // shrink: Pi drains the oldest first, and events have no IDs.
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
            item.sending = true; // removed from Pi's queue, awaiting message_start
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

fn user_content(content: &Value) -> (String, usize) {
    match content {
        Value::String(text) => (text.clone(), 0),
        Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let images = blocks
                .iter()
                .filter(|block| block["type"] == "image")
                .count();
            (text, images)
        }
        _ => (String::new(), 0),
    }
}

fn follow_up_key() -> &'static str {
    if super::clipboard::paste_key() == "Alt+V" {
        "Ctrl+Q"
    } else {
        "Alt+Enter"
    }
}

fn working_status(
    width: usize,
    flower: &str,
    phase: &str,
    seconds: u64,
    scrolled: &str,
    goal: Option<(usize, usize)>,
) -> String {
    let left = format!("{flower} {phase} · {seconds}s · Esc stop{scrolled}");
    let Some((completed, total)) = goal.filter(|(_, total)| *total > 0) else {
        return clip(&left, width);
    };
    let right = if width >= 60 {
        goal_line(width, completed, total)
    } else {
        format!("Goal {completed}/{total} · {}%", completed * 100 / total)
    };
    let space = width.saturating_sub(right.chars().count() + 1);
    if space < 5 {
        return clip(&left, width);
    }
    let left = clip(&left, space);
    format!(
        "{left}{}{right}",
        " ".repeat(width.saturating_sub(left.chars().count() + right.chars().count()))
    )
}

fn goal_line(width: usize, completed: usize, total: usize) -> String {
    let percent = completed * 100 / total;
    let suffix = format!("  {completed}/{total} · {percent}%");
    let length = width
        .saturating_sub("Goal  ".len() + suffix.chars().count())
        .clamp(1, 20);
    let filled = completed * length / total;
    clip(
        &format!(
            "Goal  {}{}{}",
            "█".repeat(filled),
            "░".repeat(length - filled),
            suffix
        ),
        width,
    )
}

impl Write for Screen {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.full && !self.suspended {
            self.dirty_transcript();
            self.transcript.extend_from_slice(buf);
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
            if self.activity.is_some() {
                self.render_stream()
            } else {
                self.render()
            }
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
fn draft_cursor_position(text: &str, width: usize, cursor: usize) -> (usize, usize) {
    let (mut row, mut col) = (0, 0);
    for (index, ch) in text.char_indices() {
        if index >= cursor {
            break;
        }
        if ch == '\n' {
            row += 1;
            col = 0;
        } else {
            col += 1;
            if col == width.max(1) {
                row += 1;
                col = 0;
            }
        }
    }
    (row, col)
}

fn draft_index_on_row(text: &str, width: usize, row: usize, column: usize) -> Option<usize> {
    let (mut current, mut col, mut best) = (0, 0, None);
    for (index, ch) in text.char_indices() {
        if current == row && col <= column {
            best = Some(index);
        }
        if current > row {
            return best;
        }
        if ch == '\n' {
            current += 1;
            col = 0;
        } else {
            col += 1;
            if col == width.max(1) {
                current += 1;
                col = 0;
            }
        }
    }
    if current == row && col <= column {
        best = Some(text.len());
    }
    best
}

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
    fn layout_reuses_rows_for_typing_and_coalesces_scrolled_changes() {
        let mut screen = Screen::new(false).unwrap();
        screen.transcript = "hibi › first\nsecond\nthird\n".as_bytes().to_vec();
        let original = screen.transcript_layout(73);
        screen.insert_draft("typing does not change transcript");
        assert!(std::rc::Rc::ptr_eq(
            &original,
            &screen.transcript_layout(73)
        ));
        screen.scroll = 2;
        for line in ["fourth\n", "fifth\n"] {
            screen.dirty_transcript();
            screen.transcript.extend_from_slice(line.as_bytes());
        }
        assert_eq!(screen.scroll, 2, "scroll counting is deferred until layout");
        assert_eq!(screen.transcript_layout(73).len(), original.len() + 2);
        assert_eq!(screen.scroll, 4);
        screen.transcript_layout(73);
        assert_eq!(screen.scroll, 4, "do not apply an anchor twice");
        screen.clear_session().unwrap();
        assert!(screen.transcript_layout(73).is_empty());
        assert_eq!(screen.scroll, 0);
    }

    #[test]
    fn printable_batches_are_bounded_utf8_safe_and_stop_before_control_keys() {
        let mut screen = Screen::new(false).unwrap();
        let (send, events) = std::sync::mpsc::channel();
        let text = "é".repeat(70);
        for byte in text.as_bytes().iter().skip(1).chain(b"\rMORE") {
            send.send(*byte).unwrap();
        }
        let mut pending = Vec::new();
        screen.insert_input_batch(text.as_bytes()[0], &events, &mut pending);
        assert_eq!(screen.draft.chars().count(), 64);
        screen.insert_input_batch(events.recv().unwrap(), &events, &mut pending);
        assert_eq!(screen.draft, text);
        assert!(pending.is_empty());
        assert_eq!(screen.pending_input.take(), Some(b'\r'));
        assert_eq!(events.recv().unwrap(), b'M');
    }

    #[test]
    fn new_session_clears_transcript_and_transient_input_but_keeps_model() {
        let mut screen = Screen::new(false).unwrap();
        screen.transcript = b"old conversation".to_vec();
        screen.draft = "old draft".into();
        screen.restore_draft(
            "failed prompt".into(),
            vec![serde_json::json!({"type":"image"})],
        );
        screen.images.push(serde_json::json!({"type":"image"}));
        screen.clipboard_notice = "old clipboard notice".into();
        screen.auth_url = Some("https://example.invalid".into());
        screen.scroll = 20;
        screen.draft_scroll = 3;
        screen.secret_input = true;
        screen.session = "old session".into();
        screen.model = "provider/model".into();
        screen.suggestions = Some(Picker::new("Commands".into(), vec!["/new".into()], None, 5));
        screen.clear_session().unwrap();
        assert!(screen.transcript.is_empty());
        assert!(screen.draft.is_empty());
        assert!(screen.restored_draft.is_none());
        assert!(screen.images.is_empty());
        assert!(screen.clipboard_notice.is_empty());
        assert!(screen.auth_url.is_none());
        assert!(screen.suggestions.is_none());
        assert!(!screen.secret_input);
        assert_eq!(screen.scroll, 0);
        assert_eq!(screen.draft_scroll, 0);
        assert_eq!(screen.session, "new chat");
        assert_eq!(screen.model, "provider/model");
    }

    #[test]
    fn restored_failed_draft_keeps_images_and_waits_for_explicit_submission() {
        let mut screen = Screen::new(false).unwrap();
        let images = vec![serde_json::json!({"type":"image","data":"test","mimeType":"image/png"})];
        screen.restore_draft("original\ntext".into(), images.clone());
        let (send, recv) = std::sync::mpsc::channel();
        for byte in b" edited\r" {
            send.send(*byte).unwrap();
        }
        assert_eq!(
            screen.prompt(&recv).unwrap().as_deref(),
            Some("original\ntext edited")
        );
        assert_eq!(screen.take_images(), images);
        assert!(screen.restored_draft.is_none());
        screen.set_disconnected(true).unwrap();
        assert!(screen.disconnected);
        screen.set_disconnected(false).unwrap();
        assert!(!screen.disconnected);
    }

    #[test]
    fn follow_up_shortcuts_are_distinct_from_ctrl_enter_newlines() {
        for sequence in [b"\r".as_slice(), b"[13;3u", b"[27;3;13~"] {
            let (send, rx) = std::sync::mpsc::channel();
            for byte in sequence {
                send.send(*byte).unwrap();
            }
            assert_eq!(escape_key(&rx), Navigation::FollowUp);
        }
        let (send, rx) = std::sync::mpsc::channel();
        for byte in b"[13;5u" {
            send.send(*byte).unwrap();
        }
        assert_eq!(escape_key(&rx), Navigation::Newline);
    }

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
    fn pi_queue_updates_wait_for_user_delivery_with_duplicate_text_and_late_acks() {
        let mut screen = Screen::new(false).unwrap();
        screen.full = true;
        screen.suspended = true;
        screen.user("initial").unwrap();
        screen.expect_initial_user("initial");
        assert!(!screen
            .delivered_user(&serde_json::json!({"content":"initial"}))
            .unwrap());
        screen
            .queue_update(&serde_json::json!({"steering":["same","same"],"followUp":["later"]}))
            .unwrap();
        assert_eq!(screen.pending_queue.len(), 3);
        assert!(screen.pending_queue.iter().all(|item| !item.sending));
        let first = DraftSubmission {
            text: "same".into(),
            images: vec![serde_json::json!({"type":"image"})],
            mode: QueueMode::Steer,
        };
        screen.queued(&first).unwrap();
        assert_eq!(screen.pending_queue[0].images, 1);
        assert_eq!(
            String::from_utf8_lossy(&screen.transcript)
                .matches("same")
                .count(),
            0
        );
        screen
            .queue_update(&serde_json::json!({"steering":["same"],"followUp":["later"]}))
            .unwrap();
        assert_eq!(
            screen
                .pending_queue
                .iter()
                .filter(|item| item.text == "same" && item.sending)
                .count(),
            1
        );
        screen.delivered_user(&serde_json::json!({"content":[{"type":"text","text":"same"},{"type":"image","data":"private-base64"}]})).unwrap();
        assert_eq!(
            screen
                .pending_queue
                .iter()
                .filter(|item| item.text == "same")
                .count(),
            1
        );
        assert!(!String::from_utf8_lossy(&screen.transcript).contains("private-base64"));
        screen
            .queued(&DraftSubmission {
                text: "same".into(),
                images: vec![],
                mode: QueueMode::Steer,
            })
            .unwrap();
        screen
            .queue_update(&serde_json::json!({"steering":[],"followUp":["later"]}))
            .unwrap();
        screen
            .delivered_user(&serde_json::json!({"content":"same"}))
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&screen.transcript)
                .matches("you › same")
                .count(),
            2
        );
        screen
            .queue_update(&serde_json::json!({"steering":[],"followUp":[]}))
            .unwrap();
        screen
            .delivered_user(&serde_json::json!({"content":"later"}))
            .unwrap();
        screen
            .queued(&DraftSubmission {
                text: "later".into(),
                images: vec![],
                mode: QueueMode::FollowUp,
            })
            .unwrap();
        assert!(
            screen.pending_queue.is_empty(),
            "late acknowledgement must not recreate an already delivered message"
        );
        assert_eq!(
            String::from_utf8_lossy(&screen.transcript)
                .matches("you › initial")
                .count(),
            1
        );
        assert_eq!(
            String::from_utf8_lossy(&screen.transcript)
                .matches("you › later")
                .count(),
            1
        );
    }

    #[test]
    fn live_composer_keeps_editing_and_stages_only_explicit_queue_submissions() {
        let mut screen = Screen::new(false).unwrap();
        screen.full = true;
        screen.suspended = true;
        screen.start_work().unwrap();
        let (_send, events) = std::sync::mpsc::channel();
        for byte in b"hello" {
            assert!(matches!(
                screen.active_key(*byte, &events).unwrap(),
                ActiveAction::None
            ));
        }
        screen.input_navigation(Navigation::Left).unwrap();
        screen.active_key(b'!', &events).unwrap();
        screen
            .images
            .push(serde_json::json!({"type":"image","mimeType":"image/png","data":"test"}));
        let ActiveAction::Submit(submission) = screen.active_key(17, &events).unwrap() else {
            panic!("Ctrl+Q should queue a follow-up");
        };
        assert_eq!(submission.text, "hell!o");
        assert_eq!(submission.mode, QueueMode::FollowUp);
        assert_eq!(submission.images.len(), 1);
        assert!(screen.draft.is_empty() && screen.images.is_empty());
        screen.active_key(b'k', &events).unwrap();
        screen.rejected_queue(submission, "queue denied").unwrap();
        assert_eq!(screen.draft, "k", "a newer draft must not be overwritten");
        assert_eq!(screen.take_rejected_queue().unwrap().0, "hell!o");
        screen.active_key(3, &events).unwrap();
        for byte in b"/new" {
            screen.active_key(*byte, &events).unwrap();
        }
        assert!(matches!(
            screen.active_key(b'\r', &events).unwrap(),
            ActiveAction::None
        ));
        assert_eq!(
            screen.draft, "/new",
            "local commands cannot be sent as steering"
        );
    }

    #[test]
    fn composer_edits_mid_string_with_arrows_and_utf8_boundaries() {
        fn type_keys(bytes: &[u8]) -> String {
            let (send, recv) = std::sync::mpsc::channel();
            for byte in bytes {
                send.send(*byte).unwrap();
            }
            Screen::new(false).unwrap().prompt(&recv).unwrap().unwrap()
        }
        assert_eq!(type_keys(b"abCD\x1b[D\x1b[D-\x1b[C\x1b[3~\r"), "ab-C");
        assert_eq!(type_keys("aé文\x1b[D\x7fß\r".as_bytes()), "aß文");
        assert_eq!(type_keys(b"hello\x01Hi \x05!\r"), "Hi hello!");
        assert_eq!(
            type_keys(b"one\ntwo\x1b[Hstart \x1b[F end\r"),
            "one\nstart two end"
        );
        assert_eq!(type_keys(b"abc\x1b[D\x04\r"), "ab");
    }

    #[test]
    fn cursor_follows_wrapping_and_multiline_navigation_without_animation() {
        assert_eq!(draft_cursor_position("abcdef", 3, 3), (1, 0));
        assert_eq!(draft_cursor_position("abc\ndef", 3, 4), (2, 0));
        assert_eq!(draft_index_on_row("abcdef", 3, 1, 2), Some(5));
        let mut screen = Screen::new(false).unwrap();
        screen.draft = "one\ntwo\nthree\nfour\nfive\nsix\nseven".into();
        screen.draft_cursor = screen.draft.len();
        for _ in 0..5 {
            screen.input_navigation(Navigation::Up).unwrap();
        }
        assert_eq!(
            draft_cursor_position(&screen.draft, 75, screen.draft_cursor).0,
            1
        );
        assert!(screen.draft_scroll > 0);
        screen.input_navigation(Navigation::Right).unwrap();
        assert_eq!(screen.preferred_column, None);
        screen.input_navigation(Navigation::End).unwrap();
        assert!(screen.draft.is_char_boundary(screen.draft_cursor));
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
        screen.draft_cursor = screen.draft.len();
        screen.input_navigation(Navigation::Up).unwrap();
        assert_eq!(screen.draft_scroll, 0);
        assert_eq!(
            draft_cursor_position(&screen.draft, 75, screen.draft_cursor).0,
            5
        );
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
        assert_eq!(screen.draft_scroll, 0);
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
    fn working_flower_animates_while_explore_and_elapsed_status_refresh() {
        let mut screen = Screen::new(false).unwrap();
        screen.full = true;
        screen.suspended = true; // test the state machine without writing ANSI to stdout
        screen.start_work().unwrap();
        assert_eq!(screen.activity.as_ref().unwrap().phase, "Working…");
        screen.tool_start("r", "read", "src/main.rs").unwrap();
        screen
            .tool_end("r", "read", "src/main.rs", false, None)
            .unwrap();
        assert!(screen.rendered_transcript().contains("◐ Exploring"));
        screen.activity.as_mut().unwrap().last_spinner -= Duration::from_millis(400);
        screen.tick_work().unwrap();
        assert!(screen.rendered_transcript().contains("◓ Exploring"));
        assert_eq!(screen.activity.as_ref().unwrap().flower_frame, 1);
        screen.activity.as_mut().unwrap().last_refresh -= Duration::from_secs(2);
        screen.tick_work().unwrap();
        assert!(screen.activity.as_ref().unwrap().last_refresh.elapsed() < Duration::from_secs(1));
        screen.tool_start("e", "edit", "src/main.rs").unwrap();
        let settled = screen.rendered_transcript();
        assert!(settled.contains("✓ Explored"));
        screen.activity.as_mut().unwrap().last_spinner -= Duration::from_millis(400);
        screen.tick_work().unwrap();
        assert_eq!(screen.rendered_transcript(), settled);
        assert_eq!(screen.activity.as_ref().unwrap().flower_frame, 2);
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
    fn working_footer_puts_status_left_and_goal_right_only_while_active() {
        let line = working_status(76, "✿", "Thinking…", 49, "", Some((2, 4)));
        assert!(line.starts_with("✿ Thinking… · 49s · Esc stop"));
        assert!(
            line.ends_with("Goal  ██████████░░░░░░░░░░  2/4 · 50%"),
            "{line}"
        );
        assert_eq!(line.chars().count(), 76);
        let narrow = working_status(40, "✿", "Reading…", 7, "", Some((2, 4)));
        assert!(narrow.contains("Goal 2/4 · 50%"));
        assert!(narrow.chars().count() <= 40);
        assert!(!working_status(76, "✿", "Working…", 3, "", None).contains("Goal"));
    }

    #[test]
    fn goal_row_is_one_line_and_absent_without_an_explicit_checklist() {
        assert_eq!(
            goal_line(40, 12, 20),
            "Goal  ████████████░░░░░░░░  12/20 · 60%"
        );
        assert_eq!(
            goal_line(40, 20, 20),
            "Goal  ████████████████████  20/20 · 100%"
        );
        assert!(goal_line(18, 3, 20).chars().count() <= 18);
        let mut screen = Screen::new(false).unwrap();
        screen.set_goal(&serde_json::json!({"hibiscusGoal":{"completed":3,"total":20}}));
        assert_eq!(screen.goal, Some((3, 20)));
        screen.set_goal(
            &serde_json::json!({"hibiscusGoal":{"completed":20,"total":20,"error":"invalid"}}),
        );
        assert_eq!(screen.goal, Some((3, 20)));
        screen.clear_session().unwrap();
        assert_eq!(screen.goal, None);
        screen.restore_goal(&[
            serde_json::json!({"role":"toolResult","toolName":"goal","details":{"hibiscusGoal":{"completed":12,"total":20}}}),
            serde_json::json!({"role":"toolResult","toolName":"read","details":{"hibiscusGoal":{"completed":20,"total":20}}}),
        ]).unwrap();
        assert_eq!(screen.goal, Some((12, 20)));
        screen.restore_goal(&[]).unwrap();
        assert_eq!(screen.goal, None);
    }

    #[test]
    fn background_covers_erased_space_and_survives_style_resets_only_with_color() {
        let original = "\x1b[Hleft\x1b[1;38;2;236;74;125m✿\x1b[0m\x1b[K\r\n\x1b[K\r\n\x1b[48;2;57;30;46muser\x1b[0m\x1b[K\r\n\x1b[J";
        let painted = paint_background(original, true);
        assert!(painted.starts_with(BASE_BACKGROUND));
        assert!(painted.contains(&format!("✿\x1b[0m{BASE_BACKGROUND}\x1b[K")));
        assert!(painted.contains(&format!("user\x1b[0m{BASE_BACKGROUND}\x1b[K")));
        assert!(painted.ends_with("\x1b[J"));
        assert_eq!(paint_background(original, false), original);
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
