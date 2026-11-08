//! The /logout prompts. Credential removal stays in runtime/logout.rs.
use crate::{engine::logout, ui::Frontend, Result};
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
    let providers = logout::stored()?;
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
    logout::remove(&provider)?;
    Ok(LogoutOutcome::Removed { provider })
}
