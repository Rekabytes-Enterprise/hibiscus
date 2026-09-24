use crate::{
    pi::{configure_builtin_tools, dialog::Dialogs},
    tui::{
        screen::{escape_key, Navigation, ScrollDisplay},
        terminal::RawMode,
        ui::Ui,
    },
    Result,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Instant;

pub(crate) enum SessionStart {
    New,
    Latest,
    Selected(PathBuf),
}

pub(crate) struct Rpc {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<Result<Value>>,
    diagnostics: Option<JoinHandle<io::Result<u64>>>,
    next_id: u64,
}

impl Rpc {
    pub(crate) fn start(start: SessionStart) -> Result<Self> {
        let pi = env::var("HIBISCUS_PI").unwrap_or_else(|_| "pi".to_owned());
        let mut command = Command::new(&pi);
        command.args(["--mode", "rpc"]);
        configure_builtin_tools(&mut command);
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
            .spawn()
            .map_err(|error| {
                format!("could not start {pi}: {error} (install Pi or set HIBISCUS_PI)")
            })?;
        let input = child.stdin.take().ok_or("pi stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("pi stdout unavailable")?;
        let (sender, output) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_record(&mut reader) {
                    Ok(Some(event)) => {
                        if sender.send(Ok(event)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        let stderr = child.stderr.take().ok_or("pi stderr unavailable")?;
        let diagnostics =
            thread::spawn(move || io::copy(&mut BufReader::new(stderr), &mut io::stderr()));
        Ok(Self {
            child,
            input: Some(input),
            output,
            diagnostics: Some(diagnostics),
            next_id: 0,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn command<R: BufRead, W: ScrollDisplay>(
        &mut self,
        command: Value,
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
        let input = self.input.as_mut().ok_or("pi stdin unavailable")?;
        writeln!(input, "{command}")?;
        input.flush()?;

        if kind == "prompt" {
            display.start_work()?;
        }
        let result = exchange(
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
        );
        if kind == "prompt" {
            let cleanup = display.stop_work();
            if result.is_ok() {
                cleanup?;
            }
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
    pub(crate) fn request<R: BufRead, W: Write>(
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
        let writer = self.input.as_mut().ok_or("pi stdin unavailable")?;
        writeln!(writer, "{command}")?;
        writer.flush()?;
        loop {
            let record = self
                .read_record()?
                .ok_or("pi RPC stream ended before the command completed")?;
            if record["type"] == "response" && record["id"] == id {
                if record["success"] != true {
                    return Err(format!(
                        "pi rejected {kind}: {}",
                        record["error"].as_str().unwrap_or("unknown error")
                    )
                    .into());
                }
                return Ok(record["data"].clone());
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
        }
    }

    fn read_record(&mut self) -> Result<Option<Value>> {
        match self.output.recv() {
            Ok(event) => event.map(Some),
            Err(_) => Ok(None),
        }
    }

    /// Stop the idle RPC child before another Pi process opens its session.
    pub(crate) fn close(&mut self) -> Result<()> {
        if self.input.is_none() {
            return Ok(());
        }
        drop(self.input.take());
        let status = self.child.wait()?;
        if let Some(reader) = self.diagnostics.take() {
            let _ = reader.join();
        }
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
        if outcome.is_err() {
            let _ = self.child.kill();
        }
        drop(self.input.take());
        let status = self.child.wait()?;
        if let Some(reader) = self.diagnostics.take() {
            let _ = reader.join();
        }
        outcome?;
        if !status.success() {
            return Err(format!("pi exited with {status}").into());
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)] // Pi RPC and terminal streams must remain independent.
fn exchange<F: BufRead, W: ScrollDisplay>(
    input: &mut impl Write,
    output: &Receiver<Result<Value>>,
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
    let mut failed = false;
    let mut interrupted = false;
    let mut abort_id = None;
    let mut abort_done = false;
    let mut clear_id = None;
    let mut clear_done = false;
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
            settled,
            &mut interrupted,
            &mut clear_id,
            &mut abort_id,
        )?;
        let event = match output.recv_timeout(std::time::Duration::from_millis(40)) {
            Ok(event) => event?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("pi RPC stream ended before the command completed".into())
            }
        };
        if event["type"] == "response" {
            if clear_id.as_deref() == event["id"].as_str() {
                clear_done = true;
            }
            if abort_id.as_deref() == event["id"].as_str() {
                abort_done = true;
            }
        }
        match event["type"].as_str() {
            Some("response") if event["id"] == id => {
                if event["success"] != true {
                    return Err(format!(
                        "pi rejected {kind}: {}",
                        event["error"].as_str().unwrap_or("unknown error")
                    )
                    .into());
                }
                accepted = true;
                if kind != "prompt" {
                    if event["data"]["cancelled"] == true {
                        return Err(format!("pi cancelled {kind}").into());
                    }
                    return Ok(false);
                }
                if settled && (!interrupted || (abort_done && clear_done)) {
                    return finish_turn(
                        &ui,
                        display,
                        printed_text || pending_newline,
                        failed && !interrupted,
                        interrupted,
                    );
                }
            }
            Some("message_update") if kind == "prompt" => {
                let update = &event["assistantMessageEvent"];
                match update["type"].as_str() {
                    Some("thinking_start" | "thinking_delta") => {
                        reasoning_since.get_or_insert_with(Instant::now);
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
                            if pending_newline {
                                writeln!(display)?;
                                pending_newline = false;
                            }
                            if !printed_text && !pending_newline {
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
                if matches!(message["stopReason"].as_str(), Some("error" | "aborted")) {
                    failed = true;
                    if !interrupted {
                        if let Some(error) = message["errorMessage"].as_str() {
                            eprintln!("pi: {error}");
                        }
                    }
                }
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
                    ui.info(display, &format!("↳ {label} …"))?;
                    display.flush()?;
                } else {
                    eprintln!("[tool: {name}]");
                }
            }
            Some("tool_execution_end") if kind == "prompt" => {
                let label = event["toolCallId"]
                    .as_str()
                    .and_then(|id| active_tools.remove(id))
                    .unwrap_or_else(|| safe_detail(event["toolName"].as_str().unwrap_or("tool")));
                display.set_work("Thinking…")?;
                if interactive {
                    let outcome = if event["isError"] == true {
                        "✗ failed"
                    } else {
                        "✓ done"
                    };
                    ui.info(display, &format!("{outcome} · {label}"))?;
                    if event["toolName"] == "edit" && event["isError"] != true {
                        show_edit_diff(&ui, display, &label, &event["result"])?;
                    }
                    display.flush()?;
                }
            }
            Some("compaction_start") if kind == "prompt" => {
                display.set_work("Compacting context…")?
            }
            Some("auto_retry_start") if kind == "prompt" => {
                display.set_work("Retrying response…")?
            }
            Some("extension_ui_request") => {
                dialogs.handle(input, &event, fallback, display, raw, events, interactive)?;
                display.flush()?;
            }
            Some("agent_settled") if kind == "prompt" => {
                show_reasoning(&ui, display, &mut reasoning_since)?;
                settled = true;
                if accepted && (!interrupted || (abort_done && clear_done)) {
                    return finish_turn(
                        &ui,
                        display,
                        printed_text || pending_newline,
                        failed && !interrupted,
                        interrupted,
                    );
                }
            }
            _ => {}
        }
        if kind == "prompt" && accepted && settled && interrupted && abort_done && clear_done {
            return finish_turn(&ui, display, printed_text || pending_newline, false, true);
        }
    }
}

fn show_reasoning<W: Write>(ui: &Ui, display: &mut W, since: &mut Option<Instant>) -> Result<()> {
    if let Some(start) = since.take() {
        if ui.interactive {
            let seconds = start.elapsed().as_secs();
            let duration = if seconds == 0 {
                "<1s".into()
            } else {
                format!("{seconds}s")
            };
            ui.info(display, &format!("reasoning · {duration}"))?;
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
    input: &mut impl Write,
    display: &mut impl ScrollDisplay,
    events: Option<&Receiver<u8>>,
    kind: &str,
    id: &str,
    settled: bool,
    interrupted: &mut bool,
    clear_id: &mut Option<String>,
    abort_id: &mut Option<String>,
) -> Result<()> {
    let stop = if kind == "prompt" && !settled && !*interrupted {
        events
            .map(|rx| active_keys(rx, display))
            .transpose()?
            .unwrap_or(false)
    } else {
        false
    };
    if stop {
        *interrupted = true;
        *clear_id = Some(format!("{id}-clear"));
        *abort_id = Some(format!("{id}-abort"));
        writeln!(input, "{}", json!({"id":clear_id,"type":"clear_queue"}))?;
        writeln!(input, "{}", json!({"id":abort_id,"type":"abort"}))?;
        input.flush()?;
    }
    Ok(())
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

fn read_record<R: BufRead>(output: &mut R) -> Result<Option<Value>> {
    let mut line = String::new();
    if output.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let event = serde_json::from_str(line.trim_end_matches(['\r', '\n']))
        .map_err(|error| format!("invalid Pi RPC record: {error}"))?;
    Ok(Some(event))
}

fn finish_turn<W: Write>(
    ui: &Ui,
    display: &mut W,
    has_text: bool,
    failed: bool,
    interrupted: bool,
) -> Result<bool> {
    let done = finish_prompt(display, has_text, failed, interrupted)?;
    if interrupted {
        ui.info(display, "Stopped. You can keep chatting.")?;
    }
    if ui.interactive {
        writeln!(display)?;
    }
    Ok(done)
}

fn finish_prompt<W: Write>(
    display: &mut W,
    has_text: bool,
    failed: bool,
    interrupted: bool,
) -> Result<bool> {
    if has_text {
        writeln!(display)?;
    }
    if failed {
        return Err("pi agent failed or was aborted".into());
    }
    Ok(interrupted)
}

#[cfg(test)]
mod tests {
    use super::*;
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
