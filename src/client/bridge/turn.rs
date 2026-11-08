//! Runs one turn: sends the prompt, renders events until the run settles.
use super::*;

#[allow(clippy::too_many_arguments)] // Hibiscus runtime and terminal streams must remain independent.
pub(super) fn exchange<F: BufRead, W: ScrollDisplay>(
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
    let mut active_tools = HashMap::<String, (String, tool::Call)>::new();
    let mut reasoning_since = None;
    let mut accepted = false;
    let mut settled = false;
    let mut printed_text = false;
    let mut leading_whitespace = String::new();
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
    let mut local_done = false;
    let mut next_queue_id = 0;
    let mut queued_count = 0;
    loop {
        // Capture before polling both sources so a notification arriving
        // between the polls and sleep cannot be lost.
        let generation = transport::activity_generation();
        if matches!(kind, "prompt" | "extension_command") {
            display.tick_work()?;
        }
        // Poll terminal input before consuming queued Runtime events too; a busy
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
        let event = match output.recv_timeout(std::time::Duration::ZERO) {
            Ok(Ok(event)) => event,
            Ok(Err(error)) => {
                preserve_uncertain_queue(display, &mut pending_queued)?;
                return Err(error);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Timed fallback still drives flower/Explore, elapsed status,
                // pending text frames and cancellation deadlines.
                transport::wait_activity(generation, std::time::Duration::from_millis(40));
                continue;
            }
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
                    display.rejected_queue(queued, "Runtime is stopping; queue delivery is uncertain. Check history before resending")?;
                } else if event["success"] == true {
                    display.queued(&queued)?;
                    // Acceptance is not a new agent run. A delayed ack can
                    // arrive after Hibiscus's final agent_settled; only agent_start
                    // invalidates that authoritative settlement.
                } else {
                    display.rejected_queue(
                        queued,
                        event["error"].as_str().unwrap_or("unknown error"),
                    )?;
                }
                display.flush()?;
                // A late queue ack can be the last record after Hibiscus's final
                // settlement and queue drain. This branch skips the common
                // completion check below, so finish here when all conditions
                // are already satisfied; never wait for an invented event.
                if accepted
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
                continue;
            }
        }
        if event["type"] == "response" {
            if (clear_id.as_deref() == event["id"].as_str()
                || abort_id.as_deref() == event["id"].as_str())
                && event["id"].is_string()
                && event["success"] != true
            {
                return Err(error::protocol(
                    "Runtime rejected cancellation; run state is uncertain",
                )
                .into());
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
            Some("extension_command_output") if kind == "extension_command" => {
                display.end_turn()?;
                writeln!(display, "{}", event["text"].as_str().unwrap_or(""))?;
                display.flush()?;
            }
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
                if kind == "extension_command" && interrupted && !(clear_done && abort_done) {
                    local_done = true;
                    continue;
                }
                if kind != "prompt" {
                    if event["data"]["cancelled"] == true {
                        return Err(ChatError::cancelled(kind).into());
                    }
                    return Ok(interrupted);
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
                        if let Some(delta) = update["delta"].as_str() {
                            let Some(delta) = assistant_chunk(
                                delta,
                                printed_text,
                                interactive,
                                &mut leading_whitespace,
                            ) else {
                                continue;
                            };
                            display.set_work("Writing reply…")?;
                            if !printed_text && reasoning_since.is_some() {
                                show_reasoning(&ui, display, &mut reasoning_since)?;
                            }
                            if pending_newline {
                                writeln!(display)?;
                                pending_newline = false;
                            }
                            if !printed_text && !pending_newline {
                                display.split_timeline()?;
                                if !display.assistant_start()? {
                                    ui.assistant(display)?;
                                }
                            }
                            display.assistant_text(&delta)?;
                            display.flush()?;
                            printed_text = true;
                        }
                    }
                    _ => {}
                }
            }
            Some("provider_status") => provider_status(display, &event)?,
            Some("message_end") if kind == "prompt" && event["message"]["role"] == "assistant" => {
                show_reasoning(&ui, display, &mut reasoning_since)?;
                let message = &event["message"];
                // RunState retains the latest attempt error until settlement;
                // intermediate failures must not be printed as final outcomes.
                // Some providers emit no text deltas; use the completed message instead.
                if !printed_text {
                    // Completed text already includes any streamed whitespace.
                    leading_whitespace.clear();
                    if let Some(blocks) = message["content"].as_array() {
                        for block in blocks {
                            if block["type"] == "text" {
                                if let Some(text) = block["text"].as_str() {
                                    let Some(text) = assistant_chunk(
                                        text,
                                        printed_text,
                                        interactive,
                                        &mut leading_whitespace,
                                    ) else {
                                        continue;
                                    };
                                    if pending_newline {
                                        writeln!(display)?;
                                        pending_newline = false;
                                    }
                                    if !printed_text && !pending_newline {
                                        display.split_timeline()?;
                                        if !display.assistant_start()? {
                                            ui.assistant(display)?;
                                        }
                                    }
                                    display.assistant_text(&text)?;
                                    printed_text = true;
                                }
                            }
                        }
                        display.flush()?;
                    }
                }
                leading_whitespace.clear();
                display.end_assistant_text()?;
                if printed_text {
                    pending_newline = true;
                    printed_text = false;
                }
            }
            Some("tool_execution_start") if kind == "prompt" => {
                show_reasoning(&ui, display, &mut reasoning_since)?;
                let name = event["toolName"].as_str().unwrap_or("unknown");
                let label = tool_label(name, &event["args"]);
                let call = tool_call(name, &event["args"]);
                if let Some(call_id) = event["toolCallId"].as_str() {
                    active_tools.insert(call_id.to_owned(), (label.clone(), call.clone()));
                }
                display.set_work(&format!("Running {label}…"))?;
                if interactive {
                    if printed_text || pending_newline {
                        writeln!(display)?;
                        pending_newline = false;
                    }
                    if name != "goal"
                        && !display.tool_start(event["toolCallId"].as_str().unwrap_or(""), &call)?
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
                let (label, call) = event["toolCallId"]
                    .as_str()
                    .and_then(|id| active_tools.remove(id))
                    .unwrap_or_else(|| (safe_detail(name), tool_call(name, &Value::Null)));
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
                    let diff = (matches!(name, "edit" | "checkpoint_diff")
                        && event["isError"] != true)
                        .then(|| event["result"]["details"]["diff"].as_str())
                        .flatten();
                    let output = event["result"]["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|block| block["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let result = tool::Outcome {
                        failed: event["isError"] == true,
                        diff,
                        output: &output,
                        exit_code: event["result"]["details"]["exitCode"].as_i64(),
                        changes: event["result"]["details"]["addedLines"]
                            .as_u64()
                            .zip(event["result"]["details"]["removedLines"].as_u64()),
                        elapsed: event["durationMs"]
                            .as_u64()
                            .map(std::time::Duration::from_millis),
                    };
                    if !display.tool_end(
                        event["toolCallId"].as_str().unwrap_or(""),
                        &call,
                        &result,
                    )? {
                        ui.info(display, &format!("{outcome} · {label}"))?;
                        if diff.is_some() {
                            show_tool_diff(&ui, display, &label, &event["result"])?;
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
                        eprintln!("runtime: {error}");
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
                display.set_work(&format!("Runtime retry {attempt}/{max} in {delay}s…"))?;
            }
            Some("summarization_retry_attempt_start") if kind == "prompt" => {
                display.set_work("Retrying context summary…")?;
            }
            Some("thinking_reset") => display.thinking_reset()?,
            Some("extension_ui_request" | "extension_error" | "provider_error") => {
                // Full-screen dialogs render through the shared cached modal
                // component; only line mode uses direct terminal prompts.
                dialogs.handle(input, &event, fallback, display, raw, events, interactive)?;
                display.flush()?;
            }
            Some("queue_rejected") if kind == "prompt" => {
                let images = event["images"]
                    .as_array()
                    .map(|images| images.iter().cloned().map(std::sync::Arc::new).collect())
                    .unwrap_or_default();
                display.clear_queue_display()?;
                display.rejected_queue(
                    DraftSubmission {
                        text: event["message"].as_str().unwrap_or("").into(),
                        images,
                        mode: if event["steer"] == true {
                            QueueMode::Steer
                        } else {
                            QueueMode::FollowUp
                        },
                    },
                    event["error"]
                        .as_str()
                        .unwrap_or("Queued message was not delivered"),
                )?;
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
        if kind == "extension_command" && local_done && clear_done && abort_done {
            return Ok(true);
        }
        if abort_done && clear_done {
            if let Some(error) = rejection.take() {
                return Err(error.into());
            }
        }
        // Runtime may emit the final queue_update after agent_settled. Only leave
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

pub(super) fn preserve_uncertain_queue(
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

pub(super) fn provider_status(display: &mut impl ScrollDisplay, event: &Value) -> io::Result<()> {
    if event["provider"] == "rooxcode" {
        match event["waiting"].as_bool() {
            Some(true) => display.set_work(crate::ui::display::ROOXCODE_WAITING)?,
            Some(false) => display.set_work("Preparing response…")?,
            None => {}
        }
    }
    Ok(())
}

/// Delay human-facing speaker/timeline changes until a reply has visible text.
/// Keep leading indentation across chunks, bounded independently of saved text.
pub(super) fn assistant_chunk<'a>(
    text: &'a str,
    printed: bool,
    interactive: bool,
    leading: &mut String,
) -> Option<Cow<'a, str>> {
    if !interactive || printed {
        return Some(Cow::Borrowed(text));
    }
    if text.trim().is_empty() {
        for ch in text.chars() {
            if leading.len() + ch.len_utf8() > 8192 {
                break;
            }
            leading.push(ch);
        }
        return None;
    }
    if leading.is_empty() {
        Some(Cow::Borrowed(text))
    } else {
        let mut text_with_indent = std::mem::take(leading);
        text_with_indent.push_str(text);
        Some(Cow::Owned(text_with_indent))
    }
}

pub(super) fn finish_turn<W: ScrollDisplay>(
    ui: &Ui,
    display: &mut W,
    has_text: bool,
    outcome: RunOutcome,
    interrupted: bool,
) -> Result<bool> {
    display.end_turn()?;
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

pub(super) fn rejected(event: &Value, kind: &str, id: &str) -> ChatError {
    let mut error = ChatError::new(
        if kind == "extension_command" {
            ErrorSource::Extension
        } else {
            ErrorSource::Command
        },
        &format!(
            "Runtime rejected {kind}: {}",
            event["error"].as_str().unwrap_or("Unknown error")
        ),
    );
    error.command_id = Some(id.into());
    error
}
