//! Built-in API-key providers whose models come from their own /models catalog.
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
            let key = super::config::key(p).ok()?;
            let id = p.id.clone();
            Some((id.clone(), tokio::spawn(fetch(id, models_url(p), key))))
        })
        .collect::<Vec<_>>();
    let mut errors = vec![];
    for (id, job) in jobs {
        let result = job
            .await
            .unwrap_or_else(|_| Err("model discovery stopped.".into()));
        let provider = config.providers.iter_mut().find(|p| p.id == id).unwrap();
        match result {
            Ok(models) => provider.models = models,
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
    let mut response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()?
        .get(url)
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| failed())?;
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
    parse(&bytes)
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
