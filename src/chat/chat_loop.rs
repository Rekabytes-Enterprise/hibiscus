//! The interactive chat loop: reads input, dispatches slash commands and runs prompts.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn chat<R: BufRead>(
    rpc: &mut Client,
    input: &mut R,
    output: &mut Frontend,
    interactive: bool,
    dialogs: &mut Dialogs,
    events: &mut Option<TerminalEvents>,
    raw: &mut Option<RawMode>,
    ui: &Ui,
    update_notice: Option<&Receiver<Option<String>>>,
) -> Result<()> {
    let mut line = String::new();
    let mut last_failed = None;
    loop {
        if !output.is_full() {
            ui.prompt(output)?;
        }
        line.clear();
        if let (Some(events), Some(raw)) = (events.as_ref(), raw.as_mut()) {
            let text = if output.is_full() {
                match output.prompt_with_update(&events.receiver, update_notice)? {
                    PromptAction::Text(text) => text,
                    PromptAction::Update(tag) => {
                        offer_update(output, &events.receiver, &tag)?;
                        continue;
                    }
                }
            } else {
                output.read_inline(&events.receiver, raw)?
            };
            let Some(text) = text else {
                return Ok(());
            };
            line.push_str(&text);
        } else if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let message =
            if output.inline_input_style().is_some() && !line.trim_start().starts_with('/') {
                line.as_str()
            } else {
                line.trim()
            };
        let images = output.take_images();
        if output.is_full()
            && !message.starts_with('/')
            && (!message.is_empty() || !images.is_empty())
        {
            let shown = if images.is_empty() {
                message.to_owned()
            } else {
                format!("{message}\n[{} image(s) attached]", images.len())
            };
            output.user(shown.trim_start_matches('\n'))?;
        }
        if message.trim().is_empty() && images.is_empty() {
            continue;
        }
        if matches!(message, "/quit" | "/exit") {
            return Ok(());
        }
        let mut submitted = false;
        let result: Result<()> = (|| {
            match message {
                "/help" => ui.help(output)?,
                "/cli" | "/tui" => {
                    if !interactive || events.is_none() {
                        return Err(
                            local("Switching views requires an interactive terminal.").into()
                        );
                    }
                    if !images.is_empty() {
                        output.restore_draft(String::new(), images.clone());
                        return Err(local("Send or remove the staged images before switching views. They remain in the TUI composer.").into());
                    }
                    let mode = if message == "/cli" {
                        Mode::Cli
                    } else {
                        Mode::Tui
                    };
                    if output.is_full() == (mode == Mode::Tui) {
                        return Ok(());
                    }
                    if mode == Mode::Tui {
                        output.clear_session()?;
                    }
                    output.set_mode(mode)?;
                    if rpc.is_connected() {
                        show_session_status(
                            rpc,
                            input,
                            output,
                            dialogs,
                            raw,
                            events.as_ref().map(|e| &e.receiver),
                            ui,
                        )?;
                    }
                    output.inline_welcome()?;
                    if rpc.is_connected() {
                        show_history_for(
                            rpc,
                            output,
                            input,
                            dialogs,
                            raw,
                            events.as_ref().map(|e| &e.receiver),
                            interactive,
                            ui,
                        )?;
                    }
                }
                "/restore" => {
                    if !output.is_full() {
                        ui.info(
                            output,
                            "Draft restoration requires the full-screen composer.",
                        )?;
                    } else if let Some((text, images)) = last_failed.take() {
                        output.restore_draft(text, images);
                        ui.info(output, "Restored failed prompt. Review it before pressing Enter; tools may already have run.")?;
                    } else {
                        ui.info(output, "No failed prompt to restore.")?;
                    }
                }
                "/reconnect" => {
                    if rpc.is_connected() {
                        ui.info(output, "Runtime is already connected.")?;
                    } else {
                        if !rpc.has_saved_session() {
                            ui.info(output, "No saved session path was confirmed; reconnecting opens a new Runtime session. Existing sessions remain available via /sessions.")?;
                        }
                        rpc.recover_connection()?;
                        if let Err(error) = show_session_status(
                            rpc,
                            input,
                            output,
                            dialogs,
                            raw,
                            events.as_ref().map(|e| &e.receiver),
                            ui,
                        ) {
                            rpc.disconnect();
                            return Err(error);
                        }
                        output.set_disconnected(false)?;
                        ui.info(output, "Reconnected to runtime. No prompt was resent.")?;
                    }
                }
                _ if !rpc.is_connected() => {
                    if !message.starts_with('/') {
                        last_failed = Some((message.to_owned(), images.clone()));
                    }
                    ui.info(output, "Runtime is disconnected. Use /reconnect, /restore, /help, or /quit. Nothing was sent.")?;
                }
                _ if message.split_whitespace().next() == Some("/output") => {
                    if !images.is_empty() {
                        output.restore_draft(String::new(), images.clone());
                    }
                    use tool_output_command::Query;
                    let query = tool_output_command::parse(message)?;
                    let list = matches!(query, Query::List);
                    let command = match query {
                        Query::List => json!({"type":"get_tool_outputs"}),
                        Query::Latest => json!({"type":"get_tool_output"}),
                        Query::Index(index) => {
                            json!({"type":"get_tool_output","index":index.get()})
                        }
                    };
                    let data = rpc.request(
                        command,
                        input,
                        output,
                        dialogs,
                        raw,
                        events.as_ref().map(|e| &e.receiver),
                        interactive,
                    )?;
                    if list {
                        let entries: Vec<crate::engine::tool_output::Summary> =
                            serde_json::from_value(data["outputs"].clone())?;
                        output.saved_tool_outputs(&entries)?;
                    } else {
                        let entry: Option<crate::engine::tool_output::Output> =
                            serde_json::from_value(data["output"].clone())?;
                        output.saved_tool_output(entry.as_ref())?;
                    }
                }
                "/new" => {
                    rpc.command(
                        json!({"type": "new_session"}),
                        &[],
                        output,
                        input,
                        dialogs,
                        events.as_ref().map(|e| &e.receiver),
                        raw,
                        interactive,
                    )?;
                    // Clear only after Runtime confirms creation. A rejected or
                    // cancelled command must leave the current view intact.
                    last_failed = None;
                    if output.is_full() {
                        output.clear_session()?;
                    } else {
                        ui.info(output, "Started new session.")?;
                    }
                    show_session_status(
                        rpc,
                        input,
                        output,
                        dialogs,
                        raw,
                        events.as_ref().map(|e| &e.receiver),
                        ui,
                    )?;
                }
                "/continue" => {
                    let cwd = env::current_dir().map_err(local)?;
                    let list =
                        list_sessions_for_chat(cwd, output, events.as_ref().map(|e| &e.receiver))?;
                    if let Some(latest) = list.as_ref().and_then(|list| list.first()) {
                        open_session(
                            rpc,
                            &latest.path,
                            output,
                            input,
                            dialogs,
                            raw,
                            events.as_ref().map(|e| &e.receiver),
                            interactive,
                        )?;
                        show_session_status(
                            rpc,
                            input,
                            output,
                            dialogs,
                            raw,
                            events.as_ref().map(|e| &e.receiver),
                            ui,
                        )?;
                    } else if list.is_some() {
                        writeln!(output, "No saved sessions for this directory.")?;
                    }
                }
                _ if message == "/models" || message.starts_with("/models ") => {
                    choose_model(
                        rpc,
                        input,
                        output,
                        message.strip_prefix("/models"),
                        interactive,
                        events.as_ref().map(|e| &e.receiver),
                        raw,
                    )?;
                    show_session_status(
                        rpc,
                        input,
                        output,
                        dialogs,
                        raw,
                        events.as_ref().map(|e| &e.receiver),
                        ui,
                    )?;
                }
                _ if message == "/thinking" || message.starts_with("/thinking ") => {
                    choose_thinking(
                        rpc,
                        input,
                        output,
                        message
                            .strip_prefix("/thinking")
                            .map(str::trim)
                            .filter(|level| !level.is_empty()),
                        interactive,
                        events.as_ref().map(|e| &e.receiver),
                        raw,
                    )?;
                    show_session_status(
                        rpc,
                        input,
                        output,
                        dialogs,
                        raw,
                        events.as_ref().map(|e| &e.receiver),
                        ui,
                    )?;
                }
                _ if message == "/compact" || message.starts_with("/compact ") => {
                    let instructions = message
                        .strip_prefix("/compact")
                        .map(str::trim)
                        .filter(|text| !text.is_empty());
                    let result = rpc.compact(
                        instructions,
                        output,
                        input,
                        dialogs,
                        events.as_ref().map(|e| &e.receiver),
                        raw,
                        interactive,
                    )?;
                    let before = result["tokensBefore"].as_u64();
                    let after = result["estimatedTokensAfter"].as_u64();
                    if let (Some(before), Some(after)) = (before, after) {
                        ui.info(
                            output,
                            &format!(
                                "Runtime compacted context: {} → ~{} tokens. Original history is saved.",
                                compact_tokens(before),
                                compact_tokens(after)
                            ),
                        )?;
                    } else {
                        ui.info(
                            output,
                            "Runtime compacted context. Original history is saved.",
                        )?;
                    }
                    show_session_status(
                        rpc,
                        input,
                        output,
                        dialogs,
                        raw,
                        events.as_ref().map(|e| &e.receiver),
                        ui,
                    )?;
                }
                "/login" => {
                    let was_cli = !output.is_full() && output.is_terminal_chat();
                    if was_cli {
                        output.set_mode(Mode::Tui)?;
                    }
                    let result =
                        inline_login(rpc, input, output, interactive, dialogs, events, raw, ui);
                    if was_cli {
                        output.set_mode(Mode::Cli)?;
                        output.inline_welcome()?;
                    }
                    result?;
                }
                "/logout" => {
                    let was_cli = !output.is_full() && output.is_terminal_chat();
                    if was_cli {
                        output.set_mode(Mode::Tui)?;
                    }
                    let result =
                        inline_logout(rpc, input, output, interactive, dialogs, events, raw, ui);
                    if was_cli {
                        output.set_mode(Mode::Cli)?;
                        output.inline_welcome()?;
                    }
                    result?;
                }
                _ if extension_commands::namespaced(message)
                    || message
                        .split_whitespace()
                        .next()
                        .is_some_and(|name| output.has_extension_command(name)) =>
                {
                    if !images.is_empty() {
                        output.restore_draft(String::new(), images.clone());
                    }
                    extension_commands::dispatch(
                        message,
                        rpc,
                        input,
                        output,
                        dialogs,
                        raw,
                        events.as_ref().map(|e| &e.receiver),
                        interactive,
                    )?;
                    show_session_status(
                        rpc,
                        input,
                        output,
                        dialogs,
                        raw,
                        events.as_ref().map(|e| &e.receiver),
                        ui,
                    )?;
                }
                "/sessions" => {
                    let cwd = env::current_dir().map_err(local)?;
                    let list =
                        list_sessions_for_chat(cwd, output, events.as_ref().map(|e| &e.receiver))?;
                    let choice = if let Some(list) = list.as_ref() {
                        if output.is_full() {
                            if list.is_empty() {
                                writeln!(output, "No saved sessions for this directory.")?;
                                None
                            } else if let Some(keys) = events.as_ref() {
                                let labels = list
                                    .iter()
                                    .map(|entry| entry.title.clone())
                                    .collect::<Vec<_>>();
                                output
                                    .select("Saved sessions", &labels, None, &keys.receiver)?
                                    .map(|index| list[index].path.clone())
                            } else {
                                None
                            }
                        } else if let (Some(keys), Some(raw)) = (events.as_ref(), raw.as_mut()) {
                            sessions::pick_with(list, output, |_| {
                                raw.write("Choose a session number (Enter to cancel): ")?;
                                terminal::read_line(&keys.receiver, raw)
                            })?
                        } else {
                            sessions::pick(list, input, output, interactive)?
                        }
                    } else {
                        None
                    };
                    if let Some(path) = choice {
                        open_session(
                            rpc,
                            &path,
                            output,
                            input,
                            dialogs,
                            raw,
                            events.as_ref().map(|e| &e.receiver),
                            interactive,
                        )?;
                        show_session_status(
                            rpc,
                            input,
                            output,
                            dialogs,
                            raw,
                            events.as_ref().map(|e| &e.receiver),
                            ui,
                        )?;
                    }
                }
                _ if message
                    .split_whitespace()
                    .next()
                    .is_some_and(commands::looks_like_command) =>
                {
                    return Err(
                        local("Unknown command. Use /help to see available commands.").into(),
                    );
                }
                _ => {
                    submitted = true;
                    let command = json!({"type":"prompt", "message":message});
                    rpc.command(
                        command,
                        &images,
                        output,
                        input,
                        dialogs,
                        events.as_ref().map(|e| &e.receiver),
                        raw,
                        interactive,
                    )?;
                }
            }
            Ok(())
        })();
        let failed = result.is_err();
        match result {
            Ok(()) => {
                if submitted {
                    last_failed = None;
                }
            }
            Err(error) => {
                // A Rooxcode prompt refused for sign-in goes back in the input (#185).
                let sign_in = submitted
                    && output.is_full()
                    && error.to_string().contains(crate::engine::reka::SIGN_IN);
                if sign_in {
                    output.restore_draft(message.to_owned(), images.clone());
                }
                if submitted {
                    last_failed = Some((message.to_owned(), images));
                }
                handle_chat_error(error, rpc, output, ui)?;
                if submitted && output.is_full() && !sign_in {
                    ui.info(output, "Use /restore to review the failed prompt and attachments. Nothing is resent automatically.")?;
                }
            }
        }
        if let Some(rejected) = output.take_rejected_queue() {
            last_failed = Some(rejected);
        }
        // Capture the authoritative session path once Runtime has settled. A
        // reconnect never guesses another session from the workspace's latest.
        if (submitted || failed) && rpc.is_connected() && interactive {
            if let Err(error) = show_session_status(
                rpc,
                input,
                output,
                dialogs,
                raw,
                events.as_ref().map(|e| &e.receiver),
                ui,
            ) {
                handle_chat_error(error, rpc, output, ui)?;
            }
        }
        output.flush()?;
    }
}
