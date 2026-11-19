//! Built-in providers whose models come from their own /models catalog: the
//! API-key providers below, and Codex, whose list depends on the signed-in plan.
//! Values are copied from the provider; Hibiscus adds no caps or defaults.
use super::config::{Api, Authentication, Config, Model, Provider};
use crate::Result;
use serde::Deserialize;
use std::{collections::HashSet, time::Duration};

struct Builtin {
    id: &'static str,
    base_url: &'static str,
    models_url: &'static str,
    key_env: &'static str,
    key_page: &'static str,
}
const BUILTINS: [Builtin; 2] = [
    Builtin {
        id: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        models_url: "https://openrouter.ai/api/v1/models",
        key_env: "OPENROUTER_API_KEY",
        key_page: "openrouter.ai/settings/keys",
    },
    Builtin {
        id: "opencode-go",
        base_url: "https://opencode.ai/inference/go/openai/v1",
        models_url: "https://opencode.ai/inference/go/v1/models",
        key_env: "OPENCODE_API_KEY",
        key_page: "opencode.ai/auth",
    },
];
/// Codex lists a model only to clients at least as new as its `minimal_client_version`
/// and requires the parameter, so Hibiscus sends the current official Codex release:
/// looked up on npm once per engine run, else the last one seen, else this pin.
const CODEX_VERSION_PIN: &str = "0.167.0";
const CODEX_VERSION_URL: &str = "https://registry.npmjs.org/@openai/codex/latest";
const CODEX_VERSION_FILE: &str = "codex-client-version";
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_MODELS: usize = 5000;

/// Adds missing built-ins. A provider whose config lists no models is discovered.
pub(super) fn install(config: &mut Config) {
    for b in &BUILTINS {
        if !config.providers.iter().any(|p| p.id == b.id) {
            config.providers.push(Provider {
                id: b.id.into(),
                base_url: b.base_url.into(),
                api: Api::ChatCompletions,
                authentication: Authentication::ApiKey,
                api_key_env: Some(b.key_env.into()),
                models: vec![],
            });
        }
        if config
            .providers
            .iter()
            .any(|p| p.id == b.id && p.models.is_empty())
        {
            config.discovered.insert(b.id.into());
        }
    }
}

pub(crate) fn key_page(provider: &str) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|b| b.id == provider)
        .map(|b| b.key_page)
}

/// Whether the provider's model list comes from discovery, not config.json.
pub(super) fn discovered(config: &Config, provider: &str) -> bool {
    config.discovered.contains(provider)
}

fn models_url(provider: &Provider) -> String {
    BUILTINS
        .iter()
        .find(|b| b.id == provider.id && b.base_url == provider.base_url)
        .map(|b| b.models_url.to_owned())
        .unwrap_or_else(|| format!("{}/models", provider.base_url.trim_end_matches('/')))
}

/// Refreshes every discovered provider that has a key. Returns one message per failure.
pub(super) async fn refresh(config: &mut Config) -> Vec<String> {
    let jobs = config
        .providers
        .iter()
        .filter(|p| config.discovered.contains(&p.id))
        .filter_map(|p| {
            let id = p.id.clone();
            let job = if p.authentication == Authentication::Codex {
                super::oauth::stored_access_token().ok()?;
                tokio::spawn(fetch_codex(config.codex_version.clone()))
            } else {
                let key = super::config::key(p).ok()?;
                let fetched = fetch(id.clone(), models_url(p), key);
                tokio::spawn(async move { fetched.await.map(|models| (models, None)) })
            };
            Some((id, job))
        })
        .collect::<Vec<_>>();
    let mut errors = vec![];
    for (id, job) in jobs {
        let result = job
            .await
            .unwrap_or_else(|_| Err("model discovery stopped.".into()));
        let provider = config.providers.iter_mut().find(|p| p.id == id).unwrap();
        match result {
            Ok((models, version)) => {
                provider.models = models;
                if version.is_some() {
                    config.codex_version = version;
                }
            }
            // Keep the last good list so an outage does not drop the selected model.
            Err(error) => errors.push(format!("{id}: {error}")),
        }
    }
    errors
}

async fn fetch(id: String, url: String, key: String) -> Result<Vec<Model>> {
    let failed = || {
        format!("model discovery failed. Check connectivity and your {id} API key, then open /models to retry.")
    };
    let response = client()?
        .get(url)
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| failed())?;
    parse(&body(response, failed).await?)
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()?)
}

/// A successful response's body, at most 8 MiB.
async fn body(mut response: reqwest::Response, failed: impl Fn() -> String) -> Result<Vec<u8>> {
    if !response.status().is_success() {
        return Err(format!(
            "model discovery returned HTTP {}.",
            response.status().as_u16()
        )
        .into());
    }
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|_| failed())? {
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err("model catalog exceeds 8 MiB.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// The models the signed-in Codex plan offers. Only ever sent to the fixed Codex endpoint.
/// Also returns the Codex release it used, so later refreshes in this run reuse it.
async fn fetch_codex(known: Option<String>) -> Result<(Vec<Model>, Option<String>)> {
    let failed = || {
        "model discovery failed. Check connectivity, or sign in to Codex again with /login."
            .to_owned()
    };
    let token = super::oauth::credential().await?;
    let version = match known {
        Some(version) => version,
        None => {
            codex_version(
                &crate::paths::home()?.join(CODEX_VERSION_FILE),
                CODEX_VERSION_URL,
            )
            .await
        }
    };
    let url = format!("{}/models?client_version={version}", super::oauth::RESOURCE);
    let response = client()?
        .get(url)
        .bearer_auth(&token.access)
        .header("chatgpt-account-id", &token.account_id)
        .header("originator", "hibiscus")
        .send()
        .await
        .map_err(|_| failed())?;
    Ok((parse_codex(&body(response, failed).await?)?, Some(version)))
}

/// A plain release number such as `0.167.0`; anything else is not sent.
fn release(text: &str) -> Option<String> {
    let text = text.trim();
    let parts = text.split('.').collect::<Vec<_>>();
    (parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit())))
    .then(|| text.to_owned())
}

/// The current Codex release from `url` (npm), saved to `cache` as the fallback.
/// Never fails: a failed lookup uses the last release seen, then the pin. No credentials are sent.
async fn codex_version(cache: &std::path::Path, url: &str) -> String {
    #[derive(Deserialize)]
    struct Latest {
        version: String,
    }
    let looked_up = async {
        let response = client().ok()?.get(url).send().await.ok()?;
        let bytes = body(response, String::new).await.ok()?;
        release(&serde_json::from_slice::<Latest>(&bytes).ok()?.version)
    }
    .await;
    match looked_up {
        Some(version) => {
            // A file that cannot be written only loses the offline fallback.
            let _ = std::fs::write(cache, &version);
            version
        }
        None => std::fs::read_to_string(cache)
            .ok()
            .and_then(|t| release(&t))
            .unwrap_or_else(|| CODEX_VERSION_PIN.to_owned()),
    }
}

/// Reads Codex's `models[]`, keeping the ones its own app lists (`visibility: "list"`).
fn parse_codex(bytes: &[u8]) -> Result<Vec<Model>> {
    #[derive(Deserialize)]
    struct Catalog {
        models: Vec<Entry>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Entry {
        slug: String,
        visibility: String,
        context_window: Option<u64>,
        input_modalities: Vec<String>,
        supported_reasoning_levels: Vec<Level>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Level {
        effort: String,
    }
    let catalog: Catalog =
        serde_json::from_slice(bytes).map_err(|_| "returned an invalid model catalog.")?;
    if catalog.models.len() > MAX_MODELS {
        return Err("model catalog exceeds 5000 models.".into());
    }
    let mut seen = HashSet::new();
    let mut models = vec![];
    for e in catalog.models {
        if e.slug.trim().is_empty()
            || e.slug.len() > 256
            || e.slug.chars().any(char::is_control)
            || !seen.insert(e.slug.clone())
        {
            return Err("returned invalid or duplicate model IDs.".into());
        }
        if e.visibility != "list" {
            continue;
        }
        // Efforts Hibiscus does not know are left out rather than sent blindly.
        let efforts = e
            .supported_reasoning_levels
            .into_iter()
            .filter_map(|l| serde_json::from_value(serde_json::Value::String(l.effort)).ok())
            .collect::<Vec<_>>();
        models.push(Model {
            id: e.slug,
            context_window: e.context_window,
            max_request_bytes: None,
            max_messages: None,
            max_tokens: 0,
            reasoning: !efforts.is_empty(),
            reasoning_efforts: efforts,
            images: e.input_modalities.iter().any(|m| m == "image"),
        });
    }
    // An empty list would leave nothing to choose; keep the previous one instead.
    if models.is_empty() {
        return Err("listed no models for this plan.".into());
    }
    Ok(models)
}

/// Reads OpenRouter and OpenCode catalogs, which share `data[].id` but name limits differently.
fn parse(bytes: &[u8]) -> Result<Vec<Model>> {
    #[derive(Deserialize)]
    struct Catalog {
        data: Vec<Entry>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Entry {
        id: String,
        // OpenRouter
        context_length: Option<u64>,
        architecture: Option<Architecture>,
        supported_parameters: Option<Vec<String>>,
        // OpenCode
        limit: Option<Limit>,
        modalities: Option<Architecture>,
        capabilities: Option<Capabilities>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Architecture {
        input_modalities: Vec<String>,
        input: Vec<String>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Limit {
        context: Option<u64>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Capabilities {
        tools: Option<Supported>,
        reasoning: Option<Supported>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Supported {
        supported: bool,
    }
    let catalog: Catalog =
        serde_json::from_slice(bytes).map_err(|_| "returned an invalid model catalog.")?;
    if catalog.data.len() > MAX_MODELS {
        return Err("model catalog exceeds 5000 models.".into());
    }
    let mut seen = HashSet::new();
    let mut models = vec![];
    for e in catalog.data {
        if e.id.trim().is_empty()
            || e.id.len() > 256
            || e.id.chars().any(char::is_control)
            || !seen.insert(e.id.clone())
        {
            return Err("returned invalid or duplicate model IDs.".into());
        }
        let params = e.supported_parameters.as_deref();
        let has = |name: &str| params.is_some_and(|p| p.iter().any(|v| v == name));
        let tools = match (&e.capabilities, params) {
            (Some(c), _) => c.tools.as_ref().is_some_and(|t| t.supported),
            (None, Some(_)) => has("tools"),
            (None, None) => true,
        };
        // Hibiscus is an agent; a model that cannot call tools cannot do the work.
        if !tools {
            continue;
        }
        let inputs = [e.architecture, e.modalities]
            .into_iter()
            .flatten()
            .flat_map(|a| a.input_modalities.into_iter().chain(a.input))
            .collect::<Vec<_>>();
        let limit = e.limit.unwrap_or_default();
        models.push(Model {
            id: e.id,
            context_window: limit.context.or(e.context_length),
            max_request_bytes: None,
            max_messages: None,
            // 0 means the provider's own output limit: the request omits max_tokens.
            // Some report output equal to the whole window, so sending it would overflow.
            max_tokens: 0,
            reasoning: e
                .capabilities
                .and_then(|c| c.reasoning)
                .is_some_and(|r| r.supported)
                || has("reasoning"),
            reasoning_efforts: vec![],
            images: inputs.iter().any(|m| m.starts_with("image")),
        });
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::super::thinking::ReasoningEffort;
    use super::*;
    use serde_json::json;

    #[test]
    fn openrouter_catalog_uses_reported_values_and_skips_models_without_tools() {
        let models = parse(&serde_json::to_vec(&json!({"data":[
            {"id":"a/agent","context_length":262144,"top_provider":{"max_completion_tokens":262144},
             "architecture":{"input_modalities":["text","image"]},"supported_parameters":["tools","reasoning"]},
            {"id":"b/chat","context_length":8192,"supported_parameters":["temperature"]},
            {"id":"c/unknown","supported_parameters":["tools"]}
        ]})).unwrap()).unwrap();
        assert_eq!(models.len(), 2);
        let m = &models[0];
        assert_eq!((m.context_window, m.max_tokens), (Some(262144), 0));
        assert!(m.images && m.reasoning);
        let m = &models[1];
        assert_eq!(
            (m.id.as_str(), m.context_window, m.max_tokens),
            ("c/unknown", None, 0)
        );
        assert!(!m.images && !m.reasoning);
    }

    #[test]
    fn opencode_catalog_uses_reported_values_and_skips_models_without_tools() {
        let models = parse(&serde_json::to_vec(&json!({"object":"list","data":[
            {"id":"kimi","limit":{"context":1048576,"output":131072},
             "modalities":{"input":["text","image/png"]},
             "capabilities":{"tools":{"supported":true},"reasoning":{"supported":true,"effort":["max"]}}},
            {"id":"embed","capabilities":{"tools":{"supported":false}}}
        ]})).unwrap()).unwrap();
        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!((m.context_window, m.max_tokens), (Some(1048576), 0));
        assert!(m.images && m.reasoning);
    }

    #[test]
    fn codex_catalog_keeps_listed_models_with_their_limits_and_efforts() {
        let models = parse_codex(json!({"models":[
            {"slug":"gpt-a","visibility":"list","context_window":272000,"input_modalities":["text","image"],
             "supported_reasoning_levels":[{"effort":"low"},{"effort":"max"},{"effort":"future"}],"base_instructions":"x"},
            {"slug":"hidden","visibility":"hide"},
            {"slug":"gpt-b","visibility":"list","input_modalities":["text"]}
        ]}).to_string().as_bytes()).unwrap();
        assert_eq!(models.len(), 2);
        let a = &models[0];
        assert_eq!(
            (a.id.as_str(), a.context_window, a.images, a.reasoning),
            ("gpt-a", Some(272000), true, true)
        );
        assert_eq!(
            a.reasoning_efforts,
            [ReasoningEffort::Low, ReasoningEffort::Max]
        );
        assert_eq!(
            (models[1].id.as_str(), models[1].reasoning, models[1].images),
            ("gpt-b", false, false)
        );
        assert!(parse_codex(br#"{"models":[{"slug":"h","visibility":"hide"}]}"#).is_err());
        assert!(parse_codex(
            br#"{"models":[{"slug":"x","visibility":"list"},{"slug":"x","visibility":"list"}]}"#
        )
        .is_err());
        assert!(parse_codex(b"{}").is_err());
    }

    #[test]
    fn codex_version_asks_npm_then_falls_back_to_the_last_seen_then_the_pin() {
        let dir =
            std::env::temp_dir().join(format!("hibiscus-codex-version-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("v");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let get = |url: &str| runtime.block_on(codex_version(&cache, url));
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest", server.local_addr().unwrap());
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for (mut stream, body) in server.incoming().flatten().zip([
                r#"{"version":"0.200.1"}"#,
                r#"{"version":"0.201.0"}"#,
                r#"{"version":"1.0.0-beta"}"#,
            ]) {
                let _ = stream.read(&mut [0; 4096]);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        let offline = "http://127.0.0.1:9/latest";
        // Nothing seen and nothing reachable: the pin.
        assert_eq!(get(offline), CODEX_VERSION_PIN);
        // Each call asks npm and saves the answer.
        assert_eq!(get(&url), "0.200.1");
        assert_eq!(get(&url), "0.201.0");
        assert_eq!(std::fs::read_to_string(&cache).unwrap(), "0.201.0");
        // Offline, or a malformed answer: the last release seen.
        assert_eq!(get(offline), "0.201.0");
        assert_eq!(get(&url), "0.201.0");
        assert_eq!(release(" 0.167.0\n").as_deref(), Some("0.167.0"));
        assert!(
            release("99").is_none() && release("1.0.0-beta").is_none() && release("1..0").is_none()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_catalogs_are_rejected() {
        assert!(parse(b"{}").is_err());
        assert!(parse(br#"{"data":[{"id":"x"},{"id":"x"}]}"#).is_err());
        assert!(parse(br#"{"data":[{"id":" "}]}"#).is_err());
    }

    #[test]
    fn builtins_are_added_once_and_configured_models_are_not_discovered() {
        let mut config: Config = serde_json::from_value(json!({"providers":[
            {"id":"openrouter","baseUrl":"https://openrouter.ai/api/v1","api":"chat_completions","models":[{"id":"mine"}]}
        ]})).unwrap();
        install(&mut config);
        install(&mut config);
        assert_eq!(
            config
                .providers
                .iter()
                .filter(|p| p.id == "opencode-go")
                .count(),
            1
        );
        assert!(discovered(&config, "opencode-go") && !discovered(&config, "openrouter"));
        let go = config
            .providers
            .iter()
            .find(|p| p.id == "opencode-go")
            .unwrap();
        assert_eq!(models_url(go), "https://opencode.ai/inference/go/v1/models");
        let mut custom = go.clone();
        custom.base_url = "http://127.0.0.1:9/v1/".into();
        assert_eq!(models_url(&custom), "http://127.0.0.1:9/v1/models");
    }
}
