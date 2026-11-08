//! Folds one API's streamed events into text, tool calls, usage and the final reply.
use super::Reply;
use crate::engine::{
    config::Api,
    session::{Block, Message},
};
use crate::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Stream {
    pub(super) text: String,
    pub(super) calls: BTreeMap<u64, (String, String, String)>,
    pub(super) blocks: BTreeMap<u64, Value>,
    pub(super) raw: Vec<Value>,
    pub(super) input: Option<u64>,
    pub(super) output: Option<u64>,
    pub(super) cache: Option<u64>,
    pub(super) cache_created: Option<u64>,
    pub(super) done: bool,
    pub(super) finish: String,
}
impl Stream {
    pub(super) fn observe(&mut self, api: &Api, v: &Value) -> Result<Option<String>> {
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
    pub(super) fn reply(mut self, api: &Api) -> Result<Reply> {
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
