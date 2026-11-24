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
    tools: Option<&[Value]>,
    out: &Sender<Value>,
) -> Result<Stream> {
    use crate::engine::{chatgpt, config::Authentication};
    let anonymous = provider.authentication == Authentication::Rooxcode;
    let chatgpt_plan = provider.authentication == Authentication::ChatGptPlan;
    let response = if anonymous {
        crate::engine::rooxcode::send_chat(provider, body, &conversation.rooxcode_session).await?
    } else {
        let mut client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600));
        if provider.discover_models {
            client = client.no_proxy();
        }
        let client = client.build()?;
        let req = if provider.authentication == crate::engine::config::Authentication::Local {
            client.post(url).json(body)
        } else if chatgpt_plan {
            chatgpt_request(&client, url, body, &chatgpt::credential().await?)?
        } else {
            let key = crate::engine::config::key(provider)?;
            client.post(url).bearer_auth(key).json(body)
        };
        let req = if provider.api == Api::Anthropic {
            req.header("anthropic-version", "2023-06-01")
        } else {
            req
        };
        let response = req.send().await.map_err(|_| {
            "Provider network request failed. Check connectivity and endpoint configuration."
        })?;
        let status = response.status();
        if !status.is_success() {
            if chatgpt_plan {
                return Err(chatgpt::response_error(response).await);
            }
            if matches!(status.as_u16(), 400 | 413 | 422)
                && tokio::time::timeout(Duration::from_secs(2), context_overflow(response))
                    .await
                    .unwrap_or(false)
            {
                return Err(super::ContextOverflow.into());
            }
            return Err(format!("Provider returned HTTP {}. Check authentication, model access, quota and endpoint configuration.",status.as_u16()).into());
        }
        response
    };
    stream_response(response, provider, tools, out).await
}

pub(super) async fn stream_response(
    mut response: reqwest::Response,
    provider: &Provider,
    tools: Option<&[Value]>,
    out: &Sender<Value>,
) -> Result<Stream> {
    use crate::engine::{chatgpt, config::Authentication};
    let anonymous = provider.authentication == Authentication::Rooxcode;
    let chatgpt_plan = provider.authentication == Authentication::ChatGptPlan;
    let mut decoder = Decoder::default();
    let mut state = Stream::default();
    let mut capacity = Capacity::Pending;
    let mut chat_output = false;
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
                if !chatgpt_plan {
                    state.done = true;
                }
                break 'stream;
            }
            let event: Value =
                serde_json::from_str(&frame).map_err(|_| "Malformed provider streaming event")?;
            if anonymous && event["error"].is_object() {
                return Err(crate::engine::rooxcode::stream_error(&event));
            }
            if !anonymous && provider.api == Api::ChatCompletions {
                // A local server can open the stream with a loading keepalive
                // and reject the prompt before generating any model output.
                if !chat_output && context_error(&event["error"]) {
                    return Err(super::ContextOverflow.into());
                }
                chat_output |= event["choices"].as_array().is_some_and(|choices| {
                    choices.iter().any(|choice| {
                        let delta = &choice["delta"];
                        delta.as_object().is_some_and(|fields| {
                            fields.iter().any(|(key, value)| {
                                key != "role" && !value.is_null() && value.as_str() != Some("")
                            })
                        }) || choice["finish_reason"].is_string()
                    })
                });
            }
            if chatgpt_plan
                && (event["error"].is_object()
                    || matches!(event["type"].as_str(), Some("response.failed" | "error")))
            {
                let code = event["response"]["error"]["code"]
                    .as_str()
                    .or_else(|| event["code"].as_str())
                    .or_else(|| event["error"]["code"].as_str());
                return Err(chatgpt::request_error(reqwest::StatusCode::BAD_REQUEST, code).into());
            }
            if chatgpt_plan && event["type"] == "response.done" {
                continue;
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

/// Classify only explicit pre-stream rejections. Do not reveal the response body.
async fn context_overflow(mut response: reqwest::Response) -> bool {
    const LIMIT: usize = 8192;
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= LIMIT => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(None) => break,
            _ => return false,
        }
    }
    let Ok(body) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    context_error(&body["error"])
}

fn context_error(error: &Value) -> bool {
    matches!(
        error["code"].as_str(),
        Some("context_length_exceeded" | "context_window_exceeded")
    ) || error["message"]
        .as_str()
        .is_some_and(|message| message.starts_with("context_length_exceeded:"))
}

#[derive(PartialEq)]
enum Capacity {
    Pending,
    Waiting,
    Started,
}

pub(super) fn chatgpt_request(
    client: &reqwest::Client,
    url: &str,
    body: &Value,
    token: &str,
) -> Result<reqwest::RequestBuilder> {
    if url != format!("{}/responses", crate::engine::chatgpt::RESOURCE) {
        return Err(
            "Refusing to send ChatGPT plan credentials outside the OpenAI Responses endpoint."
                .into(),
        );
    }
    Ok(client.post(url).bearer_auth(token).json(body))
}
