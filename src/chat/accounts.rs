//! /login and /logout from inside a chat, including the CLI handover to full-screen dialogs.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn inline_login<R: BufRead>(
    rpc: &mut Client,
    input: &mut R,
    output: &mut Frontend,
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
    let providers = crate::engine::config::load()?.providers;
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
            .filter(|p| p.authentication == crate::engine::config::Authentication::ApiKey)
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
            let configured = crate::engine::config::load()?;
            let configured = configured.providers.iter().find(|p| p.id == provider);
            if configured.is_some_and(|p| p.models.is_empty()) {
                // Catalog providers load their models at runtime; let the user pick one.
                choose_model(
                    rpc,
                    input,
                    output,
                    Some(provider),
                    interactive,
                    events.as_ref().map(|keys| &keys.receiver),
                    raw,
                )?;
            } else if let Some(model) = configured.and_then(|p| p.models.first()) {
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
pub(super) fn inline_logout<R: BufRead>(
    rpc: &mut Client,
    input: &mut R,
    output: &mut Frontend,
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
