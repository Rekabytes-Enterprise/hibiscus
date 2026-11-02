use crate::{ui::Frontend, Result};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::mpsc::Receiver,
};

const MAX_AUTH_BYTES: usize = 128 * 1024;
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AuthOutcome {
    Success,
    Cancelled,
}
pub(super) fn keys() -> Result<HashMap<String, String>> {
    keys_at(&crate::paths::home()?.join("auth.json"))
}
fn keys_at(path: &Path) -> Result<HashMap<String, String>> {
    match open_key_file(path)? {
        Some(file) => read_keys(file),
        None => Ok(HashMap::new()),
    }
}
fn open_key_file(path: &Path) -> Result<Option<fs::File>> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(
                "Cannot read native API-key credentials. Check auth.json permissions.".into(),
            )
        }
    };
    // Validate the opened object, then retain that same descriptor for reading.
    let meta = file
        .metadata()
        .map_err(|_| "Cannot inspect native API-key credentials.")?;
    if !meta.is_file()
        || meta.permissions().mode() & 0o077 != 0
        || meta.len() > MAX_AUTH_BYTES as u64
    {
        return Err("auth.json must be a regular private file, mode 0600, at most 128 KiB".into());
    }
    Ok(Some(file))
}
fn read_keys(input: impl Read) -> Result<HashMap<String, String>> {
    let mut bytes = Vec::new();
    input
        .take(MAX_AUTH_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read native API-key credentials.")?;
    if bytes.len() > MAX_AUTH_BYTES {
        return Err("auth.json exceeds the 128 KiB limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        "Invalid auth.json. Restore a valid private credential file before retrying /login.".into()
    })
}

pub(super) fn save(keys: &HashMap<String, String>) -> Result<()> {
    save_at(&crate::paths::home()?.join("auth.json"), keys)
}
fn save_at(path: &Path, keys: &HashMap<String, String>) -> Result<()> {
    let bytes = serde_json::to_vec(keys)?;
    if bytes.len() > MAX_AUTH_BYTES {
        return Err("Stored API keys would exceed the 128 KiB auth.json limit".into());
    }
    super::config::atomic(path, &bytes)
}
pub(crate) fn sign_in(
    output: &mut Frontend,
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

#[cfg(test)]
mod tests;
