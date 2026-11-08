//! Library API for the desktop app. The CLI and TUI do not use it.
//! See `docs/desktop/protocol.md` for the commands and events.
pub use crate::runtime::embed::{Engine, Start};
pub use crate::runtime::CodexSignIn;
use crate::Result;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Stores an API key for a configured API-key provider in `auth.json`.
/// Start a new engine afterwards, as the TUI reconnects after `/login`.
pub fn save_api_key(provider: &str, key: &str) -> Result<()> {
    crate::runtime::auth::save_api_key(provider, key.to_owned())
}

/// Removes the stored credential for `provider`: its API key, or the Codex login.
pub fn sign_out(provider: &str) -> Result<()> {
    crate::runtime::logout::remove(provider)
}

/// Where to create an API key for a built-in provider, e.g. `openrouter.ai/settings/keys`.
pub fn key_page(provider: &str) -> Option<&'static str> {
    crate::runtime::catalog::key_page(provider)
}

/// A saved session for a workspace.
pub struct SavedSession {
    pub path: PathBuf,
    pub title: String,
    pub modified: SystemTime,
}

/// Saved sessions for `workspace`, newest first.
pub fn sessions(workspace: &Path) -> Result<Vec<SavedSession>> {
    Ok(crate::chat::sessions::list(&workspace.canonicalize()?)?
        .into_iter()
        .map(|s| SavedSession {
            path: s.path,
            title: s.title,
            modified: s.modified,
        })
        .collect())
}

/// A configured provider, for the login screen.
pub struct ProviderInfo {
    pub id: String,
    /// Signs in with an API key (other providers use their own flow).
    pub api_key: bool,
    /// Signs in through the browser with `CodexSignIn`.
    pub browser: bool,
    pub signed_in: bool,
    pub key_page: Option<&'static str>,
}

/// Every configured provider, including signed-out ones.
pub fn providers() -> Result<Vec<ProviderInfo>> {
    use crate::runtime::config;
    Ok(config::load()?
        .providers
        .iter()
        .map(|p| ProviderInfo {
            id: p.id.clone(),
            api_key: p.authentication == config::Authentication::ApiKey,
            browser: p.authentication == config::Authentication::Codex,
            signed_in: config::access(p).is_ok(),
            key_page: key_page(&p.id),
        })
        .collect())
}
