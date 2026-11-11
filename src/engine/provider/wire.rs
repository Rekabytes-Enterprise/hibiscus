//! Builds each API's request body from the stored conversation.
use crate::engine::{
    config::{Api, Model, Provider},
    session::{Block, Message},
};
use serde_json::{json, Value};

fn text_blocks(blocks: &[Block], api: &Api) -> Vec<Value> {
    blocks.iter().filter_map(|block|match block {
    Block::Text{text}=>Some(match api{Api::Responses=>json!({"type":"input_text","text":text}),_=>json!({"type":"text","text":text})}),
    Block::Image{mime_type,data}=>Some(match api{Api::Anthropic=>json!({"type":"image","source":{"type":"base64","media_type":mime_type,"data":data}}),Api::Responses=>json!({"type":"input_image","image_url":format!("data:{mime_type};base64,{data}")}),Api::ChatCompletions=>json!({"type":"image_url","image_url":{"url":format!("data:{mime_type};base64,{data}")}})}),_=>None}).collect()
}
/// A tool result's blocks as the model gets them: images only for a model
/// that takes them, else a note in their place.
fn tool_blocks(content: &[Block], model: &Model) -> Vec<Block> {
    if model.images || !content.iter().any(|b| matches!(b, Block::Image { .. })) {
        return content.to_vec();
    }
    let mut blocks: Vec<_> = content
        .iter()
        .filter(|b| !matches!(b, Block::Image { .. }))
        .cloned()
        .collect();
    blocks.push(Block::Text {
        text: "[The image was not sent: this model does not accept images.]".into(),
    });
    blocks
}
fn has_image(blocks: &[Block]) -> bool {
    blocks.iter().any(|b| matches!(b, Block::Image { .. }))
}
fn wire_messages(messages: &[Message], provider: &Provider, model: &Model) -> Vec<Value> {
    let api = &provider.api;
    let mut wire = vec![];
    // Chat Completions tool messages hold text only, so their images follow
    // in one user message after the last tool result of the step.
    let mut images: Vec<Value> = vec![];
    for m in messages {
        if !images.is_empty() && !matches!(m, Message::ToolResult { .. }) {
            wire.push(json!({"role":"user","content":std::mem::take(&mut images)}));
        }
        let tool = match m {
            Message::ToolResult { content, .. } => tool_blocks(content, model),
            _ => vec![],
        };
        let tool_text = tool
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
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
            (Api::Responses, Message::ToolResult { tool_call_id, .. }) => {
                let output = if has_image(&tool) {
                    json!(text_blocks(&tool, api))
                } else {
                    json!(tool_text)
                };
                wire.push(
                    json!({"type":"function_call_output","call_id":tool_call_id,"output":output}),
                )
            }
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
                let content = if has_image(&tool) {
                    json!(text_blocks(&tool, api))
                } else {
                    json!(tool_text)
                };
                let result = json!({"type":"tool_result","tool_use_id":tool_call_id,"content":content,"is_error":is_error});
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
                wire.push(json!({"role":"tool","tool_call_id":tool_call_id,"content":tool_text}));
                let attached: Vec<_> = tool
                    .iter()
                    .filter(|b| matches!(b, Block::Image { .. }))
                    .cloned()
                    .collect();
                if !attached.is_empty() {
                    images.push(json!({"type":"text","text":format!("Image from tool call {tool_call_id}:")}));
                    images.extend(text_blocks(&attached, api));
                }
            }
        }
    }
    if !images.is_empty() {
        wire.push(json!({"role":"user","content":images}));
    }
    wire
}
pub(in crate::engine) fn request_body(
    provider: &Provider,
    model: &Model,
    conversation: &crate::engine::session::Session,
    system: &str,
    thinking: &str,
    tools: Option<&[Value]>,
) -> (&'static str, Value) {
    let anonymous = provider.authentication == crate::engine::config::Authentication::Rooxcode;
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
            crate::engine::thinking::Policy::for_model(provider, model).rooxcode_effort(thinking)
        {
            body["reasoning_effort"] = json!(effort);
        }
        // Let the gateway apply its advertised default output limit.
        // The client reserves output space without overriding that default.
        body.as_object_mut().unwrap().remove("max_tokens");
    }
    if model.max_tokens == 0 {
        // Discovered catalog models: the provider applies its own output limit.
        let body = body.as_object_mut().unwrap();
        body.remove("max_tokens");
        body.remove("max_output_tokens");
    }
    if tools.is_none() {
        body.as_object_mut().unwrap().remove("tools");
    }
    (route, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::config::Authentication;

    fn shot(api: Api, images: bool) -> Vec<Value> {
        let provider = Provider {
            id: "p".into(),
            base_url: String::new(),
            api,
            authentication: Authentication::default(),
            api_key_env: None,
            models: vec![],
        };
        let model: Model = serde_json::from_value(json!({"id":"m","images":images})).unwrap();
        let messages = [
            Message::ToolResult {
                tool_call_id: "c1".into(),
                tool_name: "browser".into(),
                content: vec![
                    Block::Text {
                        text: "saved".into(),
                    },
                    Block::Image {
                        mime_type: "image/jpeg".into(),
                        data: "AAAA".into(),
                    },
                ],
                is_error: false,
                details: json!({}),
            },
            Message::User {
                content: vec![Block::Text {
                    text: "next".into(),
                }],
            },
        ];
        wire_messages(&messages, &provider, &model)
    }

    #[test]
    fn tool_result_images_reach_each_api() {
        let anthropic = shot(Api::Anthropic, true);
        assert_eq!(anthropic[0]["content"][0]["content"][1]["type"], "image");
        let responses = shot(Api::Responses, true);
        assert_eq!(responses[0]["output"][1]["type"], "input_image");
        // Tool messages hold text, so the image follows as a user message.
        let chat = shot(Api::ChatCompletions, true);
        assert_eq!(chat[0]["content"], "saved");
        assert_eq!(chat[1]["role"], "user");
        assert_eq!(chat[1]["content"][1]["type"], "image_url");
        assert_eq!(chat[2]["content"][0]["text"], "next");
    }

    #[test]
    fn models_without_image_input_get_a_note_instead() {
        let anthropic = shot(Api::Anthropic, false);
        let text = anthropic[0]["content"][0]["content"].as_str().unwrap();
        assert!(text.starts_with("saved\n[The image was not sent"), "{text}");
        assert_eq!(shot(Api::ChatCompletions, false).len(), 2);
    }
}
