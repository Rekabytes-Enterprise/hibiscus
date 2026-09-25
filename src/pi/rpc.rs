use super::{
    error::{self, ChatError, ErrorSource},
    run::{RunOutcome, RunState},
    transport::{Events, Inbox, Limits, PipeWriter, WireWrite},
};
use crate::{
    pi::{configure_builtin_tools, dialog::Dialogs},
    tui::{
        screen::{
            escape_key, ActiveAction, DraftSubmission, Navigation, QueueMode, ScrollDisplay,
            SharedImage,
        },
        terminal::RawMode,
        ui::Ui,
    },
    Result,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Instant;

#[derive(Clone)]
pub(crate) enum SessionStart {
    New,
    Latest,
    Selected(PathBuf),
}

pub(crate) struct Rpc {
    child: Child,
    input: Option<PipeWriter<ChildStdin>>,
    output: Inbox,
    diagnostics: Option<JoinHandle<io::Result<u64>>>,
    next_id: u64,
    resume: SessionStart,
    disconnected: bool,
}

impl Rpc {
    pub(crate) fn start(start: SessionStart) -> Result<Self> {
        let limits = Limits::from_env()?;
        let pi = env::var("HIBISCUS_PI").unwrap_or_else(|_| "pi".to_owned());
        let mut command = Command::new(&pi);
        command.args(["--mode", "rpc"]);
        configure_builtin_tools(&mut command);
        let resume = match &start {
            SessionStart::Selected(path) => SessionStart::Selected(path.clone()),
            _ => SessionStart::New,
        };
        match start {
            SessionStart::Latest => {
                command.arg("--continue");
            }
            SessionStart::Selected(path) => {
                command.arg("--session").arg(path);
            }
            SessionStart::New => {}
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|error| {
                error::transport(format!(
                    "could not start {pi}: {error} (install Pi or set HIBISCUS_PI)"
                ))
            })?;
        let input = child.stdin.take().ok_or("pi stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("pi stdout unavailable")?;
        let output = Inbox::start(stdout, limits);
        let input = match output.writer(input) {
            Ok(input) => input,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error::transport(error).into());
            }
        };
        let stderr = child.stderr.take().ok_or("pi stderr unavailable")?;
        let diagnostics =
            thread::spawn(move || io::copy(&mut BufReader::new(stderr), &mut io::stderr()));
        Ok(Self {
            child,
            input: Some(input),
            output,
            diagnostics: Some(diagnostics),
            next_id: 0,
            resume,
            disconnected: false,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn command<R: BufRead, W: ScrollDisplay>(
        &mut self,
        command: Value,
        images: &[SharedImage],
        display: &mut W,
        fallback: &mut R,
        dialogs: &mut Dialogs,
        events: Option<&Receiver<u8>>,
        raw: &mut Option<RawMode>,
        interactive: bool,
    ) -> Result<bool> {
        self.next_id += 1;
        let id = format!("hibiscus-{}", self.next_id);
        let mut command = command;
        command["id"] = json!(id);
        let kind = command["type"]
            .as_str()
            .ok_or("RPC command missing type")?
            .to_owned();
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| error::transport("stdin unavailable"))?;
        if kind == "prompt" {
            display.expect_initial_user(command["message"].as_str().unwrap_or(""));
        }
        if kind == "prompt" {
            display.start_work()?;
            display.set_work("Sending prompt…")?;
        }
        let sending = write_command(
            &mut input.checked(|| service_sending(display, events, kind == "prompt")),
            &command,
            images,
        );
        let mut result = sending.and_then(|()| {
            if kind == "prompt" {
                display.set_work("Working…")?;
            }
            exchange(
                input,
                &self.output,
                display,
                fallback,
                dialogs,
                events,
                &id,
                &kind,
                raw,
                interactive,
            )
        });
        if let Err(error) = &mut result {
            if let Some(error) = error.downcast_mut::<ChatError>() {
                error.command_id = Some(id);
            }
        }
        if kind == "prompt" {
            display.stop_work()?;
        }
        if kind == "new_session" && result.is_ok() {
            self.resume = SessionStart::New;
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prompt<R: BufRead, W: ScrollDisplay>(
        &mut self,
        message: &str,
        display: &mut W,
        fallback: &mut R,
        dialogs: &mut Dialogs,
        events: Option<&Receiver<u8>>,
        raw: &mut Option<RawMode>,
        interactive: bool,
    ) -> Result<()> {
        self.command(
            json!({"type": "prompt", "message": message}),
            &[],
            display,
            fallback,
            dialogs,
            events,
            raw,
            interactive,
        )
        .map(|_| ())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn request<R: BufRead, W: ScrollDisplay>(
        &mut self,
        mut command: Value,
        fallback: &mut R,
        display: &mut W,
        dialogs: &mut Dialogs,
        raw: &mut Option<RawMode>,
        events: Option<&Receiver<u8>>,
        interactive: bool,
    ) -> Result<Value> {
        self.next_id += 1;
        let id = format!("hibiscus-{}", self.next_id);
        command["id"] = json!(id);
        let kind = command["type"]
            .as_str()
            .ok_or("RPC command missing type")?
            .to_owned();
        let writer = self
            .input
            .as_mut()
            .ok_or_else(|| error::transport("stdin unavailable"))?;
        writeln!(writer, "{command}").map_err(error::transport)?;
        writer.flush().map_err(error::transport)?;
        let mut deadline = Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let mut record = receive_record(&self.output, deadline)?
                .ok_or_else(|| error::transport("stream ended before the command completed"))?;
            check_record(&record)?;
            if record["type"] == "response" && record["id"] == id {
                if record["success"] != true {
                    return Err(rejected(&record, &kind, &id).into());
                }
                if kind == "get_state" {
                    if !record["data"].is_object() {
                        return Err(error::protocol("Invalid get_state response").into());
                    }
                    self.resume = record["data"]["sessionFile"]
                        .as_str()
                        .map(|path| SessionStart::Selected(path.into()))
                        .unwrap_or(SessionStart::New);
                }
                if kind == "switch_session" && record["data"]["cancelled"] != true {
                    self.resume = command["sessionPath"]
                        .as_str()
                        .map(|path| SessionStart::Selected(path.into()))
                        .unwrap_or(SessionStart::New);
                }
                return Ok(record.get_mut("data").map_or(Value::Null, Value::take));
            }
            dialogs.handle(
                self.input.as_mut().ok_or("pi stdin unavailable")?,
                &record,
                fallback,
                display,
                raw,
                events,
                interactive,
            )?;
            if record["type"] == "extension_ui_request" {
                // Time spent answering a human dialog is not a stalled RPC.
                deadline = Instant::now() + std::time::Duration::from_secs(30);
            }
        }
    }

    pub(crate) fn is_connected(&self) -> bool {
        !self.disconnected && self.input.is_some()
    }

    pub(crate) fn has_saved_session(&self) -> bool {
        matches!(self.resume, SessionStart::Selected(_))
    }

    /// Stop an untrusted connection before opening another session writer.
    pub(crate) fn disconnect(&mut self) {
        let was_open = self.input.take().is_some();
        self.disconnected = true;
        if was_open || matches!(self.child.try_wait(), Ok(None)) {
            // SAFETY: this child was started in its own process group. Kill
            // before reaping so surviving tools cannot write the old session.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.diagnostics.take() {
            if reader.is_finished() {
                let _ = reader.join();
            }
        }
    }

    pub(crate) fn recover_connection(&mut self) -> Result<()> {
        let start = self.resume.clone();
        self.disconnect();
        *self = Self::start(start)?;
        Ok(())
    }

    fn wait_for_shutdown(&mut self) -> Result<std::process::ExitStatus> {
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if self.output.failed() || Instant::now() >= deadline {
                self.disconnect();
                return Err(error::transport(
                    "Pi RPC failed or timed out during shutdown; outcome is uncertain",
                )
                .into());
            }
            if let Some(status) = self.child.try_wait().map_err(error::transport)? {
                if self
                    .diagnostics
                    .as_ref()
                    .is_none_or(|reader| reader.is_finished())
                    && self.output.closed_cleanly()
                {
                    if let Some(reader) = self.diagnostics.take() {
                        let _ = reader.join();
                    }
                    return Ok(status);
                }
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Stop the idle RPC child before another Pi process opens its session.
    pub(crate) fn close(&mut self) -> Result<()> {
        if self.output.failed() {
            self.disconnect();
            return Err(error::transport(
                "Pi RPC receive failure; connection outcome is uncertain",
            )
            .into());
        }
        if self.input.is_none() {
            return Ok(());
        }
        drop(self.input.take());
        let status = self.wait_for_shutdown()?;
        if !status.success() {
            return Err(format!("pi exited with {status}").into());
        }
        Ok(())
    }

    pub(crate) fn reopen(&mut self, start: SessionStart) -> Result<()> {
        if self.input.is_some() {
            return Err("pi RPC child must be closed before reopening".into());
        }
        *self = Self::start(start)?;
        Ok(())
    }

    /// Reopen a saved session after an external credential change.
    pub(crate) fn reconnect(&mut self, start: SessionStart) -> Result<()> {
        self.close()?;
        self.reopen(start)
    }

    pub(crate) fn finish(mut self, outcome: Result<()>) -> Result<()> {
        if self.disconnected {
            self.disconnect();
            return outcome;
        }
        if self.output.failed() {
            self.disconnect();
            return outcome.and_then(|()| {
                Err(error::transport("Pi RPC connection failed; outcome is uncertain").into())
            });
        }
        if outcome.is_err() {
            self.disconnect();
            return outcome;
        }
        drop(self.input.take());
        let status = self.wait_for_shutdown()?;
        outcome?;
        if !status.success() {
            return Err(format!("pi exited with {status}").into());
        }
        Ok(())
    }
}

impl Drop for Rpc {
    fn drop(&mut self) {
        if self.input.is_some() {
            self.disconnect();
        }
    }
}

#[allow(clippy::too_many_arguments)] // Pi RPC and terminal streams must remain independent.
fn exchange<F: BufRead, W: ScrollDisplay>(
    input: &mut impl WireWrite,
    output: &impl Events,
    display: &mut W,
    fallback: &mut F,
    dialogs: &mut Dialogs,
    events: Option<&Receiver<u8>>,
    id: &str,
    kind: &str,
    raw: &mut Option<RawMode>,
    interactive: bool,
) -> Result<bool> {
    let ui = Ui::new(interactive);
    let mut active_tools = HashMap::<String, String>::new();
    let mut reasoning_since = None;
    let mut accepted = false;
    let mut settled = false;
    let mut printed_text = false;
    let mut pending_newline = false;
    let mut run = RunState::default();
    let mut interrupted = false;
    let mut abort_id = None;
    let mut abort_done = false;
    let mut clear_id = None;
    let mut clear_done = false;
    let mut rejection = None;
    let mut cancellation_started = None;
    let mut pending_queued = HashMap::<String, DraftSubmission>::new();
    let mut next_queue_id = 0;
    let mut queued_count = 0;
    loop {
        if kind == "prompt" {
            display.tick_work()?;
        }
        // Poll terminal input before consuming queued Pi events too; a busy
        // tool can otherwise keep stdout nonempty and starve Esc indefinitely.
        maybe_interrupt(
            input,
            display,
            events,
            kind,
            id,
            accepted,
            settled && pending_queued.is_empty() && queued_count == 0,
            &mut interrupted,
            &mut clear_id,
            &mut abort_id,
            &mut pending_queued,
            &mut next_queue_id,
            queued_count,
        )?;
        if interrupted
            && cancellation_started
                .get_or_insert_with(Instant::now)
                .elapsed()
                > std::time::Duration::from_secs(10)
        {
            return Err(error::transport(
                "Cancellation did not settle within 10 seconds; outcome is uncertain",
            )
            .into());
        }
        let event = match output.recv_timeout(std::time::Duration::from_millis(40)) {
            Ok(Ok(event)) => event,
            Ok(Err(error)) => {
                preserve_uncertain_queue(display, &mut pending_queued)?;
                return Err(error);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                preserve_uncertain_queue(display, &mut pending_queued)?;
                return Err(error::transport("stream ended before the command completed").into());
            }
        };
        if let Err(error) = check_record(&event) {
            preserve_uncertain_queue(display, &mut pending_queued)?;
            return Err(error);
        }
        run.observe(&event);
        if kind == "prompt" && event["type"] == "response" {
            if let Some(queued) = event["id"]
                .as_str()
                .and_then(|id| pending_queued.remove(id))
            {
                if event["success"] == true && interrupted {
                    display.rejected_queue(queued, "Pi is stopping; queue delivery is uncertain. Check history before resending")?;
                } else if event["success"] == true {
                    display.queued(&queued)?;
                    settled = false;
                } else {
                    display.rejected_queue(
                        queued,
                        event["error"].as_str().unwrap_or("unknown error"),
                    )?;
                }
                display.flush()?;
                continue;
            }
        }
        if event["type"] == "response" {
            if (clear_id.as_deref() == event["id"].as_str()
                || abort_id.as_deref() == event["id"].as_str())
                && event["id"].is_string()
                && event["success"] != true
            {
                return Err(
                    error::protocol("Pi rejected cancellation; run state is uncertain").into(),
                );
            }
            if clear_id.as_deref() == event["id"].as_str() {
                clear_done = true;
                display.clear_queue_display()?;
                queued_count = 0;
            }
            if abort_id.as_deref() == event["id"].as_str() {
                abort_done = true;
            }
        }
        match event["type"].as_str() {
            Some("agent_start") if kind == "prompt" => settled = false,
            Some("response") if event["id"] == id => {
                if event["success"] != true {
                    let error = rejected(&event, kind, id);
                    if !interrupted || (clear_done && abort_done) {
                        return Err(error.into());
                    }
                    // Esc may already have sent clear_queue/abort. Drain their
                    // acknowledgements before another prompt can be submitted.
                    rejection = Some(error);
                    continue;
                }
                accepted = true;
                if kind != "prompt" {
                    if event["data"]["cancelled"] == true {
                        return Err(ChatError::cancelled(kind).into());
                    }
                    return Ok(false);
                }
                if settled
                    && pending_queued.is_empty()
                    && queued_count == 0
                    && (!interrupted || (abort_done && clear_done))
                {
                    return finish_turn(
                        &ui,
                        display,
                        printed_text || pending_newline,
                        run.finish(interrupted),
                        interrupted,
                    );
                }
            }
            Some("message_start") if kind == "prompt" && event["message"]["role"] == "user" => {
                if display.delivered_user(&event["message"])? {
                    printed_text = false;
                    pending_newline = false;
                    display.flush()?;
                }
            }
            Some("queue_update") if kind == "prompt" => {
                queued_count = event["steering"].as_array().map_or(0, Vec::len)
                    + event["followUp"].as_array().map_or(0, Vec::len);
                display.queue_update(&event)?;
            }
            Some("message_update") if kind == "prompt" => {
                let update = &event["assistantMessageEvent"];
                match update["type"].as_str() {
                    Some("thinking_start" | "thinking_delta") => {
                        if reasoning_since.is_none() {
                            reasoning_since = Some(Instant::now());
                            if interactive {
                                display.thinking_start()?;
                            }
                        }
                        display.set_work("Thinking…")?;
                    }
                    Some("thinking_end") => {
                        show_reasoning(&ui, display, &mut reasoning_since)?;
                        display.set_work("Preparing response…")?;
                    }
                    Some("toolcall_start") => {
                        let name = update["toolName"].as_str().unwrap_or("tool");
                        display.set_work(&format!("Preparing {}…", safe_detail(name)))?;
                    }
                    Some("text_delta") => {
                        display.set_work("Writing reply…")?;
                        if let Some(delta) = update["delta"].as_str() {
                            if !printed_text && reasoning_since.is_some() {
                                show_reasoning(&ui, display, &mut reasoning_since)?;
                            }
                            if pending_newline {
                                writeln!(display)?;
                                pending_newline = false;
                            }
                            if !printed_text && !pending_newline {
                                display.split_timeline()?;
                                ui.assistant(display)?;
                            }
                            write!(display, "{delta}")?;
                            display.flush()?;
                            printed_text = true;
                        }
                    }
                    _ => {}
                }
            }
            Some("message_end") if kind == "prompt" && event["message"]["role"] == "assistant" => {
                show_reasoning(&ui, display, &mut reasoning_since)?;
                let message = &event["message"];
                // RunState retains the latest attempt error until settlement;
                // intermediate failures must not be printed as final outcomes.
                // Some providers emit no text deltas; use the completed message instead.
                if !printed_text {
                    if let Some(blocks) = message["content"].as_array() {
                        for block in blocks {
                            if block["type"] == "text" {
                                if let Some(text) = block["text"].as_str() {
                                    if pending_newline {
                                        writeln!(display)?;
                                        pending_newline = false;
                                    }
                                    if !printed_text && !pending_newline {
                                        display.split_timeline()?;
                                        ui.assistant(display)?;
                                    }
                                    write!(display, "{text}")?;
                                    printed_text = true;
                                }
                            }
                        }
                        display.flush()?;
                    }
                }
                if printed_text {
                    pending_newline = true;
                    printed_text = false;
                }
            }
            Some("tool_execution_start") if kind == "prompt" => {
                show_reasoning(&ui, display, &mut reasoning_since)?;
                let name = event["toolName"].as_str().unwrap_or("unknown");
                let label = tool_label(name, &event["args"]);
                if let Some(call_id) = event["toolCallId"].as_str() {
                    active_tools.insert(call_id.to_owned(), label.clone());
                }
                display.set_work(&format!("Running {label}…"))?;
                if interactive {
                    if printed_text || pending_newline {
                        writeln!(display)?;
                        pending_newline = false;
                    }
                    let path = label.split_once(" · ").map_or("", |(_, path)| path);
                    if name != "goal"
                        && !display.tool_start(
                            event["toolCallId"].as_str().unwrap_or(""),
                            name,
                            path,
                        )?
                    {
                        ui.info(display, &format!("↳ {label} …"))?;
                    }
                    display.flush()?;
                } else {
                    eprintln!("[tool: {name}]");
                }
            }
            Some("tool_execution_end") if kind == "prompt" => {
                let name = event["toolName"].as_str().unwrap_or("tool");
                let label = event["toolCallId"]
                    .as_str()
                    .and_then(|id| active_tools.remove(id))
                    .unwrap_or_else(|| safe_detail(name));
                display.set_work("Working…")?;
                if name == "goal" && event["isError"] != true {
                    display.goal(&event["result"]["details"])?;
                }
                if interactive && name != "goal" {
                    let outcome = if event["isError"] == true {
                        "✗ failed"
                    } else {
                        "✓ done"
                    };
                    let path = label.split_once(" · ").map_or("", |(_, path)| path);
                    let diff = (name == "edit" && event["isError"] != true)
                        .then(|| event["result"]["details"]["diff"].as_str())
                        .flatten();
                    if !display.tool_end(
                        event["toolCallId"].as_str().unwrap_or(""),
                        name,
                        path,
                        event["isError"] == true,
                        diff,
                    )? {
                        ui.info(display, &format!("{outcome} · {label}"))?;
                        if name == "edit" && event["isError"] != true {
                            show_edit_diff(&ui, display, &label, &event["result"])?;
                        }
                    }
                    display.flush()?;
                }
            }
            Some("compaction_start") if kind == "prompt" => {
                display.set_work("Compacting context…")?
            }
            Some("compaction_end") if event["reason"] != "overflow" => {
                if let Some(message) = event["errorMessage"].as_str() {
                    let error = ChatError::new(ErrorSource::Compaction, message);
                    if interactive {
                        writeln!(display)?;
                        ui.info(display, &format!("Warning: {error}"))?;
                    } else {
                        eprintln!("pi: {error}");
                    }
                } else if event["aborted"] == true && interactive && !interrupted {
                    ui.info(
                        display,
                        "Context compaction cancelled; chat remains available.",
                    )?;
                }
            }
            Some("auto_retry_start" | "summarization_retry_scheduled") if kind == "prompt" => {
                let attempt = event["attempt"].as_u64().unwrap_or(0);
                let max = event["maxAttempts"].as_u64().unwrap_or(0);
                let delay = event["delayMs"].as_u64().unwrap_or(0) / 1000;
                display.set_work(&format!("Pi retry {attempt}/{max} in {delay}s…"))?;
            }
            Some("summarization_retry_attempt_start") if kind == "prompt" => {
                display.set_work("Retrying context summary…")?;
            }
            Some("extension_ui_request" | "extension_error") => {
                // Full-screen dialogs render through the shared cached modal
                // component; only line mode uses direct terminal prompts.
                dialogs.handle(input, &event, fallback, display, raw, events, interactive)?;
                display.flush()?;
            }
            Some("agent_settled") if kind == "prompt" => {
                show_reasoning(&ui, display, &mut reasoning_since)?;
                settled = true;
                if accepted
                    && pending_queued.is_empty()
                    && queued_count == 0
                    && (!interrupted || (abort_done && clear_done))
                {
                    return finish_turn(
                        &ui,
                        display,
                        printed_text || pending_newline,
                        run.finish(interrupted),
                        interrupted,
                    );
                }
            }
            _ => {}
        }
        if abort_done && clear_done {
            if let Some(error) = rejection.take() {
                return Err(error.into());
            }
        }
        // Pi may emit the final queue_update after agent_settled. Only leave
        // once both the final settlement and all correlated queue responses
        // (and queue counts) agree there is no more work.
        if kind == "prompt"
            && accepted
            && settled
            && pending_queued.is_empty()
            && queued_count == 0
            && (!interrupted || (abort_done && clear_done))
        {
            return finish_turn(
                &ui,
                display,
                printed_text || pending_newline,
                run.finish(interrupted),
                interrupted,
            );
        }
    }
}

fn preserve_uncertain_queue(
    display: &mut impl ScrollDisplay,
    pending: &mut HashMap<String, DraftSubmission>,
) -> io::Result<()> {
    // Recovery has one explicit /restore slot. Keep the newest unacknowledged
    // submission, without replaying it or overwriting newer live typing.
    let latest = pending
        .keys()
        .max_by_key(|id| {
            id.rsplit('-')
                .next()
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0)
        })
        .cloned();
    if let Some(submission) = latest.and_then(|id| pending.remove(&id)) {
        display.rejected_queue(
            submission,
            "Delivery uncertain: RPC connection failed; check history before resending",
        )?;
    }
    Ok(())
}

fn show_reasoning<W: ScrollDisplay>(
    ui: &Ui,
    display: &mut W,
    since: &mut Option<Instant>,
) -> Result<()> {
    if let Some(start) = since.take() {
        if ui.interactive {
            let seconds = start.elapsed().as_secs();
            let duration = if seconds == 0 {
                "<1s".into()
            } else {
                format!("{seconds}s")
            };
            if !display.thinking_end()? {
                ui.info(display, &format!("reasoning · {duration}"))?;
            }
            display.flush()?;
        }
    }
    Ok(())
}

/// Pi's built-in edit tool supplies the authoritative post-edit diff here.
/// Never synthesize a preview from requested edits or display one on failure.
fn show_edit_diff<W: Write>(ui: &Ui, display: &mut W, label: &str, result: &Value) -> Result<()> {
    let Some(diff) = result["details"]["diff"]
        .as_str()
        .filter(|diff| !diff.is_empty())
    else {
        return Ok(());
    };
    let path = label.strip_prefix("edit · ").unwrap_or(label);
    ui.info(display, &format!("diff · {path}"))?;
    const MAX_LINES: usize = 40;
    let mut remaining = 0;
    for (index, line) in diff.lines().enumerate() {
        if index < MAX_LINES {
            writeln!(display, "    {}", safe_diff_line(line))?;
        } else {
            remaining += 1;
        }
    }
    if remaining > 0 {
        ui.info(display, &format!("… {remaining} more diff lines"))?;
    }
    Ok(())
}

fn safe_diff_line(line: &str) -> String {
    line.chars()
        .filter(|ch| !ch.is_control())
        .take(180)
        .collect()
}

fn safe_detail(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control())
        .take(70)
        .collect()
}

fn tool_label(name: &str, args: &Value) -> String {
    let name = safe_detail(name);
    if matches!(name.as_str(), "read" | "write" | "edit") {
        if let Some(path) = args["path"].as_str().or_else(|| args["filePath"].as_str()) {
            return format!("{name} · {}", safe_detail(path));
        }
    }
    name
}

#[allow(clippy::too_many_arguments)]
fn maybe_interrupt(
    input: &mut impl WireWrite,
    display: &mut impl ScrollDisplay,
    events: Option<&Receiver<u8>>,
    kind: &str,
    id: &str,
    accepted: bool,
    settled: bool,
    interrupted: &mut bool,
    clear_id: &mut Option<String>,
    abort_id: &mut Option<String>,
    pending_queued: &mut HashMap<String, DraftSubmission>,
    next_queue_id: &mut u64,
    queued_count: usize,
) -> Result<()> {
    let stop = if kind == "prompt" && !settled && !*interrupted {
        match events {
            Some(rx) if display.live_input() => poll_live_input(
                input,
                display,
                rx,
                id,
                accepted,
                queued_count,
                pending_queued,
                next_queue_id,
            )?,
            Some(rx) => active_keys(rx, display)?,
            None => false,
        }
    } else {
        false
    };
    if stop {
        *interrupted = true;
        *clear_id = Some(format!("{id}-clear"));
        *abort_id = Some(format!("{id}-abort"));
        writeln!(input, "{}", json!({"id":clear_id,"type":"clear_queue"}))
            .map_err(error::transport)?;
        writeln!(input, "{}", json!({"id":abort_id,"type":"abort"})).map_err(error::transport)?;
        input.flush().map_err(error::transport)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Pi stdin, terminal events and correlated queue state are independent.
fn poll_live_input(
    input: &mut impl WireWrite,
    display: &mut impl ScrollDisplay,
    events: &Receiver<u8>,
    id: &str,
    accepted: bool,
    queued_count: usize,
    pending: &mut HashMap<String, DraftSubmission>,
    sequence: &mut u64,
) -> Result<bool> {
    // Bound input work so a fast paste cannot starve Pi stdout or the spinner.
    // Each printable action may consume up to 128 already queued bytes.
    for _ in 0..8 {
        let byte = match display.pending_key().or_else(|| events.try_recv().ok()) {
            Some(byte) => byte,
            None => break,
        };
        match display.active_key(byte, events)? {
            ActiveAction::Stop => return Ok(true),
            ActiveAction::None => {}
            ActiveAction::Submit(submission) => {
                if !accepted {
                    display.hold_queue(submission)?;
                    continue;
                }
                if pending.len() + queued_count >= 16 {
                    display.rejected_queue(submission, "Too many pending queue requests")?;
                    continue;
                }
                *sequence += 1;
                let queue_id = format!("{id}-queued-{sequence}");
                let behavior = match submission.mode {
                    QueueMode::Steer => "steer",
                    QueueMode::FollowUp => "followUp",
                };
                let command = json!({"id":queue_id,"type":"prompt","message":&submission.text,"streamingBehavior":behavior});
                if let Err(error) = write_command(
                    &mut input.checked(|| service_sending(display, Some(events), true)),
                    &command,
                    &submission.images,
                ) {
                    display.rejected_queue(
                        submission,
                        "Delivery uncertain: sending failed; check history before resending",
                    )?;
                    return Err(error);
                }
                pending.insert(queue_id, submission);
            }
        }
    }
    Ok(false)
}

// Mouse and page-key CSI sequences also start with Esc. They scroll the
// transcript rather than aborting a running response.
fn active_keys(events: &Receiver<u8>, display: &mut impl ScrollDisplay) -> Result<bool> {
    while let Ok(key) = events.try_recv() {
        if key != 27 {
            continue;
        }
        match escape_key(events) {
            Navigation::Escape | Navigation::EscapeWith(_) => return Ok(true),
            nav => display.scroll(nav)?,
        }
    }
    Ok(false)
}

fn receive_record(output: &impl Events, deadline: Instant) -> Result<Option<Value>> {
    if Instant::now() >= deadline {
        return Err(error::transport(
            "Pi command response timed out; connection state is uncertain",
        )
        .into());
    }
    match output.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(event) => event.map(Some),
        Err(mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(error::transport(
            "Pi command response timed out; connection state is uncertain",
        )
        .into()),
    }
}

#[cfg(test)]
fn read_record<R: BufRead>(output: &mut R) -> Result<Option<Value>> {
    super::transport::read_frame(output, 64 * 1024 * 1024)
        .map_err(error::transport)?
        .map(|frame| {
            serde_json::from_slice(&frame)
                .map_err(|_| error::protocol("Invalid Pi RPC JSON record").into())
        })
        .transpose()
}

// A partially sent JSONL record cannot be followed by an abort command without
// corrupting framing. Esc while sending invalidates the connection instead.
fn service_sending(
    display: &mut impl ScrollDisplay,
    events: Option<&Receiver<u8>>,
    prompt: bool,
) -> io::Result<()> {
    if !prompt {
        return Ok(());
    }
    display.tick_work()?;
    let Some(events) = events else {
        return Ok(());
    };
    let stopped = if display.live_input() {
        let mut stopped = false;
        for _ in 0..8 {
            let Some(byte) = display.pending_key().or_else(|| events.try_recv().ok()) else {
                break;
            };
            match display.active_key(byte, events).map_err(io::Error::other)? {
                ActiveAction::Stop => {
                    stopped = true;
                    break;
                }
                ActiveAction::Submit(submission) => display.hold_queue(submission)?,
                ActiveAction::None => {}
            }
        }
        stopped
    } else {
        active_keys(events, display).map_err(io::Error::other)?
    };
    if stopped {
        Err(io::Error::other("Stopped while sending RPC command; delivery is uncertain. Reconnect and review before resending"))
    } else {
        Ok(())
    }
}

/// Serialize borrowed attachments rather than cloning base64 into another
/// Value. Buffer small JSON fragments, then flush exactly one LF-framed command.
fn write_command(input: &mut impl Write, command: &Value, images: &[SharedImage]) -> Result<()> {
    struct Envelope<'a> {
        command: &'a Value,
        images: &'a [SharedImage],
    }
    impl serde::Serialize for Envelope<'_> {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            use serde::ser::{Error, SerializeMap};
            let fields = self
                .command
                .as_object()
                .ok_or_else(|| S::Error::custom("command must be an object"))?;
            let mut map = serializer.serialize_map(None)?;
            for (key, value) in fields {
                map.serialize_entry(key, value)?;
            }
            if !self.images.is_empty() {
                map.serialize_entry("images", self.images)?;
            }
            map.end()
        }
    }
    let mut writer = std::io::BufWriter::new(input);
    let result = (|| {
        serde_json::to_writer(&mut writer, &Envelope { command, images })
            .map_err(error::transport)?;
        writer.write_all(b"\n").map_err(error::transport)?;
        writer.flush().map_err(error::transport)?;
        Ok(())
    })();
    // BufWriter's Drop otherwise retries a failed flush. Once command delivery
    // is uncertain, only the caller's explicit recovery may send more bytes.
    let _ = writer.into_parts();
    result
}

fn finish_turn<W: Write>(
    ui: &Ui,
    display: &mut W,
    has_text: bool,
    outcome: RunOutcome,
    interrupted: bool,
) -> Result<bool> {
    if has_text {
        writeln!(display)?;
    }
    if let RunOutcome::Failed(error) = outcome {
        return Err(error.into());
    }
    let done = interrupted;
    if interrupted {
        ui.info(display, "Stopped. You can keep chatting.")?;
    }
    if ui.interactive {
        writeln!(display)?;
    }
    Ok(done)
}

fn rejected(event: &Value, kind: &str, id: &str) -> ChatError {
    let mut error = ChatError::new(
        ErrorSource::Command,
        &format!(
            "Pi rejected {kind}: {}",
            event["error"].as_str().unwrap_or("Unknown error")
        ),
    );
    error.command_id = Some(id.into());
    error
}

fn check_record(event: &Value) -> Result<()> {
    let kind = event["type"]
        .as_str()
        .ok_or_else(|| error::protocol("Pi record has no event type"))?;
    if kind == "response" && !event["success"].is_boolean() {
        return Err(error::protocol("Pi response has no success flag").into());
    }
    if kind == "response" && event["command"] == "parse" && event["success"] == false {
        return Err(error::protocol("Pi rejected RPC framing").into());
    }
    if kind == "message_end"
        && event["message"]["role"] == "assistant"
        && !event["message"]["stopReason"].is_string()
    {
        return Err(error::protocol("Assistant message missing stopReason").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_unacknowledged_queue_is_preserved_for_explicit_review() {
        let mut screen = crate::tui::screen::Screen::new(false).unwrap();
        let mut pending = HashMap::new();
        for n in [2, 10] {
            pending.insert(
                format!("hibiscus-1-queued-{n}"),
                DraftSubmission {
                    text: format!("draft {n}"),
                    images: vec![json!({"type":"image","data":"synthetic"}).into()],
                    mode: QueueMode::Steer,
                },
            );
        }
        preserve_uncertain_queue(&mut screen, &mut pending).unwrap();
        let (text, images) = screen.take_rejected_queue().unwrap();
        assert_eq!(text, "draft 10");
        assert_eq!(images[0]["data"], "synthetic");
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn borrowed_images_serialize_as_one_jsonl_record_and_preserve_originals() {
        let command = json!({"id":"request","type":"prompt","message":"line one\nline two"});
        let images: Vec<SharedImage> = vec![
            json!({"type":"image","mimeType":"image/png","data":"a".repeat(1024 * 1024)}).into(),
        ];
        let pointer = images[0]["data"].as_str().unwrap().as_ptr();
        let recovery = images.clone();
        assert!(std::sync::Arc::ptr_eq(&images[0], &recovery[0]));
        let mut wire = Vec::new();
        write_command(&mut wire, &command, &images).unwrap();
        assert_eq!(wire.iter().filter(|&&byte| byte == b'\n').count(), 1);
        let decoded: Value = serde_json::from_slice(&wire).unwrap();
        assert_eq!(decoded["images"], json!(images));
        assert_eq!(decoded["message"], command["message"]);
        assert_eq!(images[0]["data"].as_str().unwrap().as_ptr(), pointer);
        assert!(command.get("images").is_none());
        wire.clear();
        write_command(&mut wire, &command, &[]).unwrap();
        assert!(serde_json::from_slice::<Value>(&wire)
            .unwrap()
            .get("images")
            .is_none());
    }

    #[test]
    fn buffered_command_does_not_retry_a_failed_write_on_drop() {
        struct FailsOnce(usize);
        impl Write for FailsOnce {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0 += 1;
                if self.0 == 1 {
                    Err(io::Error::other("uncertain write"))
                } else {
                    Ok(bytes.len())
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = FailsOnce(0);
        assert!(
            write_command(&mut writer, &json!({"type":"prompt","message":"test"}), &[]).is_err()
        );
        assert_eq!(writer.0, 1, "never implicitly retry a buffered RPC command");
    }

    #[test]
    fn edit_preview_uses_result_details_and_limits_output() {
        let diff = (0..45)
            .map(|n| format!("+{n} line"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut output = Vec::new();
        show_edit_diff(
            &Ui::new(false),
            &mut output,
            "edit · file.rs",
            &json!({"details":{"diff":diff}}),
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("diff · file.rs"));
        assert!(output.contains("+39 line"));
        assert!(!output.contains("+40 line"));
        assert!(output.contains("5 more diff lines"));
        let mut empty = Vec::new();
        show_edit_diff(
            &Ui::new(false),
            &mut empty,
            "edit · file.rs",
            &json!({"details":{}}),
        )
        .unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn tool_labels_include_file_paths_but_not_shell_commands_or_controls() {
        assert_eq!(
            tool_label("read", &json!({"path":"src/README.md"})),
            "read · src/README.md"
        );
        assert_eq!(
            tool_label("write", &json!({"path":"bad\u{1b}[2J\nname"})),
            "write · bad[2Jname"
        );
        assert_eq!(
            tool_label("bash", &json!({"command":"echo SECRET"})),
            "bash"
        );
    }

    #[test]
    fn mouse_wheel_escape_does_not_abort() {
        struct ScrollSpy(Vec<Navigation>);
        impl Write for ScrollSpy {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        impl ScrollDisplay for ScrollSpy {
            fn scroll(&mut self, key: Navigation) -> io::Result<()> {
                self.0.push(key);
                Ok(())
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        for byte in b"\x1b[<64;10;10M\x1b[5~" {
            tx.send(*byte).unwrap();
        }
        let mut display = ScrollSpy(Vec::new());
        assert!(!active_keys(&rx, &mut display).unwrap());
        assert_eq!(
            display.0,
            [Navigation::MouseScroll(3, 10), Navigation::ScrollPage(1)]
        );
        tx.send(27).unwrap();
        assert!(active_keys(&rx, &mut display).unwrap());
    }

    fn records(text: &str) -> Receiver<Result<Value>> {
        let (tx, rx) = mpsc::channel();
        for line in text.lines() {
            tx.send(Ok(serde_json::from_str(line).unwrap())).unwrap();
        }
        rx
    }

    fn run_exchange(
        text: &str,
        id: &str,
        kind: &str,
        sent: &mut Vec<u8>,
        shown: &mut Vec<u8>,
    ) -> Result<bool> {
        exchange(
            sent,
            &records(text),
            shown,
            &mut io::Cursor::new(Vec::<u8>::new()),
            &mut Dialogs,
            None,
            id,
            kind,
            &mut None,
            false,
        )
    }

    #[test]
    fn prompt_stream_waits_for_settled_and_supports_next_command() {
        let events = concat!(
            "{\"type\":\"response\",\"id\":\"other\",\"success\":true}\n",
            "{\"type\":\"response\",\"id\":\"hibiscus-1\",\"success\":true}\n",
            "{\"type\":\"message_update\",\"assistantMessageEvent\":{\"type\":\"text_delta\",\"delta\":\"hello\"}}\n",
            "{\"type\":\"agent_end\"}\n",
            "{\"type\":\"message_update\",\"assistantMessageEvent\":{\"type\":\"text_delta\",\"delta\":\" world\"}}\n",
            "{\"type\":\"agent_settled\"}\n",
            "{\"type\":\"response\",\"id\":\"hibiscus-2\",\"success\":true,\"data\":{\"cancelled\":false}}\n"
        );
        let mut sent = Vec::new();
        let mut shown = Vec::new();
        let rx = records(events);
        let mut dialogs = Dialogs;
        let mut fallback = io::Cursor::new(Vec::<u8>::new());
        exchange(
            &mut sent,
            &rx,
            &mut shown,
            &mut fallback,
            &mut dialogs,
            None,
            "hibiscus-1",
            "prompt",
            &mut None,
            false,
        )
        .unwrap();
        assert_eq!(shown, b"hello world\n");
        exchange(
            &mut sent,
            &rx,
            &mut shown,
            &mut fallback,
            &mut dialogs,
            None,
            "hibiscus-2",
            "new_session",
            &mut None,
            false,
        )
        .unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn rejected_prompt_and_failed_agent_are_errors() {
        let rejected = b"{\"type\":\"response\",\"id\":\"hibiscus-1\",\"success\":false,\"error\":\"no model\"}\n";
        assert!(run_exchange(
            std::str::from_utf8(rejected).unwrap(),
            "hibiscus-1",
            "prompt",
            &mut Vec::new(),
            &mut Vec::new()
        )
        .unwrap_err()
        .to_string()
        .contains("no model"));
        let failed = concat!(
            "{\"type\":\"response\",\"id\":\"hibiscus-1\",\"success\":true}\n",
            "{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"error\"}}\n",
            "{\"type\":\"agent_settled\"}\n"
        );
        assert!(run_exchange(
            failed,
            "hibiscus-1",
            "prompt",
            &mut Vec::new(),
            &mut Vec::new()
        )
        .is_err());
    }

    #[test]
    fn extension_dialog_is_cancelled_without_terminal() {
        let events = concat!(
            "{\"type\":\"extension_ui_request\",\"id\":\"dialog-1\",\"method\":\"confirm\"}\n",
            "{\"type\":\"response\",\"id\":\"hibiscus-1\",\"success\":true}\n",
            "{\"type\":\"agent_settled\"}\n"
        );
        let mut sent = Vec::new();
        run_exchange(events, "hibiscus-1", "prompt", &mut sent, &mut Vec::new()).unwrap();
        let response: Value = serde_json::from_slice(&sent).unwrap();
        assert_eq!(response["id"], "dialog-1");
        assert_eq!(response["cancelled"], true);
    }

    #[test]
    fn escape_sends_clear_then_abort_and_preserves_session() {
        let events = concat!(
            "{\"type\":\"response\",\"id\":\"hibiscus-1\",\"success\":true}\n",
            "{\"type\":\"response\",\"id\":\"hibiscus-1-clear\",\"success\":true}\n",
            "{\"type\":\"response\",\"id\":\"hibiscus-1-abort\",\"success\":true}\n",
            "{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"aborted\"}}\n",
            "{\"type\":\"agent_settled\"}\n",
            "{\"type\":\"response\",\"id\":\"hibiscus-2\",\"success\":true,\"data\":{\"cancelled\":false}}\n"
        );
        let rx = records(events);
        let (tx, keys) = mpsc::channel();
        tx.send(27).unwrap();
        let mut sent = Vec::new();
        let mut shown = Vec::new();
        let mut fallback = io::Cursor::new(Vec::<u8>::new());
        let mut dialogs = Dialogs;
        assert!(exchange(
            &mut sent,
            &rx,
            &mut shown,
            &mut fallback,
            &mut dialogs,
            Some(&keys),
            "hibiscus-1",
            "prompt",
            &mut None,
            false
        )
        .unwrap());
        assert!(!exchange(
            &mut sent,
            &rx,
            &mut shown,
            &mut fallback,
            &mut dialogs,
            None,
            "hibiscus-2",
            "new_session",
            &mut None,
            false
        )
        .unwrap());
        let commands: Vec<Value> = sent
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(
            commands
                .iter()
                .map(|cmd| cmd["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["clear_queue", "abort"]
        );
    }

    #[test]
    fn rejected_preflight_drains_already_sent_cancellation_before_returning() {
        let rx = records(concat!(
            "{\"type\":\"response\",\"id\":\"p\",\"success\":false,\"error\":\"no model\"}\n",
            "{\"type\":\"response\",\"id\":\"p-clear\",\"success\":true}\n",
            "{\"type\":\"response\",\"id\":\"p-abort\",\"success\":true}\n",
            "{\"type\":\"response\",\"id\":\"next\",\"success\":true}\n"
        ));
        let (sender, keys) = mpsc::channel();
        sender.send(27).unwrap();
        assert!(exchange(
            &mut Vec::new(),
            &rx,
            &mut Vec::new(),
            &mut io::Cursor::new(Vec::<u8>::new()),
            &mut Dialogs,
            Some(&keys),
            "p",
            "prompt",
            &mut None,
            false
        )
        .is_err());
        assert_eq!(rx.recv().unwrap().unwrap()["id"], "next");
    }

    #[test]
    fn silent_command_deadline_and_invalid_records_require_reconnection() {
        let (_sender, receiver) = mpsc::channel();
        let error = receive_record(&receiver, Instant::now()).unwrap_err();
        assert_eq!(
            error.downcast_ref::<ChatError>().unwrap().recovery(),
            super::error::RecoveryAction::Reconnect
        );
        for record in [
            json!({}),
            json!({"type":"response","success":"yes"}),
            json!({"type":"response","success":false,"command":"parse"}),
        ] {
            assert!(check_record(&record).is_err());
        }
        assert!(check_record(&json!({"type":"future_event"})).is_ok());
        let error = read_record(&mut b"private invalid payload\n".as_slice()).unwrap_err();
        assert!(!error.to_string().contains("private invalid payload"));
    }

    #[test]
    fn strict_lf_framing_accepts_unicode_separators() {
        let mut stream =
            "{\"type\":\"message_update\",\"text\":\"a\u{2028}b\"}\n{\"type\":\"agent_settled\"}\n"
                .as_bytes();
        assert_eq!(
            read_record(&mut stream).unwrap().unwrap()["text"],
            "a\u{2028}b"
        );
        assert_eq!(
            read_record(&mut stream).unwrap().unwrap()["type"],
            "agent_settled"
        );
    }
}
