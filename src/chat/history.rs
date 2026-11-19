//! Session status, transcript history and choosing a saved session to open.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn show_session_status<R: BufRead>(
    rpc: &mut Client,
    input: &mut R,
    output: &mut Frontend,
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
pub(super) fn show_history_for<R: BufRead>(
    rpc: &mut Client,
    output: &mut Frontend,
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

/// Only full-screen scans run off-thread. Keep Hibiscus's files authoritative and
/// preserve typing that arrives while a slow first-time scan is running.
pub(super) fn list_sessions_for_chat(
    cwd: PathBuf,
    output: &mut Frontend,
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
                        Ok(27) => match crate::ui::input::escape_key(keys) {
                            crate::ui::input::Navigation::Escape => return Ok(None),
                            crate::ui::input::Navigation::EscapeWith(byte) => {
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

#[allow(clippy::too_many_arguments)]
pub(super) fn open_session<R: BufRead>(
    rpc: &mut Client,
    path: &std::path::Path,
    output: &mut Frontend,
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
