//! Owns view selection and terminal handover. Each frontend owns its rendering state.
use super::{
    display::*,
    input::{MouseEvent, Navigation},
    mode::Mode,
    terminal,
};
use crate::{cli, runtime::diagnostics::Diagnostics, tui, Result};
use serde_json::Value;
use std::{
    env,
    io::{self, IsTerminal, Write},
    sync::mpsc::Receiver,
};

enum View {
    Tui(Box<tui::Screen>),
    Cli(cli::Screen),
}
pub(crate) struct Frontend {
    view: View,
    terminal: bool,
    capable: bool,
    color: bool,
    state: Value,
    diagnostics: Diagnostics,
    disconnected: bool,
    restored: Option<(String, Vec<SharedImage>)>,
    rejected: Option<(String, Vec<SharedImage>)>,
}
impl Frontend {
    pub(crate) fn new(interactive: bool) -> io::Result<Self> {
        Self::with_mode(interactive, Mode::Tui)
    }
    pub(crate) fn with_mode(interactive: bool, mode: Mode) -> io::Result<Self> {
        let terminal = interactive && io::stdout().is_terminal();
        let capable = terminal && env::var("TERM").is_ok_and(|term| term != "dumb");
        let fits = terminal::size().0 >= 40 && terminal::size().1 >= 14;
        let color = capable && fits && env::var_os("NO_COLOR").is_none();
        let diagnostics = Diagnostics::default();
        let state = Value::Null;
        let view = if mode == Mode::Tui && capable && fits {
            View::Tui(Box::new(tui::Screen::with_diagnostics(
                terminal,
                color,
                diagnostics.clone(),
            )?))
        } else {
            View::Cli(cli::Screen::new(terminal, capable, color)?)
        };
        Ok(Self {
            view,
            terminal,
            capable,
            color,
            state,
            diagnostics,
            disconnected: false,
            restored: None,
            rejected: None,
        })
    }
    pub(crate) fn has_extension_command(&self, name: &str) -> bool {
        self.state["extensionCommands"]
            .as_array()
            .is_some_and(|commands| commands.iter().any(|command| command["name"] == name))
    }
    pub(crate) fn is_full(&self) -> bool {
        matches!(self.view, View::Tui(_))
    }
    pub(crate) fn is_terminal_chat(&self) -> bool {
        self.terminal
    }
    pub(crate) fn set_mode(&mut self, mode: Mode) -> io::Result<()> {
        if mode == Mode::Tui
            && (!self.capable || terminal::size().0 < 40 || terminal::size().1 < 14)
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "TUI requires an interactive terminal at least 40 columns by 14 rows.",
            ));
        }
        if self.is_full() == (mode == Mode::Tui) {
            return Ok(());
        }
        self.suspend()?;
        let next = match mode {
            Mode::Tui => {
                tui::Screen::with_diagnostics(self.terminal, self.color, self.diagnostics.clone())
                    .map(|s| View::Tui(Box::new(s)))
            }
            Mode::Cli => cli::Screen::new(self.terminal, self.capable, self.color).map(View::Cli),
        };
        match next {
            Ok(view) => self.view = view,
            Err(error) => {
                self.resume()?;
                return Err(error);
            }
        }
        let state = self.state.clone();
        self.status(&state)?;
        self.set_disconnected(self.disconnected)?;
        if let View::Tui(screen) = &mut self.view {
            if let Some((text, images)) = self.restored.take() {
                screen.restore_draft(text, images);
            }
        }
        Ok(())
    }
    fn tui(&mut self) -> Option<&mut tui::Screen> {
        match &mut self.view {
            View::Tui(s) => Some(s),
            View::Cli(_) => None,
        }
    }
    pub(crate) fn inline_welcome(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Cli(s) => s.welcome(),
            View::Tui(_) => Ok(()),
        }
    }
    pub(crate) fn read_inline(
        &mut self,
        events: &Receiver<u8>,
        raw: &mut terminal::RawMode,
    ) -> Result<Option<String>> {
        match &self.view {
            View::Cli(screen) => match screen.input_style() {
                Some(color) => cli::read(events, raw, color),
                None => terminal::read_line(events, raw),
            },
            View::Tui(_) => Err("Inline input requires CLI".into()),
        }
    }
    pub(crate) fn inline_input_style(&self) -> Option<bool> {
        match &self.view {
            View::Cli(s) => s.input_style(),
            View::Tui(_) => None,
        }
    }
    pub(crate) fn diagnostic_feed(&self) -> Option<Diagnostics> {
        self.is_full().then(|| self.diagnostics.clone())
    }
    pub(crate) fn suspend(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.suspend(),
            View::Cli(s) => s.suspend(),
        }
    }
    pub(crate) fn resume(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.resume(),
            View::Cli(s) => s.resume(),
        }
    }
    pub(crate) fn status(&mut self, state: &Value) -> io::Result<()> {
        self.state = state.clone();
        match &mut self.view {
            View::Tui(s) => s.status(state),
            View::Cli(s) => {
                s.status(state);
                Ok(())
            }
        }
    }
    pub(crate) fn saved_tool_outputs(
        &mut self,
        entries: &[crate::runtime::tool_output::Summary],
    ) -> io::Result<()> {
        let color = matches!(&self.view, View::Cli(s) if s.styled());
        self.write_all(cli::saved_list(entries, color, terminal::size().0).as_bytes())?;
        self.flush()
    }
    pub(crate) fn saved_tool_output(
        &mut self,
        entry: Option<&crate::runtime::tool_output::Output>,
    ) -> io::Result<()> {
        let color = matches!(&self.view, View::Cli(s) if s.styled());
        self.write_all(cli::saved_output(entry, color, terminal::size().0).as_bytes())?;
        self.flush()
    }
    pub(crate) fn session_usage(&mut self, stats: &Value) -> io::Result<()> {
        if let Some(s) = self.tui() {
            s.session_usage(stats)?;
        }
        Ok(())
    }
    pub(crate) fn user(&mut self, text: &str) -> io::Result<()> {
        if let Some(s) = self.tui() {
            s.user(text)
        } else {
            writeln!(self, "\nyou › {text}")
        }
    }
    pub(crate) fn clear_session(&mut self) -> io::Result<()> {
        self.restored = None;
        self.rejected = None;
        if let Some(s) = self.tui() {
            s.clear_session()?;
        }
        Ok(())
    }
    pub(crate) fn restore_draft(&mut self, text: String, images: Vec<SharedImage>) {
        if let Some(s) = self.tui() {
            s.restore_draft(text, images);
        } else {
            self.restored = Some((text, images));
        }
    }
    pub(crate) fn take_images(&mut self) -> Vec<SharedImage> {
        self.tui().map_or_else(Vec::new, |s| s.take_images())
    }
    pub(crate) fn take_rejected_queue(&mut self) -> Option<(String, Vec<SharedImage>)> {
        self.tui()
            .and_then(|s| s.take_rejected_queue())
            .or_else(|| self.rejected.take())
    }
    pub(crate) fn restore_goal(&mut self, messages: &[Value]) -> io::Result<()> {
        if let Some(s) = self.tui() {
            s.restore_goal(messages)?;
        }
        Ok(())
    }
    pub(crate) fn set_disconnected(&mut self, disconnected: bool) -> io::Result<()> {
        self.disconnected = disconnected;
        if let Some(s) = self.tui() {
            s.set_disconnected(disconnected)?;
        }
        Ok(())
    }
    pub(crate) fn render(&mut self) -> io::Result<()> {
        if let Some(s) = self.tui() {
            s.render()?;
        }
        Ok(())
    }
    pub(crate) fn prompt_with_update(
        &mut self,
        events: &Receiver<u8>,
        updates: Option<&Receiver<Option<String>>>,
    ) -> Result<PromptAction> {
        match self.tui() {
            Some(s) => s.prompt_with_update(events, updates),
            None => Err("Fullscreen prompt requires TUI".into()),
        }
    }
    pub(crate) fn select(
        &mut self,
        title: &str,
        items: &[String],
        current: Option<usize>,
        events: &Receiver<u8>,
    ) -> Result<Option<usize>> {
        match self.tui() {
            Some(s) => s.select(title, items, current, events),
            None => Ok(None),
        }
    }
    pub(crate) fn select_searchable(
        &mut self,
        title: &str,
        items: &[String],
        current: Option<usize>,
        events: &Receiver<u8>,
    ) -> Result<Option<usize>> {
        match self.tui() {
            Some(s) => s.select_searchable(title, items, current, events),
            None => Ok(None),
        }
    }
    pub(crate) fn start_session_scan(&mut self) -> io::Result<Option<String>> {
        match self.tui() {
            Some(s) => s.start_session_scan(),
            None => Ok(None),
        }
    }
    pub(crate) fn end_session_scan(&mut self, old: Option<String>) -> io::Result<()> {
        if let Some(s) = self.tui() {
            s.end_session_scan(old)?;
        }
        Ok(())
    }
    pub(crate) fn defer_session_typing(&mut self, bytes: impl IntoIterator<Item = u8>) {
        if let Some(s) = self.tui() {
            s.defer_session_typing(bytes);
        }
    }
    pub(crate) fn set_auth_url(&mut self, url: &str) -> io::Result<bool> {
        match self.tui() {
            Some(s) => s.set_auth_url(url),
            None => Ok(false),
        }
    }
    pub(crate) fn clear_auth_url(&mut self) -> io::Result<()> {
        if let Some(s) = self.tui() {
            s.clear_auth_url()?;
        }
        Ok(())
    }
    pub(crate) fn auth_input(
        &mut self,
        events: &Receiver<u8>,
        secret: bool,
        cancelled: impl FnMut() -> Result<bool>,
    ) -> Result<Option<String>> {
        self.auth_prompt(
            events,
            "Provider API key",
            "Enter API key",
            secret,
            cancelled,
        )
    }
    pub(crate) fn auth_prompt(
        &mut self,
        events: &Receiver<u8>,
        title: &str,
        placeholder: &str,
        secret: bool,
        cancelled: impl FnMut() -> Result<bool>,
    ) -> Result<Option<String>> {
        match self.tui() {
            Some(s) => s.auth_prompt(events, title, placeholder, secret, cancelled),
            None => Err("Authentication requires a fullscreen dialog".into()),
        }
    }
}
impl Write for Frontend {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &mut self.view {
            View::Tui(s) => s.write(buf),
            View::Cli(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.flush(),
            View::Cli(s) => s.flush(),
        }
    }
}

impl ScrollDisplay for Frontend {
    fn assistant_start(&mut self) -> io::Result<bool> {
        match &mut self.view {
            View::Tui(s) => s.assistant_start(),
            View::Cli(s) => s.assistant_start(),
        }
    }
    fn assistant_text(&mut self, text: &str) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.assistant_text(text),
            View::Cli(s) => s.assistant_text(text),
        }
    }
    fn end_assistant_text(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.end_assistant_text(),
            View::Cli(s) => s.end_assistant_text(),
        }
    }
    fn end_turn(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.end_turn(),
            View::Cli(s) => s.end_turn(),
        }
    }
    fn diff_line(&mut self, line: &str) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.diff_line(line),
            View::Cli(s) => s.diff_line(line),
        }
    }
    fn scroll(&mut self, navigation: Navigation) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.scroll(navigation),
            View::Cli(s) => s.scroll(navigation),
        }
    }
    fn mouse(&mut self, event: MouseEvent) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.mouse(event),
            View::Cli(s) => s.mouse(event),
        }
    }
    fn paste_start(&mut self) {
        match &mut self.view {
            View::Tui(s) => s.paste_start(),
            View::Cli(s) => s.paste_start(),
        }
    }
    fn paste_byte(&mut self, byte: u8) -> io::Result<bool> {
        match &mut self.view {
            View::Tui(s) => s.paste_byte(byte),
            View::Cli(s) => s.paste_byte(byte),
        }
    }
    fn start_command(&mut self, label: &str) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.start_command(label),
            View::Cli(s) => s.start_command(label),
        }
    }
    fn start_work(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.start_work(),
            View::Cli(s) => s.start_work(),
        }
    }
    fn set_work(&mut self, phase: &str) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.set_work(phase),
            View::Cli(s) => s.set_work(phase),
        }
    }
    fn tick_work(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.tick_work(),
            View::Cli(s) => s.tick_work(),
        }
    }
    fn stop_work(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.stop_work(),
            View::Cli(s) => s.stop_work(),
        }
    }
    fn complete_diagrams(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.complete_diagrams(),
            View::Cli(s) => s.complete_diagrams(),
        }
    }
    fn compaction_mode(&mut self, active: bool) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.compaction_mode(active),
            View::Cli(s) => s.compaction_mode(active),
        }
    }
    fn thinking_start(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.thinking_start(),
            View::Cli(s) => s.thinking_start(),
        }
    }
    fn thinking_end(&mut self) -> io::Result<bool> {
        match &mut self.view {
            View::Tui(s) => s.thinking_end(),
            View::Cli(s) => s.thinking_end(),
        }
    }
    fn tool_start(&mut self, id: &str, call: &super::tool::Call) -> io::Result<bool> {
        match &mut self.view {
            View::Tui(s) => s.tool_start(id, call),
            View::Cli(s) => s.tool_start(id, call),
        }
    }
    fn tool_end(
        &mut self,
        id: &str,
        call: &super::tool::Call,
        result: &super::tool::Outcome<'_>,
    ) -> io::Result<bool> {
        match &mut self.view {
            View::Tui(s) => s.tool_end(id, call, result),
            View::Cli(s) => s.tool_end(id, call, result),
        }
    }
    fn goal(&mut self, details: &Value) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.goal(details),
            View::Cli(s) => s.goal(details),
        }
    }
    fn split_timeline(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.split_timeline(),
            View::Cli(s) => s.split_timeline(),
        }
    }
    fn live_input(&self) -> bool {
        match &self.view {
            View::Tui(s) => s.live_input(),
            View::Cli(s) => s.live_input(),
        }
    }
    fn pending_key(&mut self) -> Option<u8> {
        match &mut self.view {
            View::Tui(s) => s.pending_key(),
            View::Cli(s) => s.pending_key(),
        }
    }
    fn active_key(&mut self, byte: u8, events: &Receiver<u8>) -> Result<ActiveAction> {
        match &mut self.view {
            View::Tui(s) => s.active_key(byte, events),
            View::Cli(s) => s.active_key(byte, events),
        }
    }
    fn queued(&mut self, submission: &DraftSubmission) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.queued(submission),
            View::Cli(s) => s.queued(submission),
        }
    }
    fn expect_initial_user(&mut self, message: &str) {
        match &mut self.view {
            View::Tui(s) => s.expect_initial_user(message),
            View::Cli(s) => s.expect_initial_user(message),
        }
    }
    fn delivered_user(&mut self, message: &Value) -> io::Result<bool> {
        match &mut self.view {
            View::Tui(s) => s.delivered_user(message),
            View::Cli(s) => s.delivered_user(message),
        }
    }
    fn hold_queue(&mut self, submission: DraftSubmission) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.hold_queue(submission),
            View::Cli(s) => s.hold_queue(submission),
        }
    }
    fn rejected_queue(&mut self, submission: DraftSubmission, error: &str) -> io::Result<()> {
        if matches!(self.view, View::Cli(_)) {
            self.rejected = Some((submission.text.clone(), submission.images.clone()));
        }
        match &mut self.view {
            View::Tui(s) => s.rejected_queue(submission, error),
            View::Cli(s) => s.rejected_queue(submission, error),
        }
    }
    fn queue_update(&mut self, update: &Value) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.queue_update(update),
            View::Cli(s) => s.queue_update(update),
        }
    }
    fn clear_queue_display(&mut self) -> io::Result<()> {
        match &mut self.view {
            View::Tui(s) => s.clear_queue_display(),
            View::Cli(s) => s.clear_queue_display(),
        }
    }
    fn extension_dialog(&mut self, event: &Value, events: &Receiver<u8>) -> Result<Option<Value>> {
        match &mut self.view {
            View::Tui(s) => s.extension_dialog(event, events),
            View::Cli(s) => s.extension_dialog(event, events),
        }
    }
}
