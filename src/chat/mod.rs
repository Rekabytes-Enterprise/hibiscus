mod accounts;
mod chat_loop;
pub(crate) mod commands;
mod extension_commands;
mod history;
mod login;
mod logout;
mod models;
pub(crate) mod saved_sessions;
mod tool_output_command;
use accounts::*;
use chat_loop::*;
use history::*;

use self::login::{sign_in, AuthOutcome};
use self::logout::{sign_out, LogoutOutcome};
use self::models::{choose_model, choose_thinking};
use self::saved_sessions as sessions;
use crate::{
    runtime::{
        bridge::{Client, SessionStart},
        error::{local, ChatError, RecoveryAction},
    },
    ui::{
        dialog::Dialogs,
        display::PromptAction,
        format::compact_tokens,
        messages::Ui,
        mode::Mode,
        terminal::{self, RawMode, Terminal, TerminalEvents},
        Frontend,
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
        Mode::Tui => Frontend::new(interactive_terminal),
        Mode::Cli => Frontend::with_mode(interactive_terminal, mode),
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

fn offer_update(output: &mut Frontend, keys: &Receiver<u8>, tag: &str) -> Result<()> {
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

fn handle_chat_error(
    error: Box<dyn std::error::Error + Send + Sync>,
    rpc: &mut Client,
    output: &mut Frontend,
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
