use crate::{tui::screen::Screen, Result};
use std::{collections::HashMap, fs, os::unix::fs::PermissionsExt, sync::mpsc::Receiver};
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AuthOutcome {
    Success,
    Cancelled,
}
pub(super) fn keys() -> Result<HashMap<String, String>> {
    let path = crate::paths::home()?.join("auth.json");
    match fs::symlink_metadata(&path) {
        Ok(meta) => {
            if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 {
                return Err("auth.json must be a regular private file, mode 0600".into());
            }
            Ok(serde_json::from_slice(&fs::read(path)?)?)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(e) => Err(e.into()),
    }
}
pub(super) fn save(keys: &HashMap<String, String>) -> Result<()> {
    super::config::atomic(
        &crate::paths::home()?.join("auth.json"),
        &serde_json::to_vec(keys)?,
    )
}
pub(crate) fn sign_in(
    output: &mut Screen,
    events: &Receiver<u8>,
    provider: &str,
    auth_type: &str,
) -> Result<AuthOutcome> {
    if auth_type == "oauth" && provider == "openai-codex" {
        return super::oauth::sign_in(output, events);
    }
    if auth_type != "api_key" {
        return Err("This provider requires a supported native authentication flow. Use a configured API-key provider.".into());
    }
    let config = super::config::load()?;
    if !config
        .providers
        .iter()
        .any(|p| p.id == provider && p.authentication == super::config::Authentication::ApiKey)
    {
        return Err("Unknown provider".into());
    }
    let Some(answer) = output.auth_input(events, true, || Ok(false))? else {
        return Ok(AuthOutcome::Cancelled);
    };
    if answer.trim().is_empty() || answer.len() > 8192 || answer.chars().any(char::is_control) {
        return Err("API key must be nonempty and contain no control characters".into());
    }
    let mut keys = keys()?;
    keys.insert(provider.into(), answer);
    save(&keys)?;
    Ok(AuthOutcome::Success)
}
