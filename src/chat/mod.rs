pub(crate) mod commands;
mod models;
pub(crate) mod sessions;
mod tool_outputs;

use self::models::{choose_model, choose_thinking};
use crate::{
    runtime::{
        auth::{sign_in, AuthOutcome},
        bridge::{Client, SessionStart},
        dialog::Dialogs,
        error::{local, ChatError, RecoveryAction},
        logout::{sign_out, LogoutOutcome},
    },
    tui::{
        mode::Mode,
        screen::{compact_tokens, PromptAction, Screen},
        terminal::{self, RawMode, Terminal, TerminalEvents},
        ui::Ui,
    },
    update, Result,
};
use serde_json::json;
use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

pub(crate) fn start_chat(session: SessionStart) -> Result<()> {
    start_chat_with_mode(session, Mode::Tui)
}

pub(crate) fn start_chat_with_mode(session: SessionStart, mode: Mode) -> Result<()> {
    let show_history = !matches!(session, SessionStart::New);
    let stdin = io::stdin();
    let ui = Ui::new(stdin.is_terminal());
    let tty = if stdin.is_terminal() {
        Terminal::open()
    } else {
        None
    };
    // Enter raw mode before starting the reader: Esc should arrive without
    // Enter even when a response is already in progress.
    let mut raw = tty.as_ref().map(Terminal::raw).transpose()?;
    let mut events = tty.as_ref().map(Terminal::events).transpose()?;
    let interactive_terminal = stdin.is_terminal() && tty.is_some();
    let mut screen = match mode {
        Mode::Tui => Screen::new(interactive_terminal),
        Mode::Cli => Screen::with_mode(interactive_terminal, mode),
    }?;
    // Decide routing before the child starts: startup warnings must not race
    // alternate-screen setup and write directly over the composer.
    let mut rpc = Client::start_with_diagnostics(session, screen.diagnostic_feed())?;
    let update_notice = if screen.is_full()
        && env::var_os("HIBISCUS_NO_UPDATE_CHECK").is_none()
        && update::installed_prebuilt()
    {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(update::check_startup());
        });
        Some(receiver)
    } else {
        None
    };
    let outcome = (|| {
        let mut dialogs = Dialogs;
        if !screen.is_full() && !screen.is_terminal_chat() {
            ui.header(&mut screen)?;
        }
        let setup: Result<()> = (|| {
            if ui.interactive {
                show_session_status(
                    &mut rpc,
                    &mut stdin.lock(),
                    &mut screen,
                    &mut dialogs,
                    &mut raw,
                    events.as_ref().map(|e| &e.receiver),
                    &ui,
                )?;
            }
            screen.inline_welcome()?;
            if show_history {
                show_history_for(
                    &mut rpc,
                    &mut screen,
                    &mut stdin.lock(),
                    &mut dialogs,
                    &mut raw,
                    events.as_ref().map(|e| &e.receiver),
                    stdin.is_terminal(),
                    &ui,
                )?;
            }
            Ok(())
        })();
        if let Err(error) = setup {
            handle_chat_error(error, &mut rpc, &mut screen, &ui)?;
        }
        chat(
            &mut rpc,
            &mut stdin.lock(),
            &mut screen,
            stdin.is_terminal(),
            &mut dialogs,
            &mut events,
            &mut raw,
            &ui,
            update_notice.as_ref(),
        )
    })();
    // Turn off mouse reporting before restoring canonical mode; otherwise a
    // late wheel event could become shell input after Hibiscus exits.
    let restored = screen.suspend();
    drop(events);
    drop(raw);
    drop(screen);
    rpc.finish(outcome.and_then(|_| restored.map_err(Into::into)))
}

#[allow(clippy::too_many_arguments)]
fn show_session_status<R: BufRead>(
    rpc: &mut Client,
    input: &mut R,
    output: &mut Screen,
    dialogs: &mut Dialogs,
    raw: &mut Option<RawMode>,
    events: Option<&Receiver<u8>>,
    ui: &Ui,
) -> Result<()> {
    if ui.interactive {
        let state = rpc.request(
            json!({"type":"get_state"}),
            input,
            output,
            dialogs,
            raw,
            events,
            true,
        )?;
        output.status(&state)?;
        if output.is_full() {
            let stats = rpc.session_stats(input, output, dialogs, raw, events, true)?;
            output.session_usage(stats.as_ref().unwrap_or(&serde_json::Value::Null))?;
        } else if !output.is_terminal_chat() {
            ui.session(output, &state)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn show_history_for<R: BufRead>(
    rpc: &mut Client,
    output: &mut Screen,
    fallback: &mut R,
    dialogs: &mut Dialogs,
    raw: &mut Option<RawMode>,
    events: Option<&Receiver<u8>>,
    interactive: bool,
    ui: &Ui,
) -> Result<()> {
    let messages = rpc.request(
        json!({"type": "get_messages"}),
        fallback,
        output,
        dialogs,
        raw,
        events,
        interactive,
    )?;
    let messages = messages["messages"]
        .as_array()
        .ok_or("invalid get_messages response")?;
    output.restore_goal(messages)?;
    sessions::show_recent_with(messages, output, ui)
}

fn offer_update(output: &mut Screen, keys: &Receiver<u8>, tag: &str) -> Result<()> {
    if output.select(
        &format!("Hibiscus {tag} is available"),
        &["Later".into(), "Update now".into()],
        None,
        keys,
    )? == Some(1)
    {
        output.suspend()?;
        let result = update::install(tag);
        output.resume()?;
        match result {
            Ok(()) => writeln!(output, "Updated to {tag}. Restart Hibiscus to use it.")?,
            Err(error) => writeln!(output, "Update failed: {error}")?,
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn chat<R: BufRead>(
    rpc: &mut Client,
    input: &mut R,
    output: &mut Screen,
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
                match output.inline_input_style() {
                    Some(color) => crate::tui::line_editor::read(&events.receiver, raw, color)?,
                    None => terminal::read_line(&events.receiver, raw)?,
                }
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
                    use tool_outputs::Query;
                    let query = tool_outputs::parse(message)?;
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
                        let entries: Vec<crate::runtime::tool_output::Summary> =
                            serde_json::from_value(data["outputs"].clone())?;
                        output.saved_tool_outputs(&entries)?;
                    } else {
                        let entry: Option<crate::runtime::tool_output::Output> =
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
                "/models" => {
                    choose_model(
                        rpc,
                        input,
                        output,
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
                if submitted {
                    last_failed = Some((message.to_owned(), images));
                }
                handle_chat_error(error, rpc, output, ui)?;
                if submitted && output.is_full() {
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

/// Only full-screen scans run off-thread. Keep Hibiscus's files authoritative and
/// preserve typing that arrives while a slow first-time scan is running.
fn list_sessions_for_chat(
    cwd: PathBuf,
    output: &mut Screen,
    keys: Option<&Receiver<u8>>,
) -> Result<Option<Vec<sessions::SessionInfo>>> {
    let Some(keys) = keys.filter(|_| output.is_full()) else {
        return Ok(Some(sessions::list(&cwd).map_err(local)?));
    };
    let old_notice = output.start_session_scan()?;
    let scan = sessions::Scan::start(cwd);
    let mut buffered = Vec::new();
    let result: Result<Option<Vec<sessions::SessionInfo>>> =
        (|| -> Result<Option<Vec<sessions::SessionInfo>>> {
            loop {
                // Keys take precedence even if a scan finished at the same instant.
                for _ in 0..128 {
                    match keys.try_recv() {
                        Ok(27) => match crate::tui::screen::escape_key(keys) {
                            crate::tui::screen::Navigation::Escape => return Ok(None),
                            crate::tui::screen::Navigation::EscapeWith(byte) => {
                                buffered.push(byte);
                                return Ok(None);
                            }
                            _ => {}
                        },
                        Ok(3 | 4) => return Ok(None),
                        Ok(b'\r' | b'\n') => {} // Never replay Enter as an unintended prompt.
                        Ok(byte) => {
                            if buffered.len() >= 128 * 1024 {
                                return Ok(None);
                            }
                            buffered.push(byte);
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(None),
                    }
                }
                match scan.recv_timeout(std::time::Duration::from_millis(40)) {
                    Ok(result) => return Ok(Some(result.map_err(local)?)),
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(local("session scan stopped unexpectedly").into())
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => output.render()?,
                }
            }
        })();
    drop(scan); // request cancellation without blocking the terminal on Esc
    output.defer_session_typing(buffered);
    output.end_session_scan(old_notice)?;
    result
}

fn handle_chat_error(
    error: Box<dyn std::error::Error + Send + Sync>,
    rpc: &mut Client,
    output: &mut Screen,
    ui: &Ui,
) -> Result<()> {
    // Local terminal/stdin I/O failure cannot safely be turned into chat text.
    if !ui.interactive || error.is::<io::Error>() {
        return Err(error);
    }
    let error = error
        .downcast_ref::<ChatError>()
        .cloned()
        .unwrap_or_else(|| local(error));
    if error.recovery() == RecoveryAction::Reconnect || !rpc.is_connected() {
        rpc.disconnect();
        output.set_disconnected(true)?;
    }
    writeln!(output)?;
    ui.info(
        output,
        &format!("Error ({:?}): {}", error.category, error.message),
    )?;
    if rpc.is_connected() {
        ui.info(output, error.hint())?;
    } else {
        ui.info(
            output,
            "Runtime disconnected. Use /reconnect; nothing is resent automatically.",
        )?;
    }
    output.flush()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn inline_login<R: BufRead>(
    rpc: &mut Client,
    input: &mut R,
    output: &mut Screen,
    interactive: bool,
    dialogs: &mut Dialogs,
    events: &mut Option<TerminalEvents>,
    raw: &mut Option<RawMode>,
    ui: &Ui,
) -> Result<()> {
    if !interactive || !output.is_full() || events.is_none() {
        return Err(local("Authentication requires a full-screen terminal. Start Hibiscus in a supported interactive terminal, then retry /login.").into());
    }
    let state = rpc.request(
        json!({"type":"get_state"}),
        input,
        output,
        dialogs,
        raw,
        events.as_ref().map(|keys| &keys.receiver),
        interactive,
    )?;
    // Login is a provider choice, not a property of the current model. A
    // logged-out Runtime can select another model or have no model at all.
    let providers = crate::runtime::config::load()?.providers;
    let keys = &events
        .as_ref()
        .ok_or("terminal events unavailable")?
        .receiver;
    let methods = vec!["Browser Login".into(), "API Key".into()];
    let Some(method) = output.select("Sign in to Hibiscus", &methods, None, keys)? else {
        return Ok(());
    };
    let choices = if method == 0 {
        vec!["Codex · ChatGPT account".into()]
    } else {
        providers
            .iter()
            .filter(|p| p.authentication == crate::runtime::config::Authentication::ApiKey)
            .map(|p| p.id.clone())
            .collect::<Vec<_>>()
    };
    let title = if method == 0 {
        "Browser Login"
    } else {
        "API Key · provider"
    };
    let Some(index) = output.select(title, &choices, None, keys)? else {
        return Ok(());
    };
    let provider = if method == 0 {
        "openai-codex"
    } else {
        choices[index].as_str()
    };
    let auth_type = if method == 0 { "oauth" } else { "api_key" };
    let label = provider;
    let keys = &events
        .as_ref()
        .ok_or("terminal events unavailable")?
        .receiver;
    match sign_in(output, keys, provider, auth_type)? {
        AuthOutcome::Success => {
            let start = state["sessionFile"]
                .as_str()
                .map(PathBuf::from)
                .filter(|path| path.is_file())
                .map(SessionStart::Selected)
                .unwrap_or(SessionStart::New);
            rpc.reconnect(start)?;
            if let Some(model) = crate::runtime::config::load()?
                .providers
                .iter()
                .find(|p| p.id == provider)
                .and_then(|p| p.models.first())
            {
                rpc.request(
                    json!({"type":"set_model","provider":provider,"modelId":model.id}),
                    input,
                    output,
                    dialogs,
                    raw,
                    events.as_ref().map(|keys| &keys.receiver),
                    interactive,
                )?;
            }
            ui.info(
                output,
                &format!("{label} sign-in completed. Reconnected Runtime to this chat."),
            )?;
            show_session_status(
                rpc,
                input,
                output,
                dialogs,
                raw,
                events.as_ref().map(|keys| &keys.receiver),
                ui,
            )?;
        }
        AuthOutcome::Cancelled => ui.info(output, &format!("{label} sign-in cancelled."))?,
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn inline_logout<R: BufRead>(
    rpc: &mut Client,
    input: &mut R,
    output: &mut Screen,
    interactive: bool,
    dialogs: &mut Dialogs,
    events: &Option<TerminalEvents>,
    raw: &mut Option<RawMode>,
    ui: &Ui,
) -> Result<()> {
    let Some(keys) = events.as_ref().filter(|_| interactive && output.is_full()) else {
        return Err(local("Authentication requires a full-screen terminal. Start Hibiscus in a supported interactive terminal, then retry /logout.").into());
    };
    let state = rpc.request(
        json!({"type":"get_state"}),
        input,
        output,
        dialogs,
        raw,
        Some(&keys.receiver),
        interactive,
    )?;
    let start = state["sessionFile"]
        .as_str()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .map(SessionStart::Selected)
        .unwrap_or(SessionStart::New);
    let result = sign_out(output, &keys.receiver, || rpc.close());
    if let Ok(LogoutOutcome::Removed { provider }) = &result {
        ui.info(
            output,
            &format!("Removed stored credentials for {provider}."),
        )?;
    }
    // Once removal was authorized, discard cached credentials even when the
    // helper fails or times out after a possible storage mutation.
    if !rpc.is_connected() {
        rpc.reopen(start)?;
        show_session_status(rpc, input, output, dialogs, raw, Some(&keys.receiver), ui)?;
    }
    match result? {
        LogoutOutcome::Removed { .. } => {
            ui.info(output, "Logout complete. Chat is preserved. Environment variables and config.json are unchanged; provider tokens are not revoked.")?;
        }
        LogoutOutcome::Cancelled => ui.info(output, "Logout cancelled.")?,
        LogoutOutcome::Empty => {
            show_session_status(rpc, input, output, dialogs, raw, Some(&keys.receiver), ui)?;
            ui.info(
                output,
                "No stored credentials. Environment variables may still provide API access.",
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn open_session<R: BufRead>(
    rpc: &mut Client,
    path: &std::path::Path,
    output: &mut Screen,
    input: &mut R,
    dialogs: &mut Dialogs,
    raw: &mut Option<RawMode>,
    events: Option<&Receiver<u8>>,
    interactive: bool,
) -> Result<()> {
    let current = rpc.request(
        json!({"type": "get_state"}),
        input,
        output,
        dialogs,
        raw,
        events,
        interactive,
    )?;
    if current["sessionFile"].as_str() == Some(path.to_string_lossy().as_ref()) {
        return show_history_for(
            rpc,
            output,
            input,
            dialogs,
            raw,
            events,
            interactive,
            &Ui::new(interactive),
        );
    }
    let data = rpc.request(
        json!({"type": "switch_session", "sessionPath": path}),
        input,
        output,
        dialogs,
        raw,
        events,
        interactive,
    )?;
    if data["cancelled"] == true {
        writeln!(output, "Session switch cancelled.")?;
        return Ok(());
    }
    show_history_for(
        rpc,
        output,
        input,
        dialogs,
        raw,
        events,
        interactive,
        &Ui::new(interactive),
    )
}
