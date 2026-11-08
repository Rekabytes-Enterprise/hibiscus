//! Sends one request with the right credentials and reads its event stream.
use super::{
    emit,
    sse::{Decoder, Frame},
    stream::Stream,
};
use crate::engine::{
    config::{Api, Provider},
    event::Event,
};
use crate::Result;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

/// Sends one request and streams it to its completion event.
pub(super) async fn exchange(
    provider: &Provider,
    conversation: &crate::engine::session::Session,
    url: &str,
    body: &Value,
    codex: Option<&crate::engine::oauth::Token>,
    tools: Option<&[Value]>,
    out: &Sender<Value>,
) -> Result<Stream> {
    let anonymous = provider.authentication == crate::engine::config::Authentication::Rooxcode;
    let req = if anonymous {
        crate::engine::rooxcode::chat_request(provider, body, &conversation.rooxcode_session)?
    } else {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600))
            .build()?;
        let key = if let Some(token) = codex {
            token.access.clone()
        } else {
            crate::engine::config::key(provider)?
        };
        authenticated_request(&client, url, body.clone(), &key, codex)?
    };
    let req = if provider.api == Api::Anthropic {
        req.header("anthropic-version", "2023-06-01")
    } else {
        req
    };
    let mut response = req.send().await.map_err(|_| {
        if anonymous { "Rooxcode network request failed. Check service connectivity; nothing was retried automatically." } else { "Provider network request failed. Check connectivity and endpoint configuration." }
    })?;
    if !response.status().is_success() {
        if anonymous {
            return Err(crate::engine::rooxcode::response_error(response)
                .await
                .into());
        }
        return Err(format!("Provider returned HTTP {}. Check authentication, model access, quota and endpoint configuration.",response.status().as_u16()).into());
    }
    let mut decoder = Decoder::default();
    let mut state = Stream::default();
    let mut capacity = Capacity::Pending;
    'stream: while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Provider response stream failed; partial output was not committed.")?
    {
        for frame in decoder.push_frames(&chunk)? {
            let frame = match frame {
                Frame::Comment(comment) => {
                    if anonymous && comment == "rooxcode waiting" && capacity == Capacity::Pending {
                        capacity = Capacity::Waiting;
                        emit(out, Event::ProviderWaiting(true).wire()).await?;
                    }
                    continue;
                }
                Frame::Data(data) => data,
            };
            if frame == "[DONE]" {
                state.done = true;
                break 'stream;
            }
            let event: Value =
                serde_json::from_str(&frame).map_err(|_| "Malformed provider streaming event")?;
            if anonymous && event["error"].is_object() {
                return Err(crate::engine::rooxcode::stream_error(&event).into());
            }
            let delta = state.observe(&provider.api, &event)?;
            if anonymous
                && capacity != Capacity::Started
                && crate::engine::rooxcode::model_output(&event)
            {
                if capacity == Capacity::Waiting {
                    emit(out, Event::ProviderWaiting(false).wire()).await?;
                }
                capacity = Capacity::Started;
            }
            if let Some(delta) = delta.filter(|_| tools.is_some()) {
                emit(out, Event::Text(&delta).wire()).await?;
            }
            if state.done {
                break 'stream;
            }
        }
    }
    if !state.done {
        return Err(
            "Provider stream ended before its completion event; no tools were executed.".into(),
        );
    }
    Ok(state)
}

#[derive(PartialEq)]
enum Capacity {
    Pending,
    Waiting,
    Started,
}

pub(super) fn authenticated_request(
    client: &reqwest::Client,
    url: &str,
    mut body: Value,
    key: &str,
    codex: Option<&crate::engine::oauth::Token>,
) -> Result<reqwest::RequestBuilder> {
    let Some(token) = codex else {
        return Ok(client.post(url).bearer_auth(key).json(&body));
    };
    if url != format!("{}/responses", crate::engine::oauth::RESOURCE) {
        return Err(
            "Refusing to send Codex subscription credentials outside the Codex endpoint".into(),
        );
    }
    // Codex's legacy subscription endpoint does not accept this API field.
    body.as_object_mut()
        .ok_or("Invalid Responses request body")?
        .remove("max_output_tokens");
    Ok(client
        .post(url)
        .bearer_auth(&token.access)
        .json(&body)
        .header("chatgpt-account-id", &token.account_id)
        .header("originator", "hibiscus")
        .header("OpenAI-Beta", "responses=experimental")
        .header("accept", "text/event-stream"))
}
