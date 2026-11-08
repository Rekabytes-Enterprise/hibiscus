mod display;
mod live_input;
mod turn;
mod wire;
use display::*;
use live_input::*;
use turn::*;
use wire::*;

use super::{
    diagnostics::Diagnostics,
    error::{self, ChatError, ErrorSource},
    run::{RunOutcome, RunState},
    transport::{self, Events, Inbox, Limits, PipeWriter, WireWrite},
};
use crate::{
    ui::{
        dialog::Dialogs,
        display::{ActiveAction, DraftSubmission, QueueMode, ScrollDisplay, SharedImage},
        input::{escape_key, Navigation},
        messages::Ui,
        terminal::RawMode,
        tool,
    },
    Result,
};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::os::{fd::AsRawFd, unix::net::UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Instant;

pub(crate) use crate::engine::SessionStart;

pub(crate) struct Client {
    worker: Option<JoinHandle<()>>,
    control: UnixStream,
    input: Option<PipeWriter<UnixStream>>,
    output: Inbox,
    next_id: u64,
    resume: SessionStart,
    disconnected: bool,
    stats_supported: bool,
}

impl Client {
    pub(crate) fn start(start: SessionStart) -> Result<Self> {
        Self::start_with_diagnostics(start, None)
    }

    pub(crate) fn start_with_diagnostics(
        start: SessionStart,
        diagnostic_feed: Option<Diagnostics>,
    ) -> Result<Self> {
        let config = crate::engine::config::load()?;
        let session = crate::engine::session::Session::open(&start, &config)?;
        let (client, worker_socket) = UnixStream::pair()?;
        let control = client.try_clone()?;
        let output = Inbox::start(SocketReader(client.try_clone()?), Limits::from_env()?);
        let input = output.writer(client)?;
        let worker = thread::spawn(move || {
            if let Err(error) = crate::engine::agent::serve(worker_socket, config, session) {
                if let Some(feed) = diagnostic_feed {
                    feed.record(error.to_string().as_bytes(), false);
                }
            }
        });
        Ok(Self {
            worker: Some(worker),
            control,
            input: Some(input),
            output,
            next_id: 0,
            resume: start,
            disconnected: false,
            stats_supported: true,
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
        if kind == "extension_command" {
            display.start_command(&format!(
                "Running {}",
                safe_detail(command["name"].as_str().unwrap_or("extension"))
            ))?;
        }
        let sending = write_command(
            &mut input.checked(|| {
                service_sending(
                    display,
                    events,
                    matches!(kind.as_str(), "prompt" | "extension_command"),
                )
            }),
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
        if matches!(kind.as_str(), "prompt" | "extension_command") {
            display.stop_work()?;
            if kind == "prompt" && result.as_ref().is_ok_and(|stopped| !stopped) {
                display.complete_diagrams()?;
            }
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
            let mut record = receive_record(&self.output, deadline, display)?
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
            if record["type"] == "thinking_reset" {
                display.thinking_reset()?;
                continue;
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

    /// Runtime owns summarization and persistence. Unlike metadata requests, a
    /// manual compact waits for a model call's matching response, not a 30s
    /// command deadline or an agent_settled notification.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compact<R: BufRead, W: ScrollDisplay>(
        &mut self,
        instructions: Option<&str>,
        display: &mut W,
        fallback: &mut R,
        dialogs: &mut Dialogs,
        events: Option<&Receiver<u8>>,
        raw: &mut Option<RawMode>,
        interactive: bool,
    ) -> Result<Value> {
        self.next_id += 1;
        let id = format!("hibiscus-{}", self.next_id);
        let mut command = json!({"id":id,"type":"compact"});
        if let Some(instructions) = instructions {
            command["customInstructions"] = json!(instructions);
        }
        let writer = self
            .input
            .as_mut()
            .ok_or_else(|| error::transport("stdin unavailable"))?;
        write_command(writer, &command, &[])?;
        display.compaction_mode(true)?;
        display.start_work()?;
        display.set_work("Compacting context…")?;
        let result = (|| {
            let mut idle_deadline = Instant::now() + std::time::Duration::from_secs(600);
            let mut cancelled_at = None::<Instant>;
            let mut abort_id = None::<String>;
            let mut abort_done = false;
            let mut compact_reply = None::<Value>;
            loop {
                display.tick_work()?;
                if display.live_input() && abort_id.is_none() {
                    if let Some(keys) = events {
                        // Bound input work so Hibiscus's response cannot be starved by a paste.
                        for _ in 0..32 {
                            let Some(byte) = display.pending_key().or_else(|| keys.try_recv().ok())
                            else {
                                break;
                            };
                            match display.active_key(byte, keys)? {
                                ActiveAction::None => {}
                                ActiveAction::Submit(submission) => {
                                    display.hold_queue(submission)?
                                }
                                ActiveAction::Stop => {
                                    let abort = format!("{id}-abort");
                                    writeln!(writer, "{}", json!({"id":abort,"type":"abort"}))
                                        .map_err(error::transport)?;
                                    writer.flush().map_err(error::transport)?;
                                    abort_id = Some(abort);
                                    cancelled_at = Some(Instant::now());
                                    display.set_work("Cancelling compaction…")?;
                                    break;
                                }
                            }
                        }
                    }
                }
                if cancelled_at
                    .is_some_and(|start| start.elapsed() >= std::time::Duration::from_secs(10))
                {
                    return Err(error::transport("Compaction cancellation did not finish in 10 seconds; outcome is uncertain").into());
                }
                if Instant::now() >= idle_deadline {
                    return Err(error::transport(
                        "Runtime compaction produced no response for 10 minutes; outcome is uncertain",
                    )
                    .into());
                }
                let generation = transport::activity_generation();
                let event = match self.output.recv_timeout(std::time::Duration::ZERO) {
                    Ok(event) => event?,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        transport::wait_activity(generation, std::time::Duration::from_millis(40));
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(
                            error::transport("Runtime ended before compaction completed").into(),
                        )
                    }
                };
                check_record(&event)?;
                idle_deadline = Instant::now() + std::time::Duration::from_secs(600);
                if event["type"] == "response" {
                    if abort_id.as_deref() == event["id"].as_str() {
                        if event["success"] != true {
                            return Err(error::protocol(
                                "Runtime rejected compaction cancellation; state is uncertain",
                            )
                            .into());
                        }
                        abort_done = true;
                    } else if event["id"] == id {
                        compact_reply = Some(event.clone());
                    }
                }
                if let Some(reply) = compact_reply
                    .as_ref()
                    .filter(|_| abort_id.is_none() || abort_done)
                {
                    if reply["success"] != true {
                        if cancelled_at.is_some() {
                            return Err(ChatError::cancelled("compact").into());
                        }
                        return Err(rejected(reply, "compact", &id).into());
                    }
                    let data = reply
                        .get("data")
                        .filter(|data| data.is_object())
                        .ok_or_else(|| {
                            error::protocol("Runtime compact response is missing summary data")
                        })?;
                    return Ok(data.clone());
                }
                match event["type"].as_str() {
                    Some("thinking_reset") => display.thinking_reset()?,
                    Some("provider_status") => provider_status(display, &event)?,
                    Some("compaction_start") => display.set_work("Compacting context…")?,
                    Some("compaction_end") if event["aborted"] == true => {
                        display.set_work("Compaction cancelled…")?
                    }
                    Some("extension_ui_request" | "extension_error" | "provider_error") => {
                        dialogs.handle(
                            writer,
                            &event,
                            fallback,
                            display,
                            raw,
                            events,
                            interactive,
                        )?;
                        if event["type"] == "extension_ui_request" {
                            idle_deadline = Instant::now() + std::time::Duration::from_secs(600);
                        }
                    }
                    _ => {}
                }
            }
        })();
        display.stop_work()?;
        display.compaction_mode(false)?;
        result
    }

    /// Older Runtime builds can reject this optional UI metadata command. Keep
    /// chatting without inventing numbers, but do not hide transport errors.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn session_stats<R: BufRead, W: ScrollDisplay>(
        &mut self,
        fallback: &mut R,
        display: &mut W,
        dialogs: &mut Dialogs,
        raw: &mut Option<RawMode>,
        events: Option<&Receiver<u8>>,
        interactive: bool,
    ) -> Result<Option<Value>> {
        if !self.stats_supported {
            return Ok(None);
        }
        match self.request(
            json!({"type":"get_session_stats"}),
            fallback,
            display,
            dialogs,
            raw,
            events,
            interactive,
        ) {
            Ok(stats) => Ok(Some(stats)),
            Err(error)
                if error.downcast_ref::<ChatError>().is_some_and(|error| {
                    error.source == ErrorSource::Command
                        && error.message.contains("Unknown command: get_session_stats")
                }) =>
            {
                self.stats_supported = false;
                Ok(None)
            }
            Err(error) => Err(error),
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
        self.input.take();
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        self.disconnected = true;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    pub(crate) fn recover_connection(&mut self) -> Result<()> {
        let start = self.resume.clone();
        self.disconnect();
        *self = Self::start_with_diagnostics(start, None)?;
        Ok(())
    }

    pub(crate) fn close(&mut self) -> Result<()> {
        self.disconnect();
        Ok(())
    }

    pub(crate) fn reopen(&mut self, start: SessionStart) -> Result<()> {
        if self.input.is_some() {
            return Err("pi RPC child must be closed before reopening".into());
        }
        *self = Self::start_with_diagnostics(start, None)?;
        Ok(())
    }

    /// Reopen a saved session after an external credential change.
    pub(crate) fn reconnect(&mut self, start: SessionStart) -> Result<()> {
        self.close()?;
        self.reopen(start)
    }

    pub(crate) fn finish(mut self, outcome: Result<()>) -> Result<()> {
        self.disconnect();
        outcome
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if self.input.is_some() {
            self.disconnect();
        }
    }
}

// Mouse and page-key CSI sequences also start with Esc. They scroll the
// transcript rather than aborting a running response.

// A partially sent JSONL record cannot be followed by an abort command without
// corrupting framing. Esc while sending invalidates the connection instead.

// A cloned Unix socket shares O_NONBLOCK with its writer. Poll readiness in
// the dedicated ingress thread rather than interpreting WouldBlock as EOF.
struct SocketReader(UnixStream);
impl std::io::Read for SocketReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            match std::io::Read::read(&mut self.0, bytes) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    let mut fd = libc::pollfd {
                        fd: self.0.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    if unsafe { libc::poll(&mut fd, 1, 100) } < 0 {
                        let e = io::Error::last_os_error();
                        if e.kind() != io::ErrorKind::Interrupted {
                            return Err(e);
                        }
                    }
                }
                result => return result,
            }
        }
    }
}

#[cfg(test)]
mod tests;
