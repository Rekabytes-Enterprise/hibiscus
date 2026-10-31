use crate::{ui::Frontend, Result};
use std::sync::mpsc::Receiver;
pub(crate) enum LogoutOutcome {
    Removed { provider: String },
    Cancelled,
    Empty,
}
pub(crate) fn sign_out(
    output: &mut Frontend,
    events: &Receiver<u8>,
    before_remove: impl FnOnce() -> Result<()>,
) -> Result<LogoutOutcome> {
    let mut keys = super::auth::keys()?;
    let mut providers = keys.keys().cloned().collect::<Vec<_>>();
    providers.sort();
    if super::oauth::stored_access_token().is_ok() {
        providers.push("openai-codex".into());
    }
    if providers.is_empty() {
        return Ok(LogoutOutcome::Empty);
    }
    let Some(index) = output.select("Stored credentials", &providers, None, events)? else {
        return Ok(LogoutOutcome::Cancelled);
    };
    let provider = providers[index].clone();
    let choices = vec!["Cancel".into(), "Remove stored credential".into()];
    if output.select(&format!("Log out of {provider}?"), &choices, None, events)? != Some(1) {
        return Ok(LogoutOutcome::Cancelled);
    }
    before_remove()?;
    if provider == "openai-codex" {
        super::oauth::remove()?;
    } else {
        keys.remove(&provider);
        super::auth::save(&keys)?;
    }
    Ok(LogoutOutcome::Removed { provider })
}
