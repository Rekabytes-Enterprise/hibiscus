//! Model provider requests: build the body (wire), send it (http), decode the
//! event stream (sse, stream), and continue chat-completions replies cut at the
//! output limit.
mod http;
mod sse;
mod stream;
mod wire;

use super::event::Event;
use super::{
    config::{Api, Model, Provider},
    session::Message,
};
use crate::Result;
use http::exchange;
use serde_json::{json, Value};
use stream::Stream;
use tokio::sync::mpsc::Sender;
pub(super) use wire::request_body;

pub struct Reply {
    pub message: Message,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache: Option<u64>,
    pub context: Option<u64>,
}
pub async fn emit(out: &Sender<Value>, v: Value) -> Result<()> {
    out.send(v).await.map_err(|_| "UI disconnected".into())
}

pub async fn request(
    provider: &Provider,
    model: &Model,
    conversation: &super::session::Session,
    system: &str,
    thinking: &str,
    tools: Option<&[Value]>,
    out: &Sender<Value>,
) -> Result<Reply> {
    super::rooxcode::validate_provider(provider)?;
    if provider.authentication == super::config::Authentication::Rooxcode
        && !super::thinking::Policy::for_model(provider, model).supports_stored(thinking)
    {
        return Err(
            "Unsupported Rooxcode thinking level. Refresh /models and choose /thinking again."
                .into(),
        );
    }
    let codex = if provider.authentication == super::config::Authentication::Codex {
        Some(super::oauth::credential().await?)
    } else {
        None
    };
    let (route, mut body) = request_body(provider, model, conversation, system, thinking, tools);
    super::privacy::apply(&provider.id, super::privacy::training_denied(), &mut body);
    let url = format!("{}/{}", provider.base_url.trim_end_matches('/'), route);
    let mut continuation = Continuation::default();
    let mut state = loop {
        let mut state = exchange(
            provider,
            conversation,
            &url,
            &body,
            codex.as_ref(),
            tools,
            out,
        )
        .await?;
        let Some(follow_up) = continuation.next(&provider.api, &mut state) else {
            break state;
        };
        body["messages"]
            .as_array_mut()
            .ok_or("Invalid chat request body")?
            .extend(follow_up);
    };
    if state.finish == "length" && state.calls.is_empty() {
        if tools.is_none() {
            return Err(
                "Compaction summary exceeded the provider output limit; history unchanged.".into(),
            );
        }
        state.text.push_str(CUT_NOTICE);
        if tools.is_some() {
            emit(out, Event::Text(CUT_NOTICE).wire()).await?;
        }
    }
    let mut reply = state.reply(&provider.api)?;
    if let Some([input, output, cache]) = continuation.usage {
        reply.input = input;
        reply.output = output;
        reply.cache = cache;
    }
    if let Message::Assistant {
        provider_data: Some(raw),
        ..
    } = &mut reply.message
    {
        raw["provider"] = json!(provider.id);
        raw["model"] = json!(model.id);
    }
    Ok(reply)
}

const CONTINUE_PROMPT: &str = "Continue exactly where you stopped. Do not repeat earlier text.";
const REISSUE_PROMPT: &str = "Your tool call was cut off at the output limit and was not run. Re-issue the complete tool call.";
const CUT_NOTICE: &str = "\n\n[Reply cut at the output limit.]";
const MAX_CONTINUATIONS: u8 = 3;

/// Chat-completions replies stopped by "length" are continued in follow-up requests.
#[derive(Default)]
struct Continuation {
    text: String,
    rounds: u8,
    reissued: bool,
    usage: Option<[Option<u64>; 3]>,
}
impl Continuation {
    /// Messages to append for another request, or None once `state` is the final round,
    /// which then carries the text of every earlier round.
    fn next(&mut self, api: &Api, state: &mut Stream) -> Option<Vec<Value>> {
        if *api == Api::ChatCompletions {
            let current = [state.input, state.output, state.cache];
            self.usage = Some(match self.usage {
                None => current,
                Some(previous) => std::array::from_fn(|index| {
                    previous[index]
                        .zip(current[index])
                        .map(|(before, next)| before.saturating_add(next))
                }),
            });
        }
        let tool = !state.calls.is_empty();
        if *api != Api::ChatCompletions
            || state.finish != "length"
            || self.rounds >= MAX_CONTINUATIONS
            || (tool && self.reissued)
        {
            state.text.insert_str(0, &std::mem::take(&mut self.text));
            return None;
        }
        self.rounds += 1;
        self.reissued |= tool;
        // Partial tool arguments are never executed or replayed.
        state.calls.clear();
        let partial = std::mem::take(&mut state.text);
        self.text.push_str(&partial);
        let mut wire = vec![];
        if !partial.is_empty() {
            wire.push(json!({"role":"assistant","content":partial}));
        }
        wire.push(
            json!({"role":"user","content":if tool { REISSUE_PROMPT } else { CONTINUE_PROMPT }}),
        );
        Some(wire)
    }
}

#[cfg(feature = "fuzzing")]
pub(super) fn fuzz_stream(bytes: &[u8]) {
    use sse::Decoder;
    let Some((&selection, payload)) = bytes.split_first() else {
        return;
    };
    fn frames(bytes: &[u8], chunk: usize) -> Result<Vec<String>> {
        let mut decoder = Decoder::default();
        let mut frames = Vec::new();
        for bytes in bytes.chunks(chunk.max(1)) {
            frames.extend(decoder.push(bytes)?);
        }
        Ok(frames)
    }
    if let Ok(whole) = frames(payload, payload.len()) {
        let chunks = frames(payload, usize::from(selection) + 1)
            .expect("SSE decoding depends on network chunk size");
        assert_eq!(whole, chunks, "SSE frame boundaries changed across chunks");
    }
    let api = match selection % 3 {
        0 => Api::Responses,
        1 => Api::Anthropic,
        _ => Api::ChatCompletions,
    };
    let events: Vec<Value> = match serde_json::from_slice(payload) {
        Ok(Value::Array(values)) => values.into_iter().take(64).collect(),
        Ok(value) => vec![value],
        Err(_) => frames(payload, 31)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|frame| serde_json::from_str(&frame).ok())
            .take(64)
            .collect(),
    };
    let mut stream = Stream::default();
    for event in events {
        if stream.observe(&api, &event).is_err() {
            return;
        }
        if stream.done {
            let _ = stream.reply(&api);
            return;
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod parser_fuzz_regressions;
