//! Fullscreen state and session presentation.
mod events;
mod input;
mod render;
use crate::ui::{
    display::*,
    input::{escape_key, MouseEvent, Navigation},
    mode::Mode,
    paste::Input as PasteInput,
    tool,
};
use serde_json::Value;
use std::io::{self, Write};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use std::{
    collections::{HashMap, VecDeque},
    env,
};

use super::{
    activity::Timeline,
    file_mentions, header,
    markdown::{self, Role, Tone},
    modal::{self, Modal},
    paint::{Cursor, Painter, SYNC_END},
    picker::Picker,
};
use crate::{
    commands,
    ui::{format::compact_tokens, text::base64},
    Result,
};

struct PendingQueue {
    text: String,
    mode: QueueMode,
    images: usize,
    accepted: bool,
    sending: bool,
}

#[derive(Clone, Copy, Debug)]
struct TextSelection {
    anchor: (usize, usize), // transcript row, character column
    focus: (usize, usize),
    dragging: bool,
}

impl TextSelection {
    fn bounds(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    fn columns(&self, row: usize, length: usize, prefix: usize) -> Option<(usize, usize)> {
        let (start, end) = self.bounds();
        if row < start.0 || row > end.0 {
            return None;
        }
        let first = if row == start.0 {
            start.1.saturating_sub(prefix)
        } else {
            0
        };
        let last = if row == end.0 {
            end.1.saturating_sub(prefix)
        } else {
            length
        };
        (first < last).then_some((first.min(length), last.min(length)))
    }
}

const FLOWERS: [&str; 4] = ["✿", "❀", "✾", "❁"];
// TUI keeps the aubergine theme; CLI uses a local dark SGR fill.
// Neither view changes the terminal's configured palette with OSC 11.
const BASE_BACKGROUND: &str = "\x1b[48;2;48;10;36m";

// Give narrow terminals one margin cell instead of two. Keep Markdown,
// composer movement, panels and cursor based on the same content width.
const NARROW_COLUMNS: usize = 60;
fn content_width(columns: usize) -> usize {
    columns
        .saturating_sub(if columns < NARROW_COLUMNS { 2 } else { 4 })
        .max(1)
}

fn transcript_prefix(columns: usize) -> usize {
    if columns < NARROW_COLUMNS {
        2
    } else {
        3
    }
}

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

/// Last confirmed Runtime session snapshot, not an independently calculated budget.
struct SessionUsage {
    context: Option<String>,
    input: Option<u64>,
    output: Option<u64>,
    cache_read: Option<u64>,
    cost: Option<f64>,
}
impl SessionUsage {
    fn from_runtime(data: &Value) -> Option<Self> {
        if !data.is_object() {
            return None;
        }
        let context = &data["contextUsage"];
        let context = match (
            context["tokens"].as_u64(),
            context["contextWindow"].as_u64(),
        ) {
            (Some(tokens), Some(window)) if window > 0 => {
                let percent = context["percent"]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0)
                    .map_or(String::new(), |n| format!(" · {n:.0}%"));
                Some(format!(
                    "Ctx {}/{}{percent}",
                    compact_tokens(tokens),
                    compact_tokens(window)
                ))
            }
            _ => None, // Runtime may report null immediately after compaction.
        };
        let cost = data["cost"].as_f64().filter(|n| n.is_finite() && *n >= 0.0);
        let usage = Self {
            context,
            input: data["tokens"]["input"].as_u64(),
            output: data["tokens"]["output"].as_u64(),
            cache_read: data["tokens"]["cacheRead"].as_u64(),
            cost,
        };
        (!usage.parts().is_empty()).then_some(usage)
    }
    fn parts(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if let Some(context) = &self.context {
            parts.push(context.clone());
        }
        if let (Some(input), Some(output)) = (self.input, self.output) {
            parts.push(format!(
                "In {} · Out {}",
                compact_tokens(input),
                compact_tokens(output)
            ));
        }
        if let Some(read) = self.cache_read {
            parts.push(format!("Cache ~{}", compact_tokens(read)));
        }
        if let Some(cost) = self.cost {
            parts.push(if cost > 0.0 && cost < 0.005 {
                "Cost <$0.01".into()
            } else {
                format!("Cost ~${cost:.2}")
            });
        }
        parts
    }
}

fn with_usage(mut left: String, limit: usize, usage: Option<&SessionUsage>) -> String {
    if let Some(usage) = usage {
        for part in usage.parts() {
            if left.chars().count() + part.chars().count() + 3 <= limit {
                left.push_str(" · ");
                left.push_str(&part);
            }
        }
    }
    clip(&left, limit)
}

/// An alternate-screen transcript with a persistent composer. Non-terminal
/// streams pass through untouched; RPC responses are never written here.
pub(crate) struct Screen {
    out: io::Stdout,
    full: bool,
    terminal: bool,
    has_credentials: bool,
    extension_commands: Vec<crate::extensions::CommandInfo>,
    suspended: bool,
    color: bool,
    painter: Painter,
    diagnostics: crate::runtime::diagnostics::Diagnostics,
    diagnostic_revision: u64,
    last_paint: Option<Instant>,
    paint_pending: bool,
    transcript: Vec<u8>,
    layout: markdown::Layout,
    diagrams: HashMap<String, Vec<String>>,
    layout_dirty: bool,
    scroll_anchor: Option<usize>,
    workspace: String,
    draft: String,
    draft_cursor: usize,
    preferred_column: Option<usize>,
    restored_draft: Option<(String, Vec<SharedImage>)>,
    disconnected: bool,
    draft_scroll: usize,
    images: Vec<SharedImage>,
    clipboard: Option<super::clipboard::Job>,
    clipboard_notice: String,
    input_notice: Option<String>,
    queue_steering: usize,
    queue_follow_up: usize,
    pending_queue: Vec<PendingQueue>,
    delivered_before_ack: Vec<String>,
    initial_user_pending: Option<String>,
    rejected_queued: Option<(String, Vec<SharedImage>)>,
    active_utf8: Vec<u8>,
    model: String,
    thinking: Option<String>,
    session: String,
    usage: Option<SessionUsage>,
    scroll: usize,
    selection: Option<TextSelection>,
    selectable_rows: Vec<String>,
    transcript_top: usize,
    exit_armed: Option<Instant>,
    paste: Option<PasteInput>,
    picker: Option<Picker>,
    modal: Option<Modal>,
    suggestions: Option<Picker>,
    suggestions_dismissed: bool,
    file_search: Option<file_mentions::Search>,
    file_prefix: Option<file_mentions::Prefix>,
    file_matches: Vec<file_mentions::Entry>,
    pending_input: Option<u8>,
    deferred_input: VecDeque<u8>,
    secret_input: bool,
    auth_url: Option<String>,
    activity: Option<Activity>,
    manual_compaction: bool,
    timelines: Vec<Timeline>,
    current_timeline: Option<usize>,
    timeline_marked: bool,
    goal: Option<(usize, usize)>,
}

impl Screen {
    #[cfg(test)]
    pub(crate) fn new(interactive: bool) -> io::Result<Self> {
        Self::with_diagnostics(
            interactive,
            false,
            crate::runtime::diagnostics::Diagnostics::default(),
        )
    }

    pub(crate) fn with_diagnostics(
        interactive: bool,
        color: bool,
        diagnostics: crate::runtime::diagnostics::Diagnostics,
    ) -> io::Result<Self> {
        let out = io::stdout();
        // Capability selection belongs to ui::Frontend. Once selected, a
        // resize is a layout concern, never an implicit change of frontend.
        let terminal = interactive;
        let full = interactive;
        let mut screen = Self {
            out,
            full,
            terminal,
            has_credentials: false,
            extension_commands: Vec::new(),
            suspended: true,
            color,
            painter: Painter::new(env::var_os("HIBISCUS_NO_SYNC_UPDATE").is_none()),
            diagnostics,
            diagnostic_revision: 0,
            last_paint: None,
            paint_pending: false,
            transcript: Vec::new(),
            layout: markdown::Layout::default(),
            diagrams: HashMap::new(),
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
            model: "no active model".into(),
            thinking: None,
            session: "new chat".into(),
            usage: None,
            scroll: 0,
            selection: None,
            selectable_rows: Vec::new(),
            transcript_top: 0,
            exit_armed: None,
            paste: None,
            picker: None,
            modal: None,
            suggestions: None,
            suggestions_dismissed: false,
            file_search: None,
            file_prefix: None,
            file_matches: Vec::new(),
            pending_input: None,
            deferred_input: VecDeque::new(),
            secret_input: false,
            auth_url: None,
            activity: None,
            manual_compaction: false,
            timelines: Vec::new(),
            current_timeline: None,
            timeline_marked: false,
            goal: None,
        };
        screen.resume()?;
        Ok(screen)
    }

    fn diagnostics_changed(&self) -> bool {
        self.diagnostic_revision != self.diagnostics.revision()
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
            self.selection = None;
            self.selectable_rows.clear();
            self.exit_armed = None;
            self.paste = None;
            write!(
                self.out,
                "{SYNC_END}\x1b[?2004l\x1b[<u\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l"
            )?;
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
            // SGR coordinates plus button-motion let the UI own selection in
            // the alternate screen; without drag reports terminal selection is lost.
            write!(
                self.out,
                "\x1b[2J\x1b[?2004h\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?25l"
            )?;
            self.suspended = false;
            self.render()?;
        }
        Ok(())
    }

    pub(crate) fn status(&mut self, state: &Value) -> io::Result<()> {
        self.extension_commands =
            serde_json::from_value(state["extensionCommands"].clone()).unwrap_or_default();
        self.has_credentials = state["hasCredentials"]
            .as_bool()
            .unwrap_or_else(|| state["model"].is_object());
        let provider = state["model"]["provider"].as_str().unwrap_or("");
        let model = state["model"]["id"].as_str().unwrap_or("no active model");
        self.model = if provider.is_empty() {
            clean(model)
        } else {
            format!("{}/{}", clean(provider), clean(model))
        };
        self.thinking = state["model"]
            .is_object()
            .then_some(&state["thinkingLevel"])
            .and_then(Value::as_str)
            .filter(|level| {
                !level.is_empty()
                    && level.len() <= 32
                    && level
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            })
            .map(str::to_owned);
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

    pub(crate) fn session_usage(&mut self, stats: &Value) -> io::Result<()> {
        self.usage = SessionUsage::from_runtime(stats);
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

    /// Reset only the local view after Runtime has created a new session. Persisted
    /// history belongs to Runtime and is not deleted by clearing the screen.
    pub(crate) fn clear_session(&mut self) -> io::Result<()> {
        self.dirty_transcript();
        self.transcript.clear();
        self.diagrams.clear();
        self.layout.invalidate();
        self.draft.clear();
        self.draft_cursor = 0;
        self.preferred_column = None;
        self.restored_draft = None;
        self.draft_scroll = 0;
        self.scroll = 0;
        self.clear_images();
        self.suggestions = None;
        self.suggestions_dismissed = false;
        self.file_search = None;
        self.file_prefix = None;
        self.file_matches.clear();
        self.picker = None;
        self.modal = None;
        self.auth_url = None;
        self.secret_input = false;
        self.activity = None;
        self.manual_compaction = false;
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
        self.deferred_input.clear();
        self.input_notice = None;
        self.exit_armed = None;
        self.paste = None;
        self.session = "new chat".into();
        self.usage = None;
        self.render()
    }

    pub(crate) fn defer_input(&mut self, byte: u8) {
        self.pending_input = Some(byte);
    }

    /// Preserve typing collected while an asynchronous session scan completes.
    /// Enter/Esc are handled during the scan and are never replayed as sends.
    pub(crate) fn defer_session_typing(&mut self, bytes: impl IntoIterator<Item = u8>) {
        self.deferred_input.extend(bytes);
    }

    pub(crate) fn start_session_scan(&mut self) -> io::Result<Option<String>> {
        let old = self
            .input_notice
            .replace("Loading saved sessions… · Esc cancel".into());
        self.render()?;
        Ok(old)
    }

    pub(crate) fn end_session_scan(&mut self, old: Option<String>) -> io::Result<()> {
        if self.input_notice.as_deref() == Some("Loading saved sessions… · Esc cancel") {
            self.input_notice = old;
        }
        self.render()
    }

    pub(crate) fn take_rejected_queue(&mut self) -> Option<(String, Vec<SharedImage>)> {
        self.rejected_queued.take()
    }

    pub(crate) fn restore_draft(&mut self, text: String, images: Vec<SharedImage>) {
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
        self.selection = None;
        if self.scroll > 0 && self.scroll_anchor.is_none() {
            self.scroll_anchor = Some(self.layout.len());
        }
        self.layout_dirty = true;
    }

    fn transcript_layout(&mut self, width: usize) -> std::rc::Rc<Vec<std::rc::Rc<markdown::Row>>> {
        if self.layout_dirty || self.layout.width() != width {
            self.layout
                .update_with_diagrams(&self.rendered_transcript(), width, &self.diagrams);
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
    usage: Option<&SessionUsage>,
) -> String {
    if width < NARROW_COLUMNS {
        // Small screens must never show a chopped-off usage label. Keep the
        // activity and, when present, the model-reported Goal on one row.
        let phase = format!("{flower} {phase}");
        let timed = format!("{phase} · {seconds}s");
        let normal = if scrolled.is_empty() {
            format!("{timed} · Esc stop")
        } else {
            format!("{timed} · ↑")
        };
        let Some((completed, total)) = goal.filter(|(_, total)| *total > 0) else {
            return [normal, timed, phase, format!("{flower} Working…")]
                .into_iter()
                .find(|text| text.chars().count() <= width)
                .unwrap_or_else(|| flower.to_owned());
        };
        let right = format!("Goal {completed}/{total} · {}%", completed * 100 / total);
        let remaining = width.saturating_sub(right.chars().count() + 1);
        let left = [timed, phase, flower.to_owned()]
            .into_iter()
            .find(|text| text.chars().count() <= remaining)
            .unwrap_or_default();
        if left.is_empty() {
            return clip(&right, width);
        }
        return format!(
            "{left}{}{right}",
            " ".repeat(width - left.chars().count() - right.chars().count())
        );
    }
    let left = format!("{flower} {phase} · {seconds}s · Esc stop{scrolled}");
    let Some((completed, total)) = goal.filter(|(_, total)| *total > 0) else {
        return with_usage(left, width, usage);
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
    let left = with_usage(left, space, usage);
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
            // screen text independently of Hibiscus's persistent session history.
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
        } else if self.terminal {
            let original_len = buf.len();
            // The shared terminal reader keeps raw mode active in CLI.
            // Normalize output newlines without changing piped output.
            let mut start = 0;
            for (index, byte) in buf.iter().enumerate() {
                if *byte == b'\n' {
                    self.out.write_all(&buf[start..index])?;
                    if index == 0 || buf[index - 1] != b'\r' {
                        self.out.write_all(b"\r")?;
                    }
                    self.out.write_all(b"\n")?;
                    start = index + 1;
                }
            }
            self.out.write_all(&buf[start..])?;
            Ok(original_len)
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
/// an exact wrap boundary. Draft bytes sent to Runtime are never reflowed.
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
mod tests;
