//! Builds each API's request body from the stored conversation.
use crate::runtime::{
    config::{Api, Model, Provider},
    session::{Block, Message},
};
use serde_json::{json, Value};

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
pub(in crate::runtime) fn request_body(
    provider: &Provider,
    model: &Model,
    conversation: &crate::runtime::session::Session,
    system: &str,
    thinking: &str,
    tools: Option<&[Value]>,
) -> (&'static str, Value) {
    let anonymous = provider.authentication == crate::runtime::config::Authentication::Rooxcode;
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
            crate::runtime::thinking::Policy::for_model(provider, model).rooxcode_effort(thinking)
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
