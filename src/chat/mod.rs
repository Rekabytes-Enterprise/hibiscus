pub(crate) mod commands;
mod models;
pub(crate) mod sessions;

use self::models::{choose_model, choose_thinking};
use crate::{
    pi::{
        auth::{codex_sign_in, AuthOutcome},
        configure_builtin_tools,
        dialog::Dialogs,
        error::{local, ChatError, RecoveryAction},
        logout::{sign_out, LogoutOutcome},
        rpc::{Rpc, SessionStart},
    },
    tui::{
        screen::{PromptAction, Screen, ScrollDisplay},
        terminal::{self, RawMode, Terminal, TerminalEvents},
        ui::Ui,
    },
    update, Result,
};
use serde_json::json;
use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};

pub(crate) fn start_chat(session: SessionStart) -> Result<()> {
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
    let mut screen = Screen::new(stdin.is_terminal() && tty.is_some())?;
    // Decide routing before the child starts: startup warnings must not race
    // alternate-screen setup and write directly over the composer.
    let mut rpc = Rpc::start_with_diagnostics(session, screen.diagnostic_feed())?;
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
        if !screen.is_full() {
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
            tty.as_ref(),
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
    rpc: &mut Rpc,
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
        if output.is_full() {
            output.status(&state)?;
        } else {
            ui.session(output, &state)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn show_history_for<R: BufRead>(
    rpc: &mut Rpc,
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
    rpc: &mut Rpc,
    input: &mut R,
    output: &mut Screen,
    interactive: bool,
    dialogs: &mut Dialogs,
    events: &mut Option<TerminalEvents>,
    raw: &mut Option<RawMode>,
    tty: Option<&Terminal>,
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
                terminal::read_line(&events.receiver, raw)?
            };
            let Some(text) = text else {
                return Ok(());
            };
            line.push_str(&text);
        } else if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let message = line.trim();
        let images = output.take_images();
        if output.is_full() && (!message.is_empty() || !images.is_empty()) {
            let shown = if images.is_empty() {
                message.to_owned()
            } else {
                format!("{message}\n[{} image(s) attached]", images.len())
            };
            output.user(shown.trim_start_matches('\n'))?;
        }
        if message.is_empty() && images.is_empty() {
            continue;
        }
        if matches!(message, "/quit" | "/exit") {
            return Ok(());
        }
        let mut submitted = false;
        let result: Result<()> = (|| {
            match message {
                "/help" => ui.help(output)?,
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
                        ui.info(output, "Pi is already connected.")?;
                    } else {
                        if !rpc.has_saved_session() {
                            ui.info(output, "No saved session path was confirmed; reconnecting opens a new Pi session. Existing sessions remain available via /sessions.")?;
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
                        ui.info(output, "Reconnected to Pi. No prompt was resent.")?;
                    }
                }
                _ if !rpc.is_connected() => {
                    if !message.starts_with('/') {
                        last_failed = Some((message.to_owned(), images.clone()));
                    }
                    ui.info(output, "Pi is disconnected. Use /reconnect, /restore, /help, or /quit. Nothing was sent.")?;
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
                    // Clear only after Pi confirms creation. A rejected or
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
                        ui.info(output, &format!("Pi compacted context: {before} → ~{after} tokens. Original history is saved."))?;
                    } else {
                        ui.info(output, "Pi compacted context. Original history is saved.")?;
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
                    codex_login(
                        rpc,
                        input,
                        output,
                        interactive,
                        dialogs,
                        events,
                        raw,
                        tty,
                        ui,
                    )?;
                }
                "/logout" => {
                    inline_logout(rpc, input, output, interactive, dialogs, events, raw, ui)?;
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
        // Capture the authoritative session path once Pi has settled. A
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

/// Only full-screen scans run off-thread. Keep Pi's files authoritative and
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
    rpc: &mut Rpc,
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
            "Pi disconnected. Use /reconnect; nothing is resent automatically.",
        )?;
    }
    output.flush()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn codex_login<R: BufRead>(
    rpc: &mut Rpc,
    input: &mut R,
    output: &mut Screen,
    interactive: bool,
    dialogs: &mut Dialogs,
    events: &mut Option<TerminalEvents>,
    raw: &mut Option<RawMode>,
    tty: Option<&Terminal>,
    ui: &Ui,
) -> Result<()> {
    if !interactive || !output.is_full() || events.is_none() {
        return auth_handoff("/login", rpc, input, output, interactive, events, raw, tty);
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
    // logged-out Pi can select another model or have no model at all.
    let choices = vec![
        "OpenAI Codex · browser or device code".to_owned(),
        "Other provider · open Pi".to_owned(),
    ];
    let keys = &events
        .as_ref()
        .ok_or("terminal events unavailable")?
        .receiver;
    match output.select("Sign in to", &choices, None, keys)? {
        Some(0) => {}
        Some(1) => {
            return auth_handoff("/login", rpc, input, output, interactive, events, raw, tty)
        }
        _ => return Ok(()),
    }
    let keys = &events
        .as_ref()
        .ok_or("terminal events unavailable")?
        .receiver;
    match codex_sign_in(output, keys)? {
        AuthOutcome::Success => {
            let start = state["sessionFile"]
                .as_str()
                .map(PathBuf::from)
                .filter(|path| path.is_file())
                .map(SessionStart::Selected)
                .unwrap_or(SessionStart::New);
            rpc.reconnect(start)?;
            ui.info(
                output,
                "Codex sign-in completed. Reconnected Pi to this chat.",
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
        AuthOutcome::Cancelled => ui.info(output, "Codex sign-in cancelled.")?,
        AuthOutcome::Failed => ui.info(
            output,
            "Codex sign-in failed. Try again or use Pi /login for details.",
        )?,
        AuthOutcome::Unavailable => {
            ui.info(
                output,
                "Codex SDK or Node is unavailable; Pi's TUI can handle login instead.",
            )?;
            return auth_handoff("/login", rpc, input, output, interactive, events, raw, tty);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn inline_logout<R: BufRead>(
    rpc: &mut Rpc,
    input: &mut R,
    output: &mut Screen,
    interactive: bool,
    dialogs: &mut Dialogs,
    events: &Option<TerminalEvents>,
    raw: &mut Option<RawMode>,
    ui: &Ui,
) -> Result<()> {
    let Some(keys) = events.as_ref().filter(|_| interactive && output.is_full()) else {
        ui.info(output, "Inline /logout requires a full-screen terminal. Run pi and use /logout to manage stored credentials.")?;
        return Ok(());
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
    if let Ok(LogoutOutcome::Removed {
        provider,
        synchronization_warning,
    }) = &result
    {
        ui.info(
            output,
            &format!("Removed stored credentials for {provider}."),
        )?;
        if *synchronization_warning {
            ui.info(output, "Pi removed the credential but could not synchronize its helper state; reopening the chat backend.")?;
        }
    }
    // Once removal was authorized, discard cached credentials even when the
    // helper fails or times out after a possible storage mutation.
    if !rpc.is_connected() {
        rpc.reopen(start)?;
        show_session_status(rpc, input, output, dialogs, raw, Some(&keys.receiver), ui)?;
    }
    match result? {
        LogoutOutcome::Removed { .. } => {
            ui.info(output, "Logout complete. Chat is preserved. Environment variables and models.json are unchanged; provider tokens are not revoked.")?;
        }
        LogoutOutcome::Cancelled => ui.info(output, "Logout cancelled.")?,
        LogoutOutcome::Empty => ui.info(output, "No stored credentials to remove. Environment variables and models.json may still provide access.")?,
        LogoutOutcome::Unavailable => ui.info(output, "Inline logout needs Node and a compatible Pi SDK. No credentials were changed. Run pi and use /logout, or update Pi.")?,
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn auth_handoff<R: BufRead>(
    action: &str,
    rpc: &mut Rpc,
    input: &mut R,
    output: &mut Screen,
    interactive: bool,
    events: &mut Option<TerminalEvents>,
    raw: &mut Option<RawMode>,
    tty: Option<&Terminal>,
) -> Result<()> {
    if !interactive || tty.is_none() {
        writeln!(
            output,
            "{action} requires an interactive terminal. Run `pi` to manage credentials."
        )?;
        return Ok(());
    }
    let (Some(keys), Some(editor)) = (events.as_ref(), raw.as_mut()) else {
        return Ok(());
    };
    let confirmed = if output.is_full() {
        output.select(
            &format!("Hand off to Pi for {action}?"),
            &["Cancel".into(), "Open Pi".into()],
            None,
            &keys.receiver,
        )? == Some(1)
    } else {
        output.invalidate_screen();
        editor.write(&format!("Hand off to Pi for {action}? [y/N] "))?;
        terminal::read_line(&keys.receiver, editor)?
            .is_some_and(|s| s.eq_ignore_ascii_case("y") || s.eq_ignore_ascii_case("yes"))
    };
    if !confirmed {
        writeln!(output, "Cancelled.")?;
        return Ok(());
    }
    let mut dialogs = Dialogs;
    let state = rpc.request(
        json!({"type":"get_state"}),
        input,
        output,
        &mut dialogs,
        raw,
        events.as_ref().map(|e| &e.receiver),
        interactive,
    )?;
    let session = state["sessionFile"]
        .as_str()
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    let start = session.as_ref().map_or(SessionStart::New, |path| {
        SessionStart::Selected(path.clone())
    });
    // Release Pi RPC before Pi TUI touches the same session file, then give
    // Pi the terminal with no Hibiscus reader left to steal auth keystrokes.
    rpc.close()?;
    if let Err(error) = output.suspend() {
        let _ = rpc.reopen(start);
        return Err(error.into());
    }
    drop(events.take());
    drop(raw.take());
    let pi = env::var("HIBISCUS_PI").unwrap_or_else(|_| "pi".to_owned());
    writeln!(
        output,
        "In Pi, type {action} then /quit to return to Hibiscus."
    )?;
    let mut command = Command::new(&pi);
    configure_builtin_tools(&mut command);
    if let Some(path) = &session {
        command.arg("--session").arg(path);
    } else {
        command.arg("--no-session");
    }
    let result = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    // Restore terminal ownership and reconnect even if Pi failed or was cancelled.
    *raw = tty.map(Terminal::raw).transpose()?;
    *events = tty.map(Terminal::events).transpose()?;
    let screen_result = output.resume();
    let rpc_result = rpc.reopen(start);
    screen_result?;
    rpc_result?;
    match result {
        Ok(status) if status.success() => writeln!(
            output,
            "Returned from Pi. Authentication changes may require restarting Hibiscus."
        )?,
        Ok(status) => writeln!(output, "Pi exited with {status}; chat is unchanged.")?,
        Err(error) => writeln!(output, "Pi handoff failed: {error}")?,
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn open_session<R: BufRead>(
    rpc: &mut Rpc,
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
