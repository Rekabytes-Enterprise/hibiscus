use crate::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Api {
    Responses,
    ChatCompletions,
    Anthropic,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Authentication {
    #[default]
    ApiKey,
    Codex,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default = "default_output")]
    pub max_tokens: u64,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub images: bool,
}
fn default_output() -> u64 {
    4096
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub base_url: String,
    pub api: Api,
    #[serde(default)]
    pub authentication: Authentication,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub models: Vec<Model>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "defaults")]
    pub providers: Vec<Provider>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_steps")]
    pub max_steps: usize,
}
fn default_steps() -> usize {
    64
}
fn model(id: &str) -> Model {
    Model {
        id: id.into(),
        context_window: None,
        max_tokens: 4096,
        reasoning: false,
        images: true,
    }
}
fn defaults() -> Vec<Provider> {
    vec![
        Provider {
            id: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api: Api::Responses,
            authentication: Authentication::ApiKey,
            api_key_env: Some("OPENAI_API_KEY".into()),
            models: vec![model("gpt-4.1")],
        },
        Provider {
            id: "anthropic".into(),
            base_url: "https://api.anthropic.com/v1".into(),
            api: Api::Anthropic,
            authentication: Authentication::ApiKey,
            api_key_env: Some("ANTHROPIC_API_KEY".into()),
            models: vec![model("claude-sonnet-4-5")],
        },
    ]
}
pub fn load() -> Result<Config> {
    let path = crate::paths::home()?.join("config.json");
    let mut config: Config = match fs::read(path) {
        Ok(b) => {
            let raw: serde_json::Value = serde_json::from_slice(&b)?;
            if raw.get("codexContextWindow").is_some() && raw.get("providers").is_none() {
                return Err("Legacy config.json needs native providers/model configuration. See docs/native-runtime.md; original configuration is unchanged.".into());
            }
            serde_json::from_value(raw)?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::from_str("{}")?,
        Err(e) => return Err(e.into()),
    };
    // Subscription tokens must never be routed to a configurable third-party host.
    if !config.providers.iter().any(|p| p.id == "openai-codex") {
        config.providers.push(Provider {
            id: "openai-codex".into(),
            base_url: super::oauth::RESOURCE.into(),
            api: Api::Responses,
            authentication: Authentication::Codex,
            api_key_env: None,
            models: vec![Model {
                reasoning: true,
                ..model("gpt-6.1-sol")
            }],
        });
    }
    let mut ids = std::collections::HashSet::new();
    for p in &config.providers {
        if (p.authentication == Authentication::Codex || p.id == "openai-codex")
            && (p.id != "openai-codex"
                || p.authentication != Authentication::Codex
                || p.api != Api::Responses
                || p.base_url != super::oauth::RESOURCE
                || p.api_key_env.is_some())
        {
            return Err("openai-codex requires native Codex authentication and its fixed subscription Responses endpoint".into());
        }
        if p.id.is_empty() || p.id.contains('/') || !ids.insert(&p.id) {
            return Err(
                "config.json provider IDs must be nonempty, unique and contain no slash".into(),
            );
        }
        let url = reqwest::Url::parse(&p.base_url)?;
        let local = matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
        );
        if (url.scheme() != "https" && !(local && url.scheme() == "http"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("provider baseUrl must use HTTPS, or HTTP on loopback, without credentials/query/fragment".into());
        }
        let mut models = std::collections::HashSet::new();
        if p.models.iter().any(|m| {
            m.id.is_empty()
                || !models.insert(&m.id)
                || m.max_tokens == 0
                || m.context_window.is_some_and(|n| n <= m.max_tokens)
        }) {
            return Err("invalid or duplicate model configuration".into());
        }
    }
    if !(1..=1024).contains(&config.max_steps) {
        return Err("maxSteps must be between 1 and 1024".into());
    }
    if let Ok(id) = std::env::var("HIBISCUS_MODEL") {
        config.model = Some(id);
    }
    Ok(config)
}
pub fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or("file has no parent")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let temp = parent.join(format!(".hibiscus-{}-{}.tmp", std::process::id(), unique()));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub fn unique() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        + NEXT.fetch_add(1, Ordering::Relaxed) as u128
}
pub fn key(provider: &Provider) -> Result<String> {
    if provider.authentication == Authentication::Codex {
        return super::oauth::stored_access_token();
    }
    if let Some(key) = provider
        .api_key_env
        .as_ref()
        .and_then(|env| std::env::var(env).ok())
        .filter(|s| !s.trim().is_empty())
    {
        return Ok(key);
    }
    let keys = super::auth::keys()?;
    keys.get(&provider.id).cloned().ok_or_else(||format!("Authentication required for {}. Use /login or configure its API key environment variable.",provider.id).into())
}
