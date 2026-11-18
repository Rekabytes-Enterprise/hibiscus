//! The ElevenLabs connector: text to speech for the canvas.
//!
//! API reference: https://elevenlabs.io/docs/api-reference. Every request
//! sends the key in the `xi-api-key` header. The key lives in
//! `connectors.json`, never in the engine's provider config.
use crate::Result;
use base64::Engine as _;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

const API: &str = "https://api.elevenlabs.io";
const KEY: &str = "elevenlabs";
/// Where users create a key.
pub const KEY_PAGE: &str = "https://elevenlabs.io/app/settings/api-keys";
/// The documented default output: MP3, 44.1 kHz, 128 kbps, on every plan.
const FORMAT: &str = "mp3_44100_128";
const MAX_TEXT: usize = 40_000;

/// A model that can do text to speech.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Characters accepted per request, when ElevenLabs says.
    pub max_text: Option<u64>,
}

/// A voice in the user's ElevenLabs library.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub preview_url: Option<String>,
}

/// Generated speech: MP3 bytes as base64, for the app to play and save.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Speech {
    pub mime_type: &'static str,
    pub data: String,
}

fn client(timeout: u64) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(timeout))
        .build()?)
}

fn stored_key() -> Option<String> {
    crate::engine::auth::connector_key(KEY).ok().flatten()
}

/// How the app reaches ElevenLabs: `"key"` with a pasted API key (the REST
/// API), `"oauth"` after "Sign in with ElevenLabs" (the hosted MCP server, as
/// Claude does), or `None`. A key wins when both are set.
pub fn method() -> Option<&'static str> {
    if stored_key().is_some() {
        Some("key")
    } else if crate::engine::elevenlabs_auth::signed_in() {
        Some("oauth")
    } else {
        None
    }
}

pub fn connected() -> bool {
    method().is_some()
}

/// Checks the key against ElevenLabs, then stores it.
pub async fn connect(key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() || key.len() > 8192 || key.chars().any(char::is_control) {
        return Err("Paste an ElevenLabs API key.".into());
    }
    voices_with(key, None).await?;
    crate::engine::auth::save_connector_key(KEY, Some(key.to_owned()))
}

/// Forgets the API key and the sign-in, revoking the sign-in at ElevenLabs.
pub async fn disconnect() -> Result<()> {
    crate::engine::auth::save_connector_key(KEY, None)?;
    crate::engine::elevenlabs_auth::sign_out().await
}

/// ElevenLabs' message from an error body: `{"detail":{"message":…}}`,
/// `{"detail":"…"}` or a 422 list of `{"msg":…}`.
fn error_message(status: reqwest::StatusCode, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("detail").cloned());
    let text = match &detail {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(o)) => o.get("message").and_then(Value::as_str).map(str::to_owned),
        Some(Value::Array(items)) => items
            .iter()
            .find_map(|i| i.get("msg").and_then(Value::as_str))
            .map(str::to_owned),
        _ => None,
    };
    let text = text.unwrap_or_else(|| {
        status
            .canonical_reason()
            .unwrap_or("request failed")
            .to_owned()
    });
    match status.as_u16() {
        401 => format!("ElevenLabs refused the API key: {text}"),
        _ => format!("ElevenLabs: {text} ({})", status.as_u16()),
    }
}

async fn checked(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(error_message(status, &body).into())
}

pub(crate) fn parse_models(body: &Value) -> Vec<Model> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["can_do_text_to_speech"].as_bool() == Some(true))
        .filter_map(|m| {
            Some(Model {
                id: m["model_id"].as_str()?.to_owned(),
                name: m["name"].as_str().unwrap_or_default().to_owned(),
                description: m["description"].as_str().unwrap_or_default().to_owned(),
                max_text: m["maximum_text_length_per_request"].as_u64(),
            })
        })
        .collect()
}

/// The models that can do text to speech.
pub async fn models() -> Result<Vec<Model>> {
    let Some(key) = stored_key() else {
        return match method() {
            Some(_) => Ok(mcp::models()),
            None => Err("Connect ElevenLabs in Settings first.".into()),
        };
    };
    let response = client(30)?
        .get(format!("{API}/v1/models"))
        .header("xi-api-key", key)
        .send()
        .await?;
    Ok(parse_models(&checked(response).await?.json().await?))
}

pub(crate) fn parse_voices(body: &Value) -> Vec<Voice> {
    body["voices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            Some(Voice {
                id: v["voice_id"].as_str()?.to_owned(),
                name: v["name"].as_str().unwrap_or_default().to_owned(),
                category: v["category"].as_str().map(str::to_owned),
                preview_url: v["preview_url"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

async fn voices_with(key: &str, search: Option<&str>) -> Result<Vec<Voice>> {
    let mut query = vec![("page_size", "100")];
    if let Some(search) = search.filter(|s| !s.trim().is_empty()) {
        query.push(("search", search));
    }
    let response = client(30)?
        .get(format!("{API}/v2/voices"))
        .header("xi-api-key", key)
        .query(&query)
        .send()
        .await?;
    Ok(parse_voices(&checked(response).await?.json().await?))
}

/// Up to 100 voices from the user's library, matching `search` when given.
pub async fn voices(search: Option<&str>) -> Result<Vec<Voice>> {
    match stored_key() {
        Some(key) => voices_with(&key, search).await,
        None => mcp::voices(search).await,
    }
}

/// Reads `text` aloud with `voice` and `model`.
pub async fn speak(text: &str, voice: &str, model: &str) -> Result<Speech> {
    if text.trim().is_empty() || text.len() > MAX_TEXT {
        return Err("Write the text to read aloud, up to 40,000 characters.".into());
    }
    // Ids go in the URL path, so only ElevenLabs' id characters are allowed.
    let id = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    if !id(voice) || !id(model) {
        return Err("Choose an ElevenLabs voice and model.".into());
    }
    let Some(key) = stored_key() else {
        return mcp::speak(text, voice, model).await;
    };
    let response = client(300)?
        .post(format!("{API}/v1/text-to-speech/{voice}"))
        .header("xi-api-key", key)
        .query(&[("output_format", FORMAT)])
        .json(&json!({ "text": text, "model_id": model }))
        .send()
        .await?;
    let bytes = checked(response).await?.bytes().await?;
    Ok(Speech {
        mime_type: "audio/mpeg",
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// The hosted MCP server, used after "Sign in with ElevenLabs". It speaks JSON-RPC
/// over Streamable HTTP; speech runs as a flow that is polled until done.
mod mcp {
    use super::{client, parse_voices, Model, Speech, Voice};
    use crate::Result;
    use base64::Engine as _;
    use serde_json::{json, Value};
    use std::time::Duration;

    const URL: &str = crate::engine::elevenlabs_auth::RESOURCE;
    const VERSION: &str = "2025-06-18";

    /// The models `creative_generate_speech` accepts.
    pub(super) fn models() -> Vec<Model> {
        [
            (
                "eleven_v4",
                "Eleven v4",
                "Most expressive, with audio tags such as [whispering]",
            ),
            ("eleven_v3", "Eleven v3", "Expressive, 70+ languages"),
            (
                "eleven_multilingual_v2",
                "Multilingual v2",
                "Lifelike and steady, 29 languages",
            ),
            ("eleven_flash_v2_5", "Flash v2.5", "Fastest and lowest cost"),
            ("eleven_turbo_v2_5", "Turbo v2.5", "Fast, high quality"),
        ]
        .into_iter()
        .map(|(id, name, description)| Model {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            max_text: None,
        })
        .collect()
    }

    struct Session {
        client: reqwest::Client,
        token: String,
        id: Option<String>,
        next: u64,
    }

    /// The JSON-RPC response with `id` from an SSE body: each `data:` line holds a message.
    pub(crate) fn from_events(body: &str, id: u64) -> Option<Value> {
        body.split("\n\n")
            .filter_map(|event| {
                let data: Vec<_> = event
                    .lines()
                    .filter_map(|l| l.strip_prefix("data:"))
                    .map(str::trim_start)
                    .collect();
                serde_json::from_str::<Value>(&data.join("\n")).ok()
            })
            .find(|m| m["id"] == id)
    }

    impl Session {
        async fn open() -> Result<Self> {
            let mut session = Self {
                client: client(120)?,
                token: crate::engine::elevenlabs_auth::access_token().await?,
                id: None,
                next: 0,
            };
            session
                .request(
                    "initialize",
                    json!({"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"Hibiscus","version":env!("CARGO_PKG_VERSION")}}),
                )
                .await?;
            session
                .post(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                .await?;
            Ok(session)
        }

        async fn post(&mut self, body: Value) -> Result<reqwest::Response> {
            let mut request = self
                .client
                .post(URL)
                .bearer_auth(&self.token)
                .header("Accept", "application/json, text/event-stream")
                .header("MCP-Protocol-Version", VERSION)
                .json(&body);
            if let Some(id) = &self.id {
                request = request.header("Mcp-Session-Id", id);
            }
            let response = request.send().await?;
            if let Some(id) = response
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
            {
                self.id = Some(id.to_owned());
            }
            match response.status().as_u16() {
                status @ (401 | 403) => {
                    let challenge = response
                        .headers()
                        .get("www-authenticate")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    let body = response.text().await.unwrap_or_default();
                    Err(refusal(status, &challenge, &body).into())
                }
                s if !(200..300).contains(&s) => {
                    Err(format!("The ElevenLabs server answered HTTP {s}.").into())
                }
                _ => Ok(response),
            }
        }

        async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
            self.next += 1;
            let id = self.next;
            let response = self
                .post(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
                .await?;
            let events = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|t| t.starts_with("text/event-stream"));
            let body = response.text().await?;
            let message = if events {
                from_events(&body, id)
            } else {
                serde_json::from_str(&body).ok()
            }
            .ok_or("The ElevenLabs server sent an unreadable reply.")?;
            if let Some(error) = message.get("error") {
                return Err(format!(
                    "ElevenLabs: {}",
                    error["message"].as_str().unwrap_or("request failed")
                )
                .into());
            }
            Ok(message["result"].clone())
        }

        /// Calls a tool and returns its structured result, or its text parsed as JSON.
        async fn call(&mut self, tool: &str, arguments: Value) -> Result<Value> {
            let result = self
                .request("tools/call", json!({"name":tool,"arguments":arguments}))
                .await?;
            let text = result["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if result["isError"] == true {
                return Err(format!("ElevenLabs: {text}").into());
            }
            Ok(match result.get("structuredContent") {
                Some(v) if !v.is_null() => v.clone(),
                _ => serde_json::from_str(&text).unwrap_or(Value::String(text)),
            })
        }
    }

    /// A refusal the user can act on: 401 means the sign-in itself, 403 a
    /// missing permission, named from the `WWW-Authenticate` challenge when
    /// ElevenLabs sends one (`error="insufficient_scope", scope="…"`).
    pub(crate) fn refusal(status: u16, challenge: &str, body: &str) -> String {
        let field = |name: &str| {
            challenge
                .split(&format!("{name}=\""))
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let detail = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v["detail"]
                    .as_str()
                    .or(v["detail"]["message"].as_str())
                    .or(v["error_description"].as_str())
                    .map(str::to_owned)
            })
            .or_else(|| field("error_description"));
        let scope = field("scope");
        let mut text = if status == 403 || field("error").as_deref() == Some("insufficient_scope") {
            match &scope {
                Some(scope) => format!("ElevenLabs needs the {scope} permission for this. Sign in again in Settings to grant it."),
                None => "ElevenLabs says this sign-in isn't allowed to do that. Sign in again in Settings to grant the missing permission.".to_owned(),
            }
        } else {
            "ElevenLabs no longer accepts this sign-in. Sign in again in Settings.".to_owned()
        };
        match detail {
            Some(detail) => text.push_str(&format!(" ({status}: {detail})")),
            None => text.push_str(&format!(" ({status})")),
        }
        text
    }

    /// Every value under `key`, anywhere in `value`: the tools' replies nest
    /// their fields, and this keeps working if the nesting moves.
    pub(crate) fn find<'a>(value: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    if k == key {
                        out.push(v);
                    }
                    find(v, key, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|v| find(v, key, out)),
            _ => {}
        }
    }

    fn first<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
        let mut out = vec![];
        find(value, key, &mut out);
        out.into_iter().next()
    }

    pub(super) async fn voices(search: Option<&str>) -> Result<Vec<Voice>> {
        let mut session = Session::open().await?;
        let mut args = json!({});
        if let Some(search) = search.filter(|s| !s.trim().is_empty()) {
            args["search"] = json!(search);
        }
        Ok(parse_voices(
            &session.call("creative_list_voices", args).await?,
        ))
    }

    /// The generation ids a finished run reports.
    pub(crate) fn generation_ids(status: &Value) -> Vec<String> {
        let mut out = vec![];
        find(status, "generation_id", &mut out);
        let mut ids: Vec<String> = out
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::to_owned)
            .collect();
        if ids.is_empty() {
            let mut lists = vec![];
            find(status, "generations", &mut lists);
            ids = lists
                .iter()
                .filter_map(|l| l.as_array())
                .flatten()
                .filter_map(|g| g["id"].as_str())
                .map(str::to_owned)
                .collect();
        }
        ids
    }

    /// Starts one generation, polls it for up to five minutes, then downloads the audio.
    pub(super) async fn speak(text: &str, voice: &str, model: &str) -> Result<Speech> {
        let mut session = Session::open().await?;
        let started = session
            .call(
                "creative_generate_speech",
                json!({"prompt":text,"model_id":model,"voice_id":voice,"generations_count":1}),
            )
            .await?;
        let flow = first(&started, "flow_id")
            .and_then(Value::as_str)
            .ok_or("ElevenLabs did not start the speech.")?
            .to_owned();
        let sessions = first(&started, "session_ids").cloned().unwrap_or(json!([]));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
        let status = loop {
            let status = session
                .call(
                    "creative_get_flow_run_status",
                    json!({"flow_id":flow,"session_ids":sessions}),
                )
                .await?;
            if first(&status, "has_failures") == Some(&json!(true)) {
                return Err("ElevenLabs could not make this audio.".into());
            }
            if first(&status, "all_completed") == Some(&json!(true)) {
                break status;
            }
            if tokio::time::Instant::now() > deadline {
                return Err("ElevenLabs is still making the audio after five minutes.".into());
            }
            let wait = first(&status, "poll_after_seconds")
                .and_then(Value::as_f64)
                .unwrap_or(3.0);
            tokio::time::sleep(Duration::from_secs_f64(wait.clamp(1.0, 10.0))).await;
        };
        let ids = generation_ids(&status);
        let id = ids.first().ok_or("ElevenLabs finished without audio.")?;
        let generations = session
            .call(
                "creative_get_generations",
                json!({"flow_id":flow,"generation_ids":[id]}),
            )
            .await?;
        let url = first(&generations, "full_res_url")
            .and_then(Value::as_str)
            .ok_or("ElevenLabs sent no audio link.")?;
        let audio = client(300)?.get(url).send().await?.error_for_status()?;
        let mime = audio
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .filter(|t| t.starts_with("audio/"))
            .unwrap_or("audio/mpeg");
        let mime_type = if mime.contains("wav") {
            "audio/wav"
        } else {
            "audio/mpeg"
        };
        Ok(Speech {
            mime_type,
            data: base64::engine::general_purpose::STANDARD.encode(audio.bytes().await?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_text_to_speech_models() {
        let body = json!([
            {"model_id":"eleven_multilingual_v2","name":"Multilingual v2","description":"d","can_do_text_to_speech":true,"maximum_text_length_per_request":10000},
            {"model_id":"eleven_english_sts_v2","name":"STS","can_do_text_to_speech":false},
            {"name":"no id","can_do_text_to_speech":true}
        ]);
        assert_eq!(
            parse_models(&body),
            [Model {
                id: "eleven_multilingual_v2".into(),
                name: "Multilingual v2".into(),
                description: "d".into(),
                max_text: Some(10000)
            }]
        );
    }

    #[test]
    fn reads_voices() {
        let body = json!({"voices":[{"voice_id":"abc","name":"Rachel","category":"premade","preview_url":null}],"has_more":false});
        assert_eq!(
            parse_voices(&body),
            [Voice {
                id: "abc".into(),
                name: "Rachel".into(),
                category: Some("premade".into()),
                preview_url: None
            }]
        );
    }

    #[test]
    fn explains_errors() {
        let status = reqwest::StatusCode::UNAUTHORIZED;
        assert_eq!(
            error_message(
                status,
                r#"{"detail":{"status":"invalid_api_key","message":"Invalid API key"}}"#
            ),
            "ElevenLabs refused the API key: Invalid API key"
        );
        let status = reqwest::StatusCode::UNPROCESSABLE_ENTITY;
        assert_eq!(
            error_message(
                status,
                r#"{"detail":[{"loc":["body","text"],"msg":"field required","type":"missing"}]}"#
            ),
            "ElevenLabs: field required (422)"
        );
        assert_eq!(
            error_message(status, "oops"),
            "ElevenLabs: Unprocessable Entity (422)"
        );
    }

    #[tokio::test]
    async fn speak_refuses_ids_that_would_change_the_url() {
        for (voice, model) in [("../v1/user", "m"), ("v", "a b"), ("", "m")] {
            assert!(speak("Hello", voice, model).await.is_err());
        }
        assert!(speak("  ", "v", "m").await.is_err());
    }

    #[test]
    fn reads_mcp_replies() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(mcp::from_events(body, 2).unwrap()["result"]["ok"], true);
        assert!(mcp::from_events(body, 3).is_none());
        let status = json!({"all_completed":true,"sessions":[{"generations":[{"id":"g1"}]}]});
        assert_eq!(mcp::generation_ids(&status), ["g1"]);
        let status = json!({"results":[{"generation_id":"g2","status":"completed"}]});
        assert_eq!(mcp::generation_ids(&status), ["g2"]);
    }

    #[test]
    fn says_why_elevenlabs_refused() {
        let scope = r#"Bearer error="insufficient_scope", scope="flows", resource_metadata="https://api.elevenlabs.io/.well-known/oauth-protected-resource""#;
        assert_eq!(
            mcp::refusal(403, scope, ""),
            "ElevenLabs needs the flows permission for this. Sign in again in Settings to grant it. (403)"
        );
        assert_eq!(
            mcp::refusal(401, "Bearer", r#"{"detail":"Token expired"}"#),
            "ElevenLabs no longer accepts this sign-in. Sign in again in Settings. (401: Token expired)"
        );
        assert!(mcp::refusal(403, "", "").contains("missing permission"));
    }
}
