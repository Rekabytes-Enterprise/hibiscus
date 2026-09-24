pub(crate) mod commands;
mod models;
pub(crate) mod sessions;

use self::models::choose_model;
use crate::{
    pi::{
        auth::{codex_sign_in, AuthOutcome},
        dialog::Dialogs,
        rpc::{Rpc, SessionStart},
    },
    tui::{
        screen::Screen,
        terminal::{self, RawMode, Terminal, TerminalEvents},
        ui::Ui,
    },
    Result,
};
use serde_json::json;
use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::Receiver;

pub(crate) fn start_chat(session: SessionStart) -> Result<()> {
    let show_history = !matches!(session, SessionStart::New);
    let mut rpc = Rpc::start(session)?;
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
    let outcome = (|| {
        let mut dialogs = Dialogs;
        if !screen.is_full() {
            ui.header(&mut screen)?;
        }
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
fn show_history_for<R: BufRead, W: Write>(
    rpc: &mut Rpc,
    output: &mut W,
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
    sessions::show_recent_with(messages, output, ui)
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
) -> Result<()> {
    let mut line = String::new();
    loop {
        if !output.is_full() {
            ui.prompt(output)?;
        }
        line.clear();
        if let (Some(events), Some(raw)) = (events.as_ref(), raw.as_mut()) {
            let Some(text) = (if output.is_full() {
                output.prompt(&events.receiver)?
            } else {
                terminal::read_line(&events.receiver, raw)?
            }) else {
                return Ok(());
            };
            line.push_str(&text);
        } else if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let message = line.trim();
        if output.is_full() && !message.is_empty() {
            output.user(message)?;
        }
        match message {
            "" => continue,
            "/quit" | "/exit" => return Ok(()),
            "/help" => ui.help(output)?,
            "/new" => {
                rpc.command(
                    json!({"type": "new_session"}),
                    output,
                    input,
                    dialogs,
                    events.as_ref().map(|e| &e.receiver),
                    raw,
                    interactive,
                )?;
                ui.info(output, "Started new session.")?;
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
                let cwd = env::current_dir()?;
                let list = sessions::list(&cwd)?;
                if let Some(latest) = list.first() {
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
                } else {
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
                auth_handoff(message, rpc, input, output, interactive, events, raw, tty)?;
            }
            "/sessions" => {
                let cwd = env::current_dir()?;
                let list = sessions::list(&cwd)?;
                let choice = if output.is_full() {
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
                    sessions::pick_with(&list, output, |_| {
                        raw.write("Choose a session number (Enter to cancel): ")?;
                        terminal::read_line(&keys.receiver, raw)
                    })?
                } else {
                    sessions::pick(&list, input, output, interactive)?
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
            _ => rpc.prompt(
                message,
                output,
                input,
                dialogs,
                events.as_ref().map(|e| &e.receiver),
                raw,
                interactive,
            )?,
        }
    }
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
    editor.write(&format!("Hand off to Pi for {action}? [y/N] "))?;
    let confirmed = terminal::read_line(&keys.receiver, editor)?
        .is_some_and(|s| s.eq_ignore_ascii_case("y") || s.eq_ignore_ascii_case("yes"));
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
fn open_session<R: BufRead, W: Write>(
    rpc: &mut Rpc,
    path: &std::path::Path,
    output: &mut W,
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
