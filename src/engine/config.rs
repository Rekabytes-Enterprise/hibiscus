use crate::sys::{DirBuilderExt, OpenOptionsExt};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

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
    Rooxcode,
    Local,
}
fn is_false(value: &bool) -> bool {
    !value
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_request_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_messages: Option<u64>,
    #[serde(default = "default_output")]
    pub max_tokens: u64,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub reasoning_efforts: Vec<super::thinking::ReasoningEffort>,
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
    #[serde(default, skip_serializing_if = "is_false")]
    pub discover_models: bool,
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
    /// Providers whose models come from their own catalog at runtime.
    #[serde(skip)]
    pub(super) discovered: std::collections::HashSet<String>,
    /// Codex release sent when listing Codex models; looked up once per engine run.
    #[serde(skip)]
    pub(super) codex_version: Option<String>,
}
fn default_steps() -> usize {
    64
}
fn model(id: &str) -> Model {
    Model {
        id: id.into(),
        context_window: None,
        max_request_bytes: None,
        max_messages: None,
        max_tokens: 4096,
        reasoning: false,
        reasoning_efforts: vec![],
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
            discover_models: false,
            api_key_env: Some("OPENAI_API_KEY".into()),
            models: vec![model("gpt-4.1")],
        },
        Provider {
            id: "anthropic".into(),
            base_url: "https://api.anthropic.com/v1".into(),
            api: Api::Anthropic,
            authentication: Authentication::ApiKey,
            discover_models: false,
            api_key_env: Some("ANTHROPIC_API_KEY".into()),
            models: vec![model("claude-sonnet-4-5")],
        },
    ]
}
/// `config.json` as written, or `{}` when there is none.
fn raw() -> Result<serde_json::Value> {
    match fs::read(crate::paths::home()?.join("config.json")) {
        Ok(b) => Ok(serde_json::from_slice(&b)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(e) => Err(e.into()),
    }
}

pub fn load() -> Result<Config> {
    parse(raw()?)
}

fn parse(raw: serde_json::Value) -> Result<Config> {
    // Older installers wrote codexContextWindow. It no longer applies;
    // ignore it like other unknown fields and use native defaults for
    // missing settings, without rewriting the user's configuration.
    let mut config: Config = serde_json::from_value(raw)?;
    // Subscription tokens must never be routed to a configurable third-party host.
    if !config.providers.iter().any(|p| p.id == "openai-codex") {
        config.providers.push(Provider {
            id: "openai-codex".into(),
            base_url: super::oauth::RESOURCE.into(),
            api: Api::Responses,
            authentication: Authentication::Codex,
            discover_models: false,
            api_key_env: None,
            // Fallback until the plan's own list is fetched (catalog::refresh).
            models: vec![Model {
                reasoning: true,
                ..model("gpt-6.1-sol")
            }],
        });
        config.discovered.insert("openai-codex".into());
    }
    super::rooxcode::install(&mut config);
    super::catalog::install(&mut config);
    let mut ids = std::collections::HashSet::new();
    for p in &mut config.providers {
        super::rooxcode::validate_provider(p)?;
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
        if (url.scheme() != "https" && !(private(&url) && url.scheme() == "http"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("provider baseUrl must use HTTPS, or HTTP on this computer or a private network, without credentials/query/fragment".into());
        }
        if p.discover_models {
            p.base_url = local_catalog_base(&p.base_url)?
                .as_str()
                .trim_end_matches('/')
                .to_owned();
            if p.api != Api::ChatCompletions
                || !matches!(
                    p.authentication,
                    Authentication::ApiKey | Authentication::Local
                )
                || super::catalog::key_page(&p.id).is_some()
            {
                return Err("Auto-detection requires a local Chat Completions provider.".into());
            }
            config.discovered.insert(p.id.clone());
        }
        if p.authentication == Authentication::Local
            && (!p.discover_models || p.api_key_env.is_some())
        {
            return Err(
                "Keyless local authentication requires an auto-detected local provider.".into(),
            );
        }
        let mut models = std::collections::HashSet::new();
        if p.models.iter().any(|m| {
            m.id.is_empty()
                || !models.insert(&m.id)
                || m.max_request_bytes
                    .is_some_and(|n| !(1024..=104_857_600).contains(&n))
                || m.max_messages.is_some_and(|n| !(4..=10000).contains(&n))
                || (m.max_tokens == 0 && !p.discover_models)
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

/// Validate the same URL for preview, saving and catalog refresh.
pub(crate) fn local_catalog_base(raw: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(raw.trim()).map_err(|_| "Enter a valid local server URL.")?;
    if !matches!(url.scheme(), "http" | "https")
        || !private(&url)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Use an HTTP or HTTPS URL on this computer or your private network, without credentials, a query or a fragment.".into());
    }
    if url.path() == "/" {
        url.set_path("/v1");
    }
    Ok(url)
}

/// Whether `url` points at this computer or a private network (home LAN,
/// Tailscale, `.local` names), where a local model server may run over HTTP.
pub fn private(url: &reqwest::Url) -> bool {
    use std::net::IpAddr;
    let host = url.host_str().unwrap_or_default();
    let Ok(ip) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    else {
        return host == "localhost" || host.ends_with(".local");
    };
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            // 100.64.0.0/10 is carrier-grade NAT, which Tailscale uses.
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || (a == 100 && (64..128).contains(&b))
        }
        // fc00::/7 unique local and fe80::/10 link-local.
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Adds `provider` to `config.json`, keeping everything else in the file.
/// Fails, writing nothing, if the result would not load.
pub fn add_provider(provider: &Provider) -> Result<()> {
    let mut raw = raw()?;
    let object = raw
        .as_object_mut()
        .ok_or("config.json must hold an object")?;
    // Without a providers list the built-in defaults apply; keep them when starting one.
    let list = object
        .entry("providers")
        .or_insert_with(|| serde_json::to_value(defaults()).unwrap_or_default())
        .as_array_mut()
        .ok_or("config.json providers must be a list")?;
    if list.iter().any(|p| p["id"] == provider.id.as_str()) {
        return Err(format!("A provider named {} already exists", provider.id).into());
    }
    list.push(serde_json::to_value(provider)?);
    parse(raw.clone())?;
    write(&raw)
}

/// Takes the provider `id` out of `config.json`. Nothing happens when it isn't there.
pub fn remove_provider(id: &str) -> Result<()> {
    let mut raw = raw()?;
    let Some(list) = raw.get_mut("providers").and_then(|p| p.as_array_mut()) else {
        return Ok(());
    };
    let before = list.len();
    list.retain(|p| p["id"] != id);
    if list.len() == before {
        return Ok(());
    }
    write(&raw)
}

fn write(raw: &serde_json::Value) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(raw)?;
    bytes.push(b'\n');
    atomic(&crate::paths::home()?.join("config.json"), &bytes)
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
    if provider.authentication == Authentication::Rooxcode {
        return Err("Rooxcode does not use provider credentials".into());
    }
    if provider.authentication == Authentication::Codex {
        return super::oauth::stored_access_token();
    }
    if provider.authentication == Authentication::Local {
        return Err("This local server does not use provider credentials.".into());
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

pub fn access(provider: &Provider) -> Result<()> {
    if provider.authentication == Authentication::Rooxcode {
        super::rooxcode::validate_provider(provider)
    } else if provider.authentication == Authentication::Local {
        Ok(())
    } else {
        key(provider).map(|_| ())
    }
}

#[cfg(test)]
mod private_tests {
    #[test]
    fn http_is_allowed_only_on_this_computer_or_a_private_network() {
        let private = |u: &str| super::private(&reqwest::Url::parse(u).unwrap());
        for u in [
            "http://localhost:8080/v1",
            "http://127.0.0.1:1/v1",
            "http://[::1]:1/v1",
            "http://10.0.0.10:8000/v1",
            "http://10.0.0.5/v1",
            "http://172.20.1.1/v1",
            "http://100.101.2.3/v1",
            "http://rkb.local:8080/v1",
            "http://[fd00::1]/v1",
        ] {
            assert!(private(u), "{u}");
        }
        for u in [
            "http://example.com/v1",
            "http://8.8.8.8/v1",
            "http://172.32.0.1/v1",
            "http://100.128.0.1/v1",
            "http://localhost.example.com/v1",
        ] {
            assert!(!private(u), "{u}");
        }
    }
}
