//! Library API for the desktop app. The CLI and TUI do not use it.
//! See `docs/desktop/protocol.md` for the commands and events.
pub use crate::client::embed::{Engine, Start};
/// The error a Rooxcode prompt gets without a Reka Bytes session.
pub use crate::engine::reka::SIGN_IN as REKA_SIGN_IN;
pub use crate::engine::{CodexSignIn, RekaSignIn};

/// The agent-browser session the engine's browser tool uses, for the browser pane.
pub const BROWSER_SESSION: &str = crate::engine::BROWSER_SESSION;

/// Dev servers the app runs per folder, shared with the engine's `dev_server` tool.
pub mod dev_server {
    pub use crate::devserver::{detect, output, ready, start, status, stop, stop_all, Status};
}
use crate::Result;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Stores an API key for a configured API-key provider in `auth.json`.
/// Start a new engine afterwards, as the TUI reconnects after `/login`.
pub fn save_api_key(provider: &str, key: &str) -> Result<()> {
    crate::engine::auth::save_api_key(provider, key.to_owned())
}

/// The Reka Bytes account signed in for Rooxcode. Never carries its tokens.
pub struct RekaAccount {
    pub name: Option<String>,
    pub email: Option<String>,
    /// The pixel character picked on the Rooxcode account page; None is the default.
    pub avatar: Option<String>,
}
fn reka(a: crate::engine::reka::Account) -> RekaAccount {
    RekaAccount {
        name: a.name,
        email: a.email,
        avatar: a.avatar,
    }
}

/// The signed-in Reka Bytes account, shared with the terminal app.
pub fn reka_account() -> Option<RekaAccount> {
    crate::engine::reka::account().map(reka)
}

/// One budget window of the Rooxcode account, in percent only.
pub struct RekaWindow {
    pub percent: u8,
    /// Unix seconds when the oldest counted usage leaves the window.
    pub drops_at: Option<i64>,
    /// Seconds until a request fits again, when the window is full.
    pub retry_after: Option<u64>,
}

/// The Account page: the profile and usage, fresh from Rooxcode.
pub struct RekaUsage {
    pub account: RekaAccount,
    /// None when that limit is off.
    pub five_hours: Option<RekaWindow>,
    pub week: Option<RekaWindow>,
    /// Wallet credits, which pay for requests once a window is full.
    pub credits: Option<f64>,
    /// This device's name on the website's device list.
    pub device: Option<String>,
}

/// Loads the Account page. Fails when signed out or when Rooxcode can't be reached.
pub async fn reka_usage() -> Result<RekaUsage> {
    let (account, usage) = crate::engine::reka::usage().await?;
    let window = |w: crate::engine::reka::Window| RekaWindow {
        percent: w.percent,
        drops_at: w.drops_at,
        retry_after: w.retry_after,
    };
    Ok(RekaUsage {
        account: reka(account),
        five_hours: usage.five_hours.map(window),
        week: usage.week.map(window),
        credits: usage.credits,
        device: usage.device,
    })
}

/// The signed-in account with its name and avatar reloaded from Rooxcode.
/// Falls back to the saved profile offline.
pub async fn reka_profile() -> Option<RekaAccount> {
    crate::engine::reka::refresh_profile().await.map(reka)
}

/// Removes the stored credential for `provider`: its API key, the Codex
/// login, or for `rooxcode` the Reka Bytes session.
pub fn sign_out(provider: &str) -> Result<()> {
    crate::engine::logout::remove(provider)
}

/// Whether OpenRouter requests ask for providers that don't store or train on prompts.
pub fn training_denied() -> bool {
    crate::engine::privacy::training_denied()
}

/// Saves the training choice. Engines read it on every request, so no restart is needed.
pub fn set_training_denied(deny: bool) -> Result<()> {
    crate::engine::privacy::set_training_denied(deny)
}

/// Where to create an API key for a built-in provider, e.g. `openrouter.ai/settings/keys`.
pub fn key_page(provider: &str) -> Option<&'static str> {
    crate::engine::catalog::key_page(provider)
}

/// A saved session for a workspace.
pub struct SavedSession {
    pub path: PathBuf,
    pub title: String,
    pub modified: SystemTime,
}

/// Saved sessions for `workspace`, newest first.
pub fn sessions(workspace: &Path) -> Result<Vec<SavedSession>> {
    Ok(
        crate::chat::saved_sessions::list(&workspace.canonicalize()?)?
            .into_iter()
            .map(|s| SavedSession {
                path: s.path,
                title: s.title,
                modified: s.modified,
            })
            .collect(),
    )
}

/// Names a saved session that no engine has open; an empty name restores its
/// automatic title. Rename the open session with the `set_session_name` command.
pub fn rename_session(path: &Path, name: &str) -> Result<()> {
    crate::engine::session::rename_saved(path, name)
}

/// Deletes a saved session. Refuses files outside the sessions directory.
pub fn delete_session(path: &Path) -> Result<()> {
    crate::engine::session::delete_saved(path)
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
    use crate::engine::config;
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
