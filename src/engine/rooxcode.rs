//! Anonymous Rooxcode catalog and client metadata. These headers are not authentication.
use super::config::{Api, Authentication, Config, Model, Provider};
use crate::Result;
use rand::TryRng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};

pub(super) const ID: &str = "rooxcode";
pub(super) const URL: &str = "https://api-rooxcode.reka-bytes.my/v1";
// v0.1.9 permits up to 300s queued, then a 120s generation deadline.
// Keep a bounded 600s total, including the wait; keepalives never extend it.
const CHAT_TIMEOUT_SECONDS: u64 = 600;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub(super) struct SessionId(String);
impl SessionId {
    pub(super) fn new() -> Result<Self> {
        let mut bytes = [0u8; 16];
        rand::rngs::SysRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| "Cannot generate conversation UUID")?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        Ok(Self(format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        )))
    }
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for SessionId {
    type Error = &'static str;
    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        if value.len() != 36
            || !value.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            })
            || !value.bytes().any(|b| b.is_ascii_hexdigit() && b != b'0')
        {
            return Err("Invalid Rooxcode conversation UUID");
        }
        Ok(Self(value))
    }
}

pub(super) fn install(config: &mut Config) {
    if !config.providers.iter().any(|p| p.id == ID) {
        config.providers.push(Provider {
            id: ID.into(),
            base_url: URL.into(),
            api: Api::ChatCompletions,
            authentication: Authentication::Rooxcode,
            api_key_env: None,
            models: vec![],
        });
    }
}

pub(super) fn validate_provider(provider: &Provider) -> Result<()> {
    let service_host = reqwest::Url::parse(&provider.base_url)
        .ok()
        .is_some_and(|u| u.host_str() == Some("api-rooxcode.reka-bytes.my"));
    if (provider.id == ID || provider.authentication == Authentication::Rooxcode || service_host)
        && (provider.id != ID
            || provider.authentication != Authentication::Rooxcode
            || provider.api != Api::ChatCompletions
            || provider.api_key_env.is_some())
    {
        return Err("Rooxcode requires its no-key authentication mode and Chat Completions API; provider credentials cannot be attached".into());
    }
    Ok(())
}

fn client(timeout: u64) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(timeout))
        .build()?)
}

pub(super) fn chat_request(
    provider: &Provider,
    body: &Value,
    session: &SessionId,
) -> Result<reqwest::RequestBuilder> {
    validate_provider(provider)?;
    Ok(client(CHAT_TIMEOUT_SECONDS)?
        .post(format!(
            "{}/chat/completions",
            provider.base_url.trim_end_matches('/')
        ))
        .header("x-rooxcode-client", "hibiscus")
        .header("x-rooxcode-version", env!("CARGO_PKG_VERSION"))
        .header("x-rooxcode-session", session.as_str())
        .json(body))
}

async fn bounded_body(response: &mut reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Rooxcode response could not be read")?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("Rooxcode response exceeds its supported size".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(super) enum CatalogStatus {
    Available,
    Empty,
}
impl CatalogStatus {
    pub fn warning(&self) -> Option<&'static str> {
        match self {
            Self::Available => None,
            Self::Empty => Some("Rooxcode has no available models. Inference may be disabled or temporarily unavailable; open /models to refresh later."),
        }
    }
}

pub(super) async fn refresh(config: &mut Config) -> Result<CatalogStatus> {
    let provider = config
        .providers
        .iter_mut()
        .find(|p| p.id == ID)
        .ok_or("Rooxcode provider is unavailable")?;
    validate_provider(provider)?;
    provider.models.clear();
    let mut response = client(5)?
        .get(format!(
            "{}/models",
            provider.base_url.trim_end_matches('/')
        ))
        .send()
        .await
        .map_err(|_| {
            "Rooxcode model discovery failed. Check connectivity and open /models to refresh."
        })?;
    if !response.status().is_success() {
        return Err(response_error(response).await.into());
    }
    provider.models = catalog(&bounded_body(&mut response, 1024 * 1024).await?)?;
    Ok(if provider.models.is_empty() {
        CatalogStatus::Empty
    } else {
        CatalogStatus::Available
    })
}

fn catalog(bytes: &[u8]) -> Result<Vec<Model>> {
    #[derive(Deserialize)]
    struct Catalog {
        data: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        id: String,
        #[serde(default)]
        reasoning: Option<bool>,
        #[serde(default)]
        reasoning_efforts: Option<Vec<super::thinking::ReasoningEffort>>,
        #[serde(default)]
        context_window: Option<u64>,
        #[serde(default)]
        max_request_bytes: Option<u64>,
        #[serde(default)]
        max_messages: Option<u64>,
    }
    let catalog: Catalog =
        serde_json::from_slice(bytes).map_err(|_| "Rooxcode returned an invalid model catalog")?;
    if catalog.data.len() > 1000 {
        return Err("Rooxcode model catalog exceeds 1000 models".into());
    }
    let mut seen = HashSet::new();
    catalog
        .data
        .into_iter()
        .map(|entry| {
            let efforts = entry.reasoning_efforts.unwrap_or_default();
            let mut seen_efforts = HashSet::new();
            if efforts.iter().any(|effort| !seen_efforts.insert(*effort)) {
                return Err("Rooxcode returned invalid reasoning metadata".into());
            }
            if entry.id.trim().is_empty()
                || entry.id.len() > 256
                || entry.id.chars().any(char::is_control)
                || entry
                    .context_window
                    .is_some_and(|n| !(4097..=10_000_000).contains(&n))
                || entry
                    .max_request_bytes
                    .is_some_and(|n| !(1024..=104_857_600).contains(&n))
                || entry
                    .max_messages
                    .is_some_and(|n| !(4..=10000).contains(&n))
                || !seen.insert(entry.id.clone())
            {
                return Err("Rooxcode returned invalid or duplicate model IDs".into());
            }
            Ok(Model {
                id: entry.id,
                context_window: entry
                    .context_window
                    .filter(|n| (4097..=10_000_000).contains(n)),
                max_request_bytes: entry
                    .max_request_bytes
                    .filter(|n| (1024..=104_857_600).contains(n)),
                max_messages: entry.max_messages.filter(|n| (4..=10000).contains(n)),
                max_tokens: 4096,
                reasoning: entry.reasoning == Some(true),
                reasoning_efforts: if entry.reasoning == Some(true) {
                    efforts
                } else {
                    vec![]
                },
                images: false,
            })
        })
        .collect()
}

pub(super) fn code_message(status: u16, code: Option<&str>) -> String {
    let detail = match code {
        Some("request_too_large") => "the conversation exceeds the gateway request size limit. Compact the conversation before continuing",
        Some("hibiscus_client_required") => "client metadata was rejected. Update Hibiscus; no account or API key is needed",
        Some("inference_disabled") => "inference is disabled by the operator",
        Some("model_not_found" | "model_unavailable" | "model_ineligible") => "the selected model is unavailable. Open /models and explicitly choose an available model",
        Some("queue_full") => "the shared waiting queue is full. Retry manually later",
        Some("network_queue_full") => "your network already has its maximum number of waiting requests. Let those requests finish before retrying manually",
        Some("queue_timeout") => "the maximum queue wait elapsed before generation started. Retry manually later",
        Some("queue_disabled") => "request waiting was disabled by the operator. This request has stopped; retry manually when capacity is available",
        Some("network_daily_quota_exhausted") => "your network's daily quota is exhausted. Waiting cannot restore this allowance; retry manually after reset",
        Some("shared_quota_exhausted") => "the shared daily quota is exhausted. Waiting cannot restore this allowance; retry manually after reset",
        Some("upstream_quota_exhausted") => "the upstream free request quota is exhausted. Waiting cannot restore this allowance",
        Some("network_work_budget_exhausted") => "your network's daily work budget for input/output is exhausted. Retry manually after reset",
        _ if status == 429 => "rate limit or shared capacity is exhausted",
        _ if status >= 500 => "the service or upstream provider is unavailable",
        _ => "the request was rejected; check model availability and request support",
    };
    format!("Rooxcode HTTP {status}: {detail}. Nothing was retried or switched automatically.")
}

pub(super) fn model_output(event: &Value) -> bool {
    event["choices"].as_array().is_some_and(|choices| {
        choices.iter().any(|choice| {
            let delta = &choice["delta"];
            ["content", "reasoning", "reasoning_content", "refusal"]
                .iter()
                .any(|name| delta[*name].as_str().is_some_and(|v| !v.is_empty()))
                || delta["tool_calls"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty())
        })
    })
}

fn retry_seconds(value: Option<u64>) -> Option<u64> {
    value.filter(|seconds| *seconds <= 86400)
}

fn with_retry(mut message: String, retry: Option<u64>) -> String {
    if let Some(seconds) = retry_seconds(retry) {
        message.push_str(&format!(
            " Retry-After: {seconds} seconds; retry manually later."
        ));
    }
    message
}

pub(super) fn stream_error(event: &Value) -> String {
    let error = &event["error"];
    let status = error["status"]
        .as_u64()
        .filter(|s| (400..=599).contains(s))
        .and_then(|s| u16::try_from(s).ok())
        .unwrap_or(502);
    with_retry(
        code_message(status, error["code"].as_str()).replacen(
            "Rooxcode HTTP",
            "Rooxcode stream error",
            1,
        ),
        error["retry_after"].as_u64(),
    )
}

pub(super) async fn response_error(mut response: reqwest::Response) -> String {
    let status = response.status().as_u16();
    let retry = response
        .headers()
        .get("retry-after")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.parse::<u64>().ok());
    let body = bounded_body(&mut response, 65536)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let message = code_message(
        status,
        body.as_ref().and_then(|b| b["error"]["code"].as_str()),
    );
    with_retry(message, retry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn queue_stream_errors_validate_status_retry_and_never_echo_untrusted_text() {
        let error = stream_error(
            &json!({"error":{"code":"queue_timeout","status":429,"retry_after":5,"message":"PRIVATE"}}),
        );
        assert!(
            error.contains("stream error 429")
                && error.contains("maximum queue wait")
                && error.contains("Retry-After: 5")
        );
        assert!(!error.contains("PRIVATE"));
        for invalid in [
            json!(-1),
            json!(1.5),
            json!("5\nPRIVATE"),
            json!(u64::MAX),
            Value::Null,
        ] {
            let error = stream_error(
                &json!({"error":{"code":"queue_full","status":invalid,"retry_after":invalid,"message":"PRIVATE"}}),
            );
            assert!(error.contains("stream error 502") && error.contains("queue is full"));
            assert!(!error.contains("Retry-After") && !error.contains("PRIVATE"));
        }
        assert!(!model_output(
            &json!({"choices":[{"delta":{"role":"assistant"}}]})
        ));
        assert!(model_output(
            &json!({"choices":[{"delta":{"tool_calls":[{"index":0}]}}]})
        ));
        assert!(model_output(
            &json!({"choices":[{"delta":{"content":"hi"}}]})
        ));
    }

    #[test]
    fn built_in_provider_has_no_credentials_or_fixed_models_and_preserves_config() {
        let mut config: Config = serde_json::from_value(json!({"model":"mine/custom","providers":[{"id":"mine","baseUrl":"https://example.com/v1","api":"responses","models":[{"id":"custom"}]}]})).unwrap();
        install(&mut config);
        install(&mut config);
        assert_eq!(config.providers.len(), 2);
        assert_eq!(config.model.as_deref(), Some("mine/custom"));
        let provider = &config.providers[1];
        assert_eq!(provider.base_url, URL);
        assert!(provider.models.is_empty() && provider.api_key_env.is_none());
        assert!(super::super::config::access(provider).is_ok());
        assert!(super::super::config::key(provider).is_err());
        let id = SessionId::new().unwrap();
        let request = chat_request(provider, &json!({"model":"discovered"}), &id)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(
            request.headers()["x-rooxcode-version"],
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(request.headers()["x-rooxcode-session"], id.as_str());
        assert!(!request.headers().contains_key("authorization"));
        let mut wrong = provider.clone();
        wrong.authentication = Authentication::ApiKey;
        assert!(validate_provider(&wrong).is_err());
        wrong.authentication = Authentication::Rooxcode;
        wrong.api_key_env = Some("OPENAI_API_KEY".into());
        assert!(validate_provider(&wrong).is_err());
        wrong.api_key_env = None;
        wrong.id = "other".into();
        assert!(validate_provider(&wrong).is_err());
    }

    #[test]
    fn conversation_uuid_is_non_nil_hyphenated_random_and_roundtrips() {
        let id = SessionId::new().unwrap();
        let other = SessionId::new().unwrap();
        assert_ne!(id.as_str(), other.as_str());
        assert_eq!(id.as_str().as_bytes()[14], b'4');
        let value = serde_json::to_value(&id).unwrap();
        assert_eq!(
            serde_json::from_value::<SessionId>(value).unwrap().as_str(),
            id.as_str()
        );
        for invalid in [
            "00000000-0000-0000-0000-000000000000",
            "not-a-uuid",
            "93e3d61dec27447b995ff9c49b3dc962",
            "93e3d61d-ec27-447b-995f-f9c49b3dc96z",
        ] {
            assert!(SessionId::try_from(invalid.to_string()).is_err());
        }
    }

    #[test]
    fn catalog_is_bounded_validated_and_never_invents_capabilities() {
        let models = catalog(br#"{"data":[{"id":"new-alias"},{"id":"operator/model"}]}"#).unwrap();
        assert_eq!(models[1].id, "operator/model");
        assert!(models
            .iter()
            .all(|m| m.context_window.is_none() && !m.images && !m.reasoning));
        let models=catalog(br#"{"data":[{"id":"sized","context_window":131072,"max_request_bytes":10485760,"max_messages":256}]}"#).unwrap();
        assert_eq!(models[0].context_window, Some(131072));
        assert_eq!(models[0].max_request_bytes, Some(10485760));
        assert!(code_message(413, Some("request_too_large")).contains("Compact"));
        for value in [
            json!({"data":[{"id":"x","max_request_bytes":0}]}),
            json!({"data":[{"id":"x","context_window":4096}]}),
            json!({}),
            json!({"data":null}),
            json!({"data":[{"id":"x"},{"id":"x"}]}),
            json!({"data":[{"id":"\u{1b}[SECRET"}]}),
            json!({"data":[{"id":""}]}),
            json!({"data":(0..1001).map(|i|json!({"id":i.to_string()})).collect::<Vec<_>>()}),
        ] {
            let error = catalog(value.to_string().as_bytes())
                .err()
                .unwrap()
                .to_string();
            assert!(!error.contains("SECRET"));
        }
    }

    #[test]
    fn reasoning_metadata_is_typed_and_requires_explicit_capability() {
        let models = catalog(serde_json::to_vec(&json!({"data":[
            {"id":"missing"},
            {"id":"empty","reasoning":true,"reasoning_efforts":[]},
            {"id":"flag-only","reasoning":true},
            {"id":"null","reasoning":null,"reasoning_efforts":null},
            {"id":"no-flag","reasoning_efforts":["high"]},
            {"id":"disabled","reasoning":false,"reasoning_efforts":["high"]},
            {"id":"subset","reasoning":true,"reasoning_efforts":["low","high"]},
            {"id":"all","reasoning":true,"reasoning_efforts":["none","minimal","low","medium","high","xhigh","max"]}
        ]})).unwrap().as_slice()).unwrap();
        for model in &models[..6] {
            assert!(model.reasoning_efforts.is_empty());
        }
        assert_eq!(
            models[6]
                .reasoning_efforts
                .iter()
                .map(|e| e.as_str())
                .collect::<Vec<_>>(),
            ["low", "high"]
        );
        assert_eq!(models[7].reasoning_efforts.len(), 7);
        for metadata in [
            json!({"reasoning":"true"}),
            json!({"reasoning_efforts":"high"}),
            json!({"reasoning_efforts":[1]}),
            json!({"reasoning_efforts":["high","high"]}),
            json!({"reasoning_efforts":["off"]}),
            json!({"reasoning_efforts":["default"]}),
            json!({"reasoning_efforts":["SECRET_UNSUPPORTED"]}),
        ] {
            let mut entry = metadata;
            entry["id"] = json!("invalid");
            let error = catalog(json!({"data":[entry]}).to_string().as_bytes())
                .err()
                .unwrap()
                .to_string();
            assert!(!error.contains("SECRET_UNSUPPORTED"));
        }
    }
}
