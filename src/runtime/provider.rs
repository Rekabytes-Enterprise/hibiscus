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
pub async fn request(
    provider: &Provider,
    model: &Model,
    messages: &[Message],
    system: &str,
    thinking: &str,
    tools: bool,
    out: &Sender<Value>,
) -> Result<Reply> {
    let codex = if provider.authentication == super::config::Authentication::Codex {
        Some(super::oauth::credential().await?)
    } else {
        None
    };
    let key = if let Some(token) = &codex {
        token.access.clone()
    } else {
        super::config::key(provider)?
    };
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .build()?;
    let definitions = if tools {
        super::tools::definitions()
    } else {
        vec![]
    };
    let mut wire = wire_messages(messages, provider, model);
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
    if !tools {
        body.as_object_mut().unwrap().remove("tools");
    }
    let url = format!("{}/{}", provider.base_url.trim_end_matches('/'), route);
    let req = authenticated_request(&client, &url, body, &key, codex.as_ref())?;
    let req = if provider.api == Api::Anthropic {
        req.header("anthropic-version", "2023-06-01")
    } else {
        req
    };
    let mut response = req.send().await.map_err(|_| {
        "Provider network request failed. Check connectivity and endpoint configuration."
    })?;
    if !response.status().is_success() {
        return Err(format!("Provider returned HTTP {}. Check authentication, model access, quota and endpoint configuration.",response.status().as_u16()).into());
    }
    let mut decoder = Decoder::default();
    let mut state = Stream::default();
    'stream: while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Provider response stream failed; partial output was not committed.")?
    {
        for frame in decoder.push(&chunk)? {
            if frame == "[DONE]" {
                state.done = true;
                break 'stream;
            }
            let event: Value =
                serde_json::from_str(&frame).map_err(|_| "Malformed provider streaming event")?;
            let delta = state.observe(&provider.api, &event)?;
            if let Some(delta) = delta.filter(|_| tools) {
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
    let mut reply = state.reply(&provider.api)?;
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
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > 2 * 1024 * 1024 {
            return Err("Provider streaming frame exceeds 2 MiB".into());
        }
        let mut frames = vec![];
        loop {
            let marker = self
                .buffer
                .windows(2)
                .position(|v| v == b"\n\n")
                .map(|i| (i, 2))
                .or_else(|| {
                    self.buffer
                        .windows(4)
                        .position(|v| v == b"\r\n\r\n")
                        .map(|i| (i, 4))
                });
            let Some((end, skip)) = marker else { break };
            let frame = String::from_utf8(self.buffer.drain(..end + skip).collect())?;
            let data = frame
                .lines()
                .filter_map(|l| {
                    l.strip_prefix("data:")
                        .map(|s| s.strip_prefix(' ').unwrap_or(s))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !data.is_empty() {
                frames.push(data);
            }
        }
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
                            b["text"] = json!(format!("{}{}", b["text"].as_str().unwrap_or(""), t));
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
                if !matches!(self.finish.as_str(), "stop" | "tool_calls") {
                    return Err("Provider response was truncated or incomplete".into());
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
}
