use super::event::Event;
use super::{
    config::{Api, Model, Provider},
    session::{Block, Message},
};
use crate::Result;
use serde_json::{json, Value};
use std::{collections::BTreeMap, time::Duration};
use tokio::sync::mpsc::Sender;

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
fn text_blocks(blocks: &[Block], api: &Api) -> Vec<Value> {
    blocks.iter().filter_map(|block|match block {
    Block::Text{text}=>Some(match api{Api::Responses=>json!({"type":"input_text","text":text}),_=>json!({"type":"text","text":text})}),
    Block::Image{mime_type,data}=>Some(match api{Api::Anthropic=>json!({"type":"image","source":{"type":"base64","media_type":mime_type,"data":data}}),Api::Responses=>json!({"type":"input_image","image_url":format!("data:{mime_type};base64,{data}")}),Api::ChatCompletions=>json!({"type":"image_url","image_url":{"url":format!("data:{mime_type};base64,{data}")}})}),_=>None}).collect()
}
fn wire_messages(messages: &[Message], provider: &Provider, model: &Model) -> Vec<Value> {
    let api = &provider.api;
    let mut wire = vec![];
    for m in messages {
        match (api, m) {
            (Api::Responses, Message::User { content }) => {
                wire.push(json!({"role":"user","content":text_blocks(content,api)}))
            }
            (
                Api::Responses,
                Message::Assistant {
                    content,
                    provider_data,
                    ..
                },
            ) => {
                if let Some(raw) = provider_data
                    .as_ref()
                    .filter(|r| {
                        r["api"] == "responses"
                            && r["provider"] == provider.id
                            && r["model"] == model.id
                    })
                    .and_then(|r| r["items"].as_array())
                {
                    wire.extend(raw.clone());
                    continue;
                }
                for b in content {
                    match b{Block::Text{text}=>wire.push(json!({"role":"assistant","content":[{"type":"output_text","text":text}]})),Block::ToolCall{id,name,arguments}=>wire.push(json!({"type":"function_call","call_id":id,"name":name,"arguments":arguments.to_string()})),_=>{}}
                }
            }
            (Api::Responses, Message::ToolResult { tool_call_id, .. }) => wire.push(
                json!({"type":"function_call_output","call_id":tool_call_id,"output":m.text()}),
            ),
            (Api::Anthropic, Message::User { content }) => {
                wire.push(json!({"role":"user","content":text_blocks(content,api)}))
            }
            (
                Api::Anthropic,
                Message::Assistant {
                    content,
                    provider_data,
                    ..
                },
            ) => {
                let blocks=provider_data.as_ref().filter(|r|r["api"]=="anthropic").and_then(|r|r["items"].as_array()).cloned().unwrap_or_else(||content.iter().filter_map(|b|match b{Block::Text{text}=>Some(json!({"type":"text","text":text})),Block::ToolCall{id,name,arguments}=>Some(json!({"type":"tool_use","id":id,"name":name,"input":arguments})),_=>None}).collect());
                wire.push(json!({"role":"assistant","content":blocks}));
            }
            (
                Api::Anthropic,
                Message::ToolResult {
                    tool_call_id,
                    is_error,
                    ..
                },
            ) => {
                let result = json!({"type":"tool_result","tool_use_id":tool_call_id,"content":m.text(),"is_error":is_error});
                if let Some(last) = wire.last_mut().filter(|v| v["role"] == "user") {
                    last["content"].as_array_mut().unwrap().push(result);
                } else {
                    wire.push(json!({"role":"user","content":[result]}));
                }
            }
            (Api::ChatCompletions, Message::User { content }) => {
                wire.push(json!({"role":"user","content":text_blocks(content,api)}))
            }
            (
                Api::ChatCompletions,
                Message::Assistant {
                    content,
                    provider_data,
                    ..
                },
            ) => {
                if let Some(raw) = provider_data.as_ref().filter(|r| {
                    r["api"] == "chat_completions"
                        && r["provider"] == provider.id
                        && r["model"] == model.id
                }) {
                    wire.push(raw["message"].clone());
                    continue;
                }
                let calls:Vec<Value>=content.iter().filter_map(|b|if let Block::ToolCall{id,name,arguments}=b{Some(json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}))}else{None}).collect();
                let mut v = json!({"role":"assistant","content":m.text()});
                if !calls.is_empty() {
                    v["tool_calls"] = json!(calls);
                }
                wire.push(v);
            }
            (Api::ChatCompletions, Message::ToolResult { tool_call_id, .. }) => {
                wire.push(json!({"role":"tool","tool_call_id":tool_call_id,"content":m.text()}))
            }
        }
    }
    wire
}
pub(super) fn request_body(
    provider: &Provider,
    model: &Model,
    conversation: &super::session::Session,
    system: &str,
    thinking: &str,
    tools: Option<&[Value]>,
) -> (&'static str, Value) {
    let anonymous = provider.authentication == super::config::Authentication::Rooxcode;
    let definitions = tools.unwrap_or_default();
    let mut wire = wire_messages(&conversation.messages, provider, model);
    let (route, mut body) = match provider.api {
        Api::Responses => (
            "responses",
            json!({"model":model.id,"input":wire,"instructions":system,"stream":true,"store":false,"max_output_tokens":model.max_tokens,"tools":definitions.iter().map(|v|json!({"type":"function","name":v["name"],"description":v["description"],"parameters":v["parameters"],"strict":false})).collect::<Vec<_>>(),"include":["reasoning.encrypted_content"]}),
        ),
        Api::Anthropic => (
            "messages",
            json!({"model":model.id,"messages":wire,"system":system,"stream":true,"max_tokens":model.max_tokens,"tools":definitions.iter().map(|v|json!({"name":v["name"],"description":v["description"],"input_schema":v["parameters"]})).collect::<Vec<_>>()}),
        ),
        Api::ChatCompletions => {
            wire.insert(0, json!({"role":"system","content":system}));
            (
                "chat/completions",
                json!({"model":model.id,"messages":wire,"stream":true,"stream_options":{"include_usage":true},"max_tokens":model.max_tokens,"tools":definitions.iter().map(|v|json!({"type":"function","function":v})).collect::<Vec<_>>()}),
            )
        }
    };
    if thinking != "off" && model.reasoning && provider.api == Api::Responses {
        body["reasoning"] = json!({"effort":thinking});
    }
    if anonymous {
        if let Some(effort) =
            super::thinking::Policy::for_model(provider, model).rooxcode_effort(thinking)
        {
            body["reasoning_effort"] = json!(effort);
        }
        // Let the gateway apply its advertised default output limit.
        // The client reserves output space without overriding that default.
        body.as_object_mut().unwrap().remove("max_tokens");
    }
    if tools.is_none() {
        body.as_object_mut().unwrap().remove("tools");
    }
    (route, body)
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

/// Sends one request and streams it to its completion event.
async fn exchange(
    provider: &Provider,
    conversation: &super::session::Session,
    url: &str,
    body: &Value,
    codex: Option<&super::oauth::Token>,
    tools: Option<&[Value]>,
    out: &Sender<Value>,
) -> Result<Stream> {
    let anonymous = provider.authentication == super::config::Authentication::Rooxcode;
    let req = if anonymous {
        super::rooxcode::chat_request(provider, body, &conversation.rooxcode_session)?
    } else {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600))
            .build()?;
        let key = if let Some(token) = codex {
            token.access.clone()
        } else {
            super::config::key(provider)?
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
            return Err(super::rooxcode::response_error(response).await.into());
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
                return Err(super::rooxcode::stream_error(&event).into());
            }
            let delta = state.observe(&provider.api, &event)?;
            if anonymous && capacity != Capacity::Started && super::rooxcode::model_output(&event) {
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

#[derive(Debug, PartialEq)]
enum Frame {
    Data(String),
    Comment(String),
}

fn authenticated_request(
    client: &reqwest::Client,
    url: &str,
    mut body: Value,
    key: &str,
    codex: Option<&super::oauth::Token>,
) -> Result<reqwest::RequestBuilder> {
    let Some(token) = codex else {
        return Ok(client.post(url).bearer_auth(key).json(&body));
    };
    if url != format!("{}/responses", super::oauth::RESOURCE) {
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
#[derive(Default)]
struct Decoder {
    buffer: Vec<u8>,
}
impl Decoder {
    #[cfg(any(test, feature = "fuzzing"))]
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        Ok(self
            .push_frames(bytes)?
            .into_iter()
            .filter_map(|frame| match frame {
                Frame::Data(data) => Some(data),
                Frame::Comment(_) => None,
            })
            .collect())
    }
    fn push_frames(&mut self, bytes: &[u8]) -> Result<Vec<Frame>> {
        if bytes.len() > (2 * 1024 * 1024_usize).saturating_sub(self.buffer.len()) {
            return Err("Provider streaming frame exceeds 2 MiB".into());
        }
        self.buffer.extend_from_slice(bytes);
        let mut frames = vec![];
        let mut consumed = 0;
        loop {
            let remaining = &self.buffer[consumed..];
            // Pick the first delimiter on the wire, regardless of line endings.
            let marker = remaining.windows(2).enumerate().find_map(|(i, pair)| {
                if pair == b"\n\n" {
                    Some((i, 2))
                } else if pair == b"\r\n" && remaining.get(i..i + 4) == Some(b"\r\n\r\n") {
                    Some((i, 4))
                } else {
                    None
                }
            });
            let Some((end, skip)) = marker else { break };
            let frame = std::str::from_utf8(&remaining[..end])?;
            for comment in frame.lines().filter_map(|line| line.strip_prefix(':')) {
                frames.push(Frame::Comment(comment.trim_start().to_owned()));
            }
            let data = frame
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("data:")
                        .map(|s| s.strip_prefix(' ').unwrap_or(s))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !data.is_empty() {
                frames.push(Frame::Data(data));
            }
            consumed += end + skip;
        }
        // Drain once rather than repeatedly shifting every remaining frame.
        self.buffer.drain(..consumed);
        Ok(frames)
    }
}
#[derive(Default)]
struct Stream {
    text: String,
    calls: BTreeMap<u64, (String, String, String)>,
    blocks: BTreeMap<u64, Value>,
    raw: Vec<Value>,
    input: Option<u64>,
    output: Option<u64>,
    cache: Option<u64>,
    cache_created: Option<u64>,
    done: bool,
    finish: String,
}
impl Stream {
    fn observe(&mut self, api: &Api, v: &Value) -> Result<Option<String>> {
        if v["error"].is_object()
            || v["type"] == "error"
            || v["type"] == "response.failed"
            || v["type"] == "response.incomplete"
        {
            return Err("Provider reported a failed or incomplete response. Check account access or retry explicitly.".into());
        }
        let mut delta = None;
        match api {
            Api::Responses => match v["type"].as_str() {
                Some("response.output_text.delta") => {
                    delta = v["delta"].as_str().map(str::to_owned)
                }
                Some("response.completed" | "response.done") => {
                    if v["response"]["status"] != "completed" {
                        return Err("Provider response did not complete".into());
                    }
                    self.done = true;
                    self.raw = v["response"]["output"]
                        .as_array()
                        .cloned()
                        .ok_or("Missing response output")?;
                    self.input = v["response"]["usage"]["input_tokens"].as_u64();
                    self.output = v["response"]["usage"]["output_tokens"].as_u64();
                    self.cache =
                        v["response"]["usage"]["input_tokens_details"]["cached_tokens"].as_u64();
                }
                _ => {}
            },
            Api::Anthropic => match v["type"].as_str() {
                Some("message_start") => {
                    self.input = v["message"]["usage"]["input_tokens"].as_u64();
                    self.cache = v["message"]["usage"]["cache_read_input_tokens"].as_u64();
                    self.cache_created =
                        v["message"]["usage"]["cache_creation_input_tokens"].as_u64();
                }
                Some("content_block_start") => {
                    let i = v["index"].as_u64().ok_or("Invalid content block index")?;
                    let block = v["content_block"].clone();
                    block
                        .get("type")
                        .and_then(Value::as_str)
                        .ok_or("Invalid content block")?;
                    if block["type"] == "tool_use" {
                        self.calls.insert(
                            i,
                            (
                                block["id"].as_str().ok_or("Missing tool ID")?.into(),
                                block["name"].as_str().ok_or("Missing tool name")?.into(),
                                String::new(),
                            ),
                        );
                    }
                    self.blocks.insert(i, block);
                }
                Some("content_block_delta") => {
                    let i = v["index"].as_u64().ok_or("Invalid content delta index")?;
                    let d = &v["delta"];
                    match d["type"].as_str() {
                        Some("text_delta") => {
                            let t = d["text"].as_str().ok_or("Invalid text delta")?;
                            delta = Some(t.into());
                            let b = self.blocks.get_mut(&i).ok_or("Unknown content block")?;
                            if b["type"] != "text" {
                                return Err("Text delta does not match a text block".into());
                            }
                            let previous = b["text"].as_str().ok_or("Invalid text block")?;
                            b["text"] = json!(format!("{previous}{t}"));
                        }
                        Some("input_json_delta") => {
                            self.calls
                                .get_mut(&i)
                                .ok_or("Unknown tool call")?
                                .2
                                .push_str(
                                    d["partial_json"]
                                        .as_str()
                                        .ok_or("Invalid arguments delta")?,
                                );
                        }
                        _ => {}
                    }
                }
                Some("message_delta") => {
                    if let Some(n) = v["usage"]["input_tokens"].as_u64() {
                        self.input = Some(n);
                    }
                    if let Some(n) = v["usage"]["cache_read_input_tokens"].as_u64() {
                        self.cache = Some(n);
                    }
                    if let Some(n) = v["usage"]["cache_creation_input_tokens"].as_u64() {
                        self.cache_created = Some(n);
                    }
                    self.finish = v["delta"]["stop_reason"].as_str().unwrap_or("").into();
                    self.output = v["usage"]["output_tokens"].as_u64();
                }
                Some("message_stop") => {
                    if !matches!(
                        self.finish.as_str(),
                        "end_turn" | "tool_use" | "stop_sequence"
                    ) {
                        return Err("Provider stopped before completing the response".into());
                    }
                    self.done = true;
                }
                _ => {}
            },
            Api::ChatCompletions => {
                if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
                    self.input = u["prompt_tokens"].as_u64();
                    self.output = u["completion_tokens"].as_u64();
                    self.cache = u["prompt_tokens_details"]["cached_tokens"].as_u64();
                }
                if let Some(c) = v["choices"].as_array().and_then(|c| c.first()) {
                    delta = c["delta"]["content"].as_str().map(str::to_owned);
                    if let Some(reason) = c["finish_reason"].as_str() {
                        self.finish = reason.into();
                    }
                    if let Some(calls) = c["delta"]["tool_calls"].as_array() {
                        for call in calls {
                            let i = call["index"].as_u64().ok_or("Invalid tool call index")?;
                            let entry = self.calls.entry(i).or_default();
                            if let Some(id) = call["id"].as_str() {
                                entry.0.push_str(id);
                            }
                            if let Some(name) = call["function"]["name"].as_str() {
                                entry.1.push_str(name);
                            }
                            if let Some(args) = call["function"]["arguments"].as_str() {
                                entry.2.push_str(args);
                            }
                        }
                    }
                }
            }
        }
        if let Some(t) = &delta {
            self.text.push_str(t);
            if self.text.len() > 8 * 1024 * 1024 {
                return Err("Response exceeds 8 MiB".into());
            }
        }
        if self.calls.values().any(|c| c.2.len() > 1024 * 1024) || self.calls.len() > 128 {
            return Err("Tool-call response exceeds bounds".into());
        }
        Ok(delta)
    }
    fn reply(mut self, api: &Api) -> Result<Reply> {
        let mut blocks = vec![];
        let raw = match api {
            Api::Responses => {
                for item in &self.raw {
                    match item["type"].as_str() {
                        Some("function_call") => blocks.push(Block::ToolCall {
                            id: item["call_id"].as_str().ok_or("Missing tool ID")?.into(),
                            name: item["name"].as_str().ok_or("Missing tool name")?.into(),
                            arguments: serde_json::from_str(
                                item["arguments"].as_str().ok_or("Missing tool arguments")?,
                            )?,
                        }),
                        Some("message") => {
                            for b in item["content"].as_array().ok_or("Invalid output content")? {
                                if b["type"] == "output_text" {
                                    blocks.push(Block::Text {
                                        text: b["text"]
                                            .as_str()
                                            .ok_or("Invalid output text")?
                                            .into(),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
                json!({"api":"responses","items":self.raw})
            }
            Api::Anthropic => {
                for (i, (id, name, args)) in &self.calls {
                    let block = self.blocks.get_mut(i).ok_or("Missing tool block")?;
                    let arguments = if args.is_empty() {
                        block["input"].clone()
                    } else {
                        serde_json::from_str(args)?
                    };
                    block["input"] = arguments.clone();
                    blocks.push(Block::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments,
                    });
                }
                if !self.text.is_empty() {
                    blocks.insert(
                        0,
                        Block::Text {
                            text: self.text.clone(),
                        },
                    );
                }
                json!({"api":"anthropic","items":self.blocks.into_values().collect::<Vec<_>>()})
            }
            Api::ChatCompletions => {
                match self.finish.as_str() {
                    "stop" | "tool_calls" => {}
                    // Text-only replies reach here after the continuation cap.
                    "length" if self.calls.is_empty() => {}
                    "length" => return Err("A tool call exceeded the provider output limit and was not executed.".into()),
                    "content_filter" => return Err("Provider blocked the response under its content policy; nothing was committed.".into()),
                    _ => return Err("Provider response was truncated or incomplete".into()),
                }
                if !self.text.is_empty() {
                    blocks.push(Block::Text {
                        text: self.text.clone(),
                    });
                }
                let mut calls = vec![];
                for (_, (id, name, args)) in self.calls {
                    let arguments: Value = serde_json::from_str(&args)?;
                    calls.push(json!({"id":id,"type":"function","function":{"name":name,"arguments":args}}));
                    blocks.push(Block::ToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
                let mut message = json!({"role":"assistant","content":self.text});
                if !calls.is_empty() {
                    message["tool_calls"] = json!(calls);
                }
                json!({"api":"chat_completions","message":message})
            }
        };
        let mut ids = std::collections::HashSet::new();
        for b in &blocks {
            if let Block::ToolCall {
                id,
                name,
                arguments,
            } = b
            {
                if id.is_empty() || name.is_empty() || !arguments.is_object() || !ids.insert(id) {
                    return Err("Invalid or duplicate tool call".into());
                }
            }
        }
        Ok(Reply {
            message: Message::Assistant {
                content: blocks,
                stop_reason: "stop".into(),
                provider_data: Some(raw),
            },
            input: self.input,
            output: self.output,
            cache: self.cache,
            context: self.input.zip(self.output).map(|(input, output)| {
                let cached = if *api == Api::Anthropic {
                    self.cache
                        .unwrap_or(0)
                        .saturating_add(self.cache_created.unwrap_or(0))
                } else {
                    0
                };
                input.saturating_add(cached).saturating_add(output)
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_done_requires_completed_status_before_accepting_tools() {
        for status in ["incomplete", "failed", "in_progress", "completed"] {
            let mut stream = Stream::default();
            let event = json!({"type":"response.done","response":{"status":status,"output":[
                {"type":"function_call","call_id":"call","name":"write","arguments":"{\"path\":\"file\",\"content\":\"text\"}"}
            ]}});
            let result = stream.observe(&Api::Responses, &event);
            if status == "completed" {
                result.unwrap();
                assert!(stream.done);
                let reply = stream.reply(&Api::Responses).unwrap();
                assert!(
                    matches!(reply.message, Message::Assistant { content, .. } if matches!(content.first(),Some(Block::ToolCall{..})))
                );
            } else {
                assert!(result.is_err());
                assert!(!stream.done);
            }
        }
    }
    #[test]
    fn codex_credentials_are_pinned_to_subscription_transport() {
        let client = reqwest::Client::new();
        let token = super::super::oauth::Token {
            access: "FAKE-ACCESS".into(),
            refresh: "FAKE-REFRESH".into(),
            expires: 1,
            account_id: "FAKE-ACCOUNT".into(),
        };
        let body = json!({"model":"test","store":false,"stream":true,"max_output_tokens":4096,
            "input":[{"role":"user","content":"hello"}]});
        for url in [
            "https://evil.example/responses",
            "https://api.openai.com/v1/responses",
            "https://chatgpt.com/backend-api/codex/responses?redirect=evil",
        ] {
            assert!(
                authenticated_request(&client, url, body.clone(), "OTHER-KEY", Some(&token))
                    .is_err()
            );
        }
        let request = authenticated_request(
            &client,
            &format!("{}/responses", super::super::oauth::RESOURCE),
            body,
            "OTHER-KEY",
            Some(&token),
        )
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(request.headers()["authorization"], "Bearer FAKE-ACCESS");
        assert_eq!(request.headers()["chatgpt-account-id"], "FAKE-ACCOUNT");
        assert_eq!(request.headers()["originator"], "hibiscus");
        assert_eq!(request.headers()["accept"], "text/event-stream");
        let body: Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert!(body.get("max_output_tokens").is_none());
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert!(!body.to_string().contains("FAKE-REFRESH"));
    }
    #[test]
    fn anthropic_context_includes_cached_input_and_final_usage() {
        let mut stream = Stream::default();
        stream.observe(&Api::Anthropic,&json!({"type":"message_start","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":30}}})).unwrap();
        stream.observe(&Api::Anthropic,&json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":15,"output_tokens":5}})).unwrap();
        stream
            .observe(&Api::Anthropic, &json!({"type":"message_stop"}))
            .unwrap();
        let reply = stream.reply(&Api::Anthropic).unwrap();
        assert_eq!(reply.input, Some(15));
        assert_eq!(reply.cache, Some(20));
        assert_eq!(reply.context, Some(70));
    }

    fn chat_round(events: &[Value]) -> Stream {
        let mut stream = Stream::default();
        for event in events {
            stream.observe(&Api::ChatCompletions, event).unwrap();
        }
        stream
    }
    fn text(content: &str, finish: Option<&str>) -> Value {
        json!({"choices":[{"delta":{"content":content},"finish_reason":finish}]})
    }
    fn call(id: &str, args: &str, finish: Option<&str>) -> Value {
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":id,"function":{"name":"write","arguments":args}}]},"finish_reason":finish}]})
    }
    fn reply_text(reply: &Reply) -> String {
        reply.message.text()
    }

    #[test]
    fn continuation_usage_sums_rounds_without_turning_missing_values_into_zero() {
        for missing in [false, true] {
            let mut continuation = Continuation::default();
            let mut first = Stream {
                finish: "length".into(),
                input: Some(u64::MAX),
                output: Some(2),
                cache: (!missing).then_some(1),
                ..Stream::default()
            };
            assert!(continuation
                .next(&Api::ChatCompletions, &mut first)
                .is_some());
            let mut last = Stream {
                finish: "stop".into(),
                input: Some(15),
                output: Some(3),
                cache: Some(2),
                ..Stream::default()
            };
            assert!(continuation
                .next(&Api::ChatCompletions, &mut last)
                .is_none());
            assert_eq!(
                continuation.usage,
                Some([Some(u64::MAX), Some(5), (!missing).then_some(3)])
            );
            assert_eq!(last.reply(&Api::ChatCompletions).unwrap().context, Some(18));
        }
    }

    #[test]
    fn length_is_continued_and_text_is_joined() {
        let mut continuation = Continuation::default();
        let mut first = chat_round(&[text("Hello, ", Some("length"))]);
        let follow_up = continuation
            .next(&Api::ChatCompletions, &mut first)
            .unwrap();
        assert_eq!(
            follow_up,
            [
                json!({"role":"assistant","content":"Hello, "}),
                json!({"role":"user","content":CONTINUE_PROMPT})
            ]
        );
        let mut second = chat_round(&[text("world.", Some("stop"))]);
        assert!(continuation
            .next(&Api::ChatCompletions, &mut second)
            .is_none());
        let reply = second.reply(&Api::ChatCompletions).unwrap();
        assert_eq!(reply_text(&reply), "Hello, world.");
    }

    #[test]
    fn partial_tool_call_is_discarded_and_the_reissued_call_is_returned() {
        let mut continuation = Continuation::default();
        let mut first = chat_round(&[
            text("Writing.", None),
            call("cut", "{\"path\":\"fi", Some("length")),
        ]);
        let follow_up = continuation
            .next(&Api::ChatCompletions, &mut first)
            .unwrap();
        assert_eq!(follow_up[1]["content"], REISSUE_PROMPT);
        assert!(!follow_up.iter().any(|m| m.get("tool_calls").is_some()));
        let mut second = chat_round(&[call("full", "{\"path\":\"file\"}", Some("tool_calls"))]);
        assert!(continuation
            .next(&Api::ChatCompletions, &mut second)
            .is_none());
        let reply = second.reply(&Api::ChatCompletions).unwrap();
        let Message::Assistant { content, .. } = &reply.message else {
            panic!("assistant reply expected")
        };
        let ids: Vec<_> = content
            .iter()
            .filter_map(|b| match b {
                Block::ToolCall { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["full"]);
        assert_eq!(reply_text(&reply), "Writing.");

        // A re-issued call that is cut again is an error, never executed.
        let mut continuation = Continuation::default();
        let mut first = chat_round(&[call("a", "{", Some("length"))]);
        continuation
            .next(&Api::ChatCompletions, &mut first)
            .unwrap();
        let mut second = chat_round(&[call("b", "{", Some("length"))]);
        assert!(continuation
            .next(&Api::ChatCompletions, &mut second)
            .is_none());
        let error = second
            .reply(&Api::ChatCompletions)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("tool call exceeded the provider output limit"),
            "{error}"
        );
    }

    #[test]
    fn continuations_stop_after_three_and_keep_the_text() {
        let mut continuation = Continuation::default();
        for round in 0..MAX_CONTINUATIONS {
            let mut stream = chat_round(&[text(&format!("{round}"), Some("length"))]);
            assert!(continuation
                .next(&Api::ChatCompletions, &mut stream)
                .is_some());
        }
        let mut last = chat_round(&[text("3", Some("length"))]);
        assert!(continuation
            .next(&Api::ChatCompletions, &mut last)
            .is_none());
        let reply = last.reply(&Api::ChatCompletions).unwrap();
        assert_eq!(reply_text(&reply), "0123");
    }

    #[test]
    fn content_filter_and_missing_finish_reason_are_errors() {
        let error = chat_round(&[text("x", Some("content_filter"))])
            .reply(&Api::ChatCompletions)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("content policy"), "{error}");
        assert!(!error.contains("truncated"), "{error}");
        let mut missing = chat_round(&[text("x", None)]);
        assert!(Continuation::default()
            .next(&Api::ChatCompletions, &mut missing)
            .is_none());
        let error = missing
            .reply(&Api::ChatCompletions)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("truncated or incomplete"), "{error}");
    }
}

#[cfg(feature = "fuzzing")]
pub(super) fn fuzz_stream(bytes: &[u8]) {
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
mod parser_fuzz_regressions {
    use super::*;

    #[test]
    fn sse_comments_stay_separate_from_data_across_every_chunk_boundary() {
        let wire =
            b": rooxcode waiting\r\n\r\n: ping\n\nevent: message\ndata: {\"choices\":[]}\n\n";
        for size in 1..=wire.len() {
            let mut decoder = Decoder::default();
            let frames: Vec<_> = wire
                .chunks(size)
                .flat_map(|bytes| decoder.push_frames(bytes).unwrap())
                .collect();
            assert_eq!(
                frames,
                vec![
                    Frame::Comment("rooxcode waiting".into()),
                    Frame::Comment("ping".into()),
                    Frame::Data("{\"choices\":[]}".into())
                ]
            );
        }
    }

    #[test]
    fn mixed_sse_delimiters_preserve_wire_order_across_chunks() {
        let wire = b"data: first\r\n\r\ndata: second\n\n";
        for chunk in 1..=wire.len() {
            let mut decoder = Decoder::default();
            let frames: Vec<_> = wire
                .chunks(chunk)
                .flat_map(|bytes| decoder.push(bytes).unwrap())
                .collect();
            assert_eq!(frames, ["first", "second"], "chunk size {chunk}");
        }
    }

    #[test]
    fn rejected_stream_chunk_preserves_the_bounded_buffer() {
        let mut decoder = Decoder::default();
        decoder.push(b"data: pending").unwrap();
        let before = decoder.buffer.clone();
        assert!(decoder.push(&vec![b'x'; 2 * 1024 * 1024]).is_err());
        assert_eq!(decoder.buffer, before);
    }

    #[test]
    fn malformed_anthropic_blocks_return_errors() {
        for block in [
            Value::Null,
            json!(false),
            json!(1),
            json!([]),
            json!("text"),
        ] {
            let mut stream = Stream::default();
            assert!(stream
                .observe(
                    &Api::Anthropic,
                    &json!({
                        "type":"content_block_start", "index":0, "content_block":block
                    })
                )
                .is_err());
        }
        let mut stream = Stream::default();
        stream
            .observe(
                &Api::Anthropic,
                &json!({
                    "type":"content_block_start", "index":0,
                    "content_block":{"type":"thinking","thinking":"example"}
                }),
            )
            .unwrap();
        assert!(stream
            .observe(
                &Api::Anthropic,
                &json!({
                    "type":"content_block_delta", "index":0,
                    "delta":{"type":"text_delta","text":"hello"}
                })
            )
            .is_err());
    }
}
