//! Library API for the desktop app. The CLI and TUI do not use it.
//! See `docs/desktop/protocol.md` for the commands and events.
pub use crate::client::embed::{Engine, Start};
/// The error a Rooxcode prompt gets without a Reka Bytes session.
pub use crate::engine::reka::SIGN_IN as REKA_SIGN_IN;
pub use crate::engine::session::SessionKind;
pub use crate::engine::RekaSignIn;
pub use crate::pdf::prepare as prepare_pdf;

/// The agent-browser session the engine's browser tool uses, for the browser pane.
pub const BROWSER_SESSION: &str = crate::engine::BROWSER_SESSION;

/// WSL 2 connections for the Windows app: detect distros, connect a folder,
/// and find a distro's home for the folder picker.
pub mod wsl {
    pub use crate::wsl::{connect, detect, home, Distro};
}

/// Where a session's canvas is kept: one JSON file per session, named by a hash of its path.
pub(crate) fn canvas_board(session: &std::path::Path) -> Result<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(session.as_os_str().as_encoded_bytes());
    let name: String = hash[..12].iter().map(|b| format!("{b:02x}")).collect();
    Ok(crate::paths::home()?
        .join("canvas")
        .join("boards")
        .join(format!("{name}.json")))
}

/// The canvas saved with `session`, as the app wrote it, or `None`.
pub fn load_canvas(session: &std::path::Path) -> Result<Option<String>> {
    match std::fs::read_to_string(canvas_board(session)?) {
        Ok(json) => Ok(Some(json)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Saves `session`'s canvas, replacing the file atomically. An empty canvas removes it.
pub fn save_canvas(session: &std::path::Path, json: &str) -> Result<()> {
    const MAX: usize = 64 * 1024 * 1024;
    if json.len() > MAX {
        return Err(
            "This canvas is too large to save (over 64 MiB). Remove some frames or audio.".into(),
        );
    }
    let path = canvas_board(session)?;
    if json.trim().is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().ok_or("No canvas folder")?)?;
    crate::engine::config::atomic(&path, json.as_bytes())
}

/// A saved reference's bytes, for showing it again after a restart. Only files
/// under `~/.hibiscus/canvas/references` can be read this way.
pub fn read_canvas_reference(path: &std::path::Path) -> Result<Vec<u8>> {
    let dir = crate::paths::home()?
        .join("canvas")
        .join("references")
        .canonicalize()?;
    let path = path.canonicalize()?;
    if !path.starts_with(&dir) {
        return Err("Only canvas references can be read".into());
    }
    Ok(std::fs::read(path)?)
}

/// Saves a reference file added on a canvas card under `~/.hibiscus/canvas/references`
/// so the agent can open it, and returns its path. The name keeps only safe characters.
pub fn save_canvas_reference(name: &str, bytes: &[u8]) -> Result<std::path::PathBuf> {
    const MAX: usize = 2 * 1024 * 1024 * 1024;
    if bytes.is_empty() || bytes.len() > MAX {
        return Err("References must be between 1 byte and 2 GiB".into());
    }
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim_start_matches('.');
    let dir = crate::paths::home()?.join("canvas").join("references");
    std::fs::create_dir_all(&dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let path = dir.join(format!(
        "{stamp}-{}",
        if safe.is_empty() { "reference" } else { safe }
    ));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// A video style: a skill whose frontmatter sets `hibiscus-style: true`. It never
/// sets the voice; videos use the voice generated in the session.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    pub name: String,
    pub description: String,
    /// The SKILL.md the agent is told to follow.
    pub path: std::path::PathBuf,
}

/// The `key: value` lines of a SKILL.md's frontmatter.
fn frontmatter(text: &str) -> std::collections::HashMap<String, String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Default::default();
    }
    lines
        .take_while(|l| l.trim() != "---")
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().trim_matches('"').to_owned()))
        .collect()
}

pub(crate) fn style(path: &std::path::Path, text: &str) -> Option<Style> {
    let fields = frontmatter(text);
    (fields.get("hibiscus-style").map(String::as_str) == Some("true")).then(|| Style {
        name: fields.get("name").cloned().unwrap_or_default(),
        description: fields.get("description").cloned().unwrap_or_default(),
        path: path.to_owned(),
    })
}

static BUNDLED_SKILLS: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// The skills folder shipped inside the app, set once at startup.
pub fn set_bundled_skills(dir: std::path::PathBuf) {
    let _ = BUNDLED_SKILLS.set(dir);
}

/// Skill folders in the order they are read: the app's own, then the user's
/// `~/.hibiscus/skills`. A personal skill with the same name replaces the bundled one.
pub(crate) fn skill_dirs() -> Vec<std::path::PathBuf> {
    BUNDLED_SKILLS
        .get()
        .cloned()
        .into_iter()
        .chain(crate::paths::home().ok().map(|h| h.join("skills")))
        .collect()
}

/// The skills folder shipped with the app, once the app has set it.
pub(crate) fn bundled_skills() -> Option<std::path::PathBuf> {
    BUNDLED_SKILLS.get().cloned()
}

/// The video styles from the app's bundled skills and `~/.hibiscus/skills`, sorted by name.
pub fn styles() -> Vec<Style> {
    let mut found: Vec<Style> = Vec::new();
    for dir in skill_dirs() {
        for path in std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(std::result::Result::ok)
            .map(|e| e.path().join("SKILL.md"))
        {
            let Some(style) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| style(&path, &t))
            else {
                continue;
            };
            if style.name.is_empty() {
                continue;
            }
            found.retain(|s| s.name != style.name);
            found.push(style);
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// The video engine (Node.js, HyperFrames and its Chrome) the canvas's
/// "Download engine" button installs into `~/.hibiscus/runtime`.
pub mod video_engine {
    pub use crate::video_engine::{install, installed, preview, Progress};
}

/// The ElevenLabs connector: its key, models, voices and text to speech.
pub mod elevenlabs {
    pub use crate::elevenlabs::{
        connect, connected, disconnect, method, models, speak, voices, Model, Speech, Voice,
        KEY_PAGE,
    };
    pub use crate::engine::elevenlabs_auth::ElevenLabsSignIn;
}

/// Dev servers the app runs per folder, shared with the engine's `dev_server` tool.
pub mod dev_server {
    pub use crate::devserver::{detect, output, ready, start, status, stop, stop_all, Status};
}

/// Commands the agent runs in the background, for the Background tasks pane.
pub mod jobs {
    pub use crate::jobs::{clear, list, stop, stop_all, Info};
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

/// Removes the stored API key, ChatGPT plan login, or Reka Bytes session.
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
    pub kind: SessionKind,
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
                kind: s.kind,
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

/// One pending browser sign-in for the ChatGPT plan provider.
pub struct ChatGptSignIn {
    flow: crate::engine::chatgpt::BrowserSignIn,
    paste: tokio::sync::mpsc::Sender<String>,
    pasted: tokio::sync::mpsc::Receiver<String>,
}

impl ChatGptSignIn {
    pub fn start() -> Result<Self> {
        let flow = crate::engine::chatgpt::BrowserSignIn::start()?;
        let (paste, pasted) = tokio::sync::mpsc::channel(1);
        Ok(Self {
            flow,
            paste,
            pasted,
        })
    }

    pub fn url(&self) -> &str {
        self.flow.url()
    }

    pub fn paste(&self) -> tokio::sync::mpsc::Sender<String> {
        self.paste.clone()
    }

    pub async fn finish(self) -> Result<()> {
        let Self {
            flow,
            paste: _paste,
            pasted,
        } = self;
        let registration =
            tokio::time::timeout(std::time::Duration::from_secs(600), flow.complete(pasted))
                .await
                .map_err(|_| "ChatGPT sign-in timed out. Try again.")??;
        crate::engine::chatgpt::save(&registration)
    }
}

/// A configured provider, for the login screen.
pub struct ProviderInfo {
    pub id: String,
    /// Signs in with an API key (other providers use their own flow).
    pub api_key: bool,
    /// Signs in with a native browser OAuth flow.
    pub browser: bool,
    pub signed_in: bool,
    pub key_page: Option<&'static str>,
    /// Added from Settings as a local AI server; Settings can remove it.
    pub local: bool,
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
            browser: p.authentication == config::Authentication::ChatGptPlan,
            signed_in: config::access(p).is_ok(),
            key_page: key_page(&p.id),
            local: matches!(
                p.authentication,
                config::Authentication::ApiKey | config::Authentication::Local
            ) && reqwest::Url::parse(&p.base_url).is_ok_and(|u| config::private(&u)),
        })
        .collect())
}

/// A model server on this computer or the home network, such as Ollama, LM Studio or vLLM.
pub struct LocalProvider {
    /// Its name in Hibiscus, e.g. `ornith`; models show as `ornith/<model>`.
    pub id: String,
    /// Where its API starts, e.g. `http://localhost:8000/v1`.
    pub base_url: String,
    /// `chat_completions`, `responses` or `anthropic`.
    pub api: String,
    pub models: Vec<String>,
    pub context_window: Option<u64>,
    pub discover_models: bool,
    /// Empty when the server takes no key.
    pub key: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    id: String,
    context_window: Option<u64>,
    reasoning: bool,
    images: bool,
}

/// Preview the catalog without storing the server or its key.
pub async fn detect_local_models(base_url: &str, key: &str) -> Result<Vec<LocalModel>> {
    Ok(local_models(base_url, key)
        .await?
        .into_iter()
        .map(|model| LocalModel {
            id: model.id,
            context_window: model.context_window,
            reasoning: model.reasoning,
            images: model.images,
        })
        .collect())
}

async fn local_models(base_url: &str, key: &str) -> Result<Vec<crate::engine::config::Model>> {
    let key = key.trim();
    if key.len() > 8192 || key.chars().any(char::is_control) {
        return Err("The optional API key contains invalid characters or is too long.".into());
    }
    let models =
        crate::engine::catalog::fetch_local(base_url, (!key.is_empty()).then_some(key)).await?;
    if models.is_empty() {
        return Err("The local catalog returned no usable models.".into());
    }
    Ok(models)
}

/// Adds a local model server to `config.json` and stores its key.
/// Start a new engine afterwards, as after saving an API key.
pub async fn add_local_provider(local: LocalProvider) -> Result<()> {
    use crate::engine::config;
    let api = serde_json::from_value(serde_json::Value::String(local.api.clone()))
        .map_err(|_| format!("Unknown API {}", local.api))?;
    let models: Vec<String> = local
        .models
        .iter()
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty())
        .collect();
    if !local.discover_models && models.is_empty() {
        return Err("Add at least one model name".into());
    }
    let (base_url, authentication, models) = if local.discover_models {
        if api != config::Api::ChatCompletions {
            return Err("Auto-detection uses the OpenAI Chat Completions API.".into());
        }
        let base_url = config::local_catalog_base(&local.base_url)?.to_string();
        let models = local_models(&base_url, &local.key).await?;
        let authentication = if local.key.trim().is_empty() {
            config::Authentication::Local
        } else {
            config::Authentication::ApiKey
        };
        (base_url, authentication, models)
    } else {
        let models = models
            .into_iter()
            .map(|id| {
                serde_json::from_value(
                    serde_json::json!({"id": id, "contextWindow": local.context_window}),
                )
            })
            .collect::<std::result::Result<_, _>>()?;
        (local.base_url, config::Authentication::ApiKey, models)
    };
    let provider = config::Provider {
        id: local.id.trim().to_owned(),
        base_url: base_url.trim().trim_end_matches('/').to_owned(),
        api,
        authentication,
        discover_models: local.discover_models,
        api_key_env: None,
        models,
    };
    if !reqwest::Url::parse(&provider.base_url).is_ok_and(|u| config::private(&u)) {
        return Err("A local AI server must be on this computer or your private network. Add other servers in config.json.".into());
    }
    config::add_provider(&provider)?;
    if provider.authentication == config::Authentication::Local {
        return Ok(());
    }
    // Keep the existing manual provider behavior for older keyless setups.
    let key = if local.key.trim().is_empty() {
        "local"
    } else {
        local.key.trim()
    };
    if let Err(e) = save_api_key(&provider.id, key) {
        let _ = config::remove_provider(&provider.id);
        return Err(e);
    }
    Ok(())
}

/// Removes a local AI server added with `add_local_provider`, and its key.
pub fn remove_local_provider(id: &str) -> Result<()> {
    sign_out(id)?;
    crate::engine::config::remove_provider(id)
}

#[cfg(test)]
mod style_tests {
    #[test]
    fn styles_come_from_marked_skills() {
        let path = std::path::Path::new("/s/SKILL.md");
        let text = "---\nname: tech-explainer-short\ndescription: A vertical tech short\nhibiscus-style: true\n---\n# Body";
        assert_eq!(
            super::style(path, text).unwrap().name,
            "tech-explainer-short"
        );
        assert!(super::style(path, "---\nname: other\n---\n").is_none());
        assert!(super::style(path, "no frontmatter").is_none());
    }
}
