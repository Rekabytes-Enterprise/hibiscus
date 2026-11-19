//! Pages of the conversation for the desktop chat (#171): the latest
//! messages first, older ones as the user scrolls, with big tool output and
//! images left for `get_tool_result`. The engine keeps every message; only
//! what goes to the window is cut.
use super::session::{Block, Message};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Bytes of a tool output sent in a page.
const PREVIEW: usize = 2048;
/// An edit's diff larger than this is left for `get_tool_result` too.
const DIFF: usize = 16384;

/// Up to `limit` messages ending before `before` (default: the end). A page
/// never starts with a tool result, so each call stays with its results.
pub(super) fn page(messages: &[Message], limit: usize, before: Option<usize>) -> Value {
    let end = before.unwrap_or(messages.len()).min(messages.len());
    let mut start = end.saturating_sub(limit.max(1));
    while start > 0 && matches!(messages[start], Message::ToolResult { .. }) {
        start -= 1;
    }
    let page: Vec<Value> = messages[start..end].iter().map(preview).collect();
    json!({"messages":page,"start":start,"total":messages.len(),"edited":edited(messages)})
}

/// One tool result in full, for a row the user opens.
pub(super) fn tool_result(messages: &[Message], call: &str) -> Option<Value> {
    messages
        .iter()
        .find(|m| matches!(m, Message::ToolResult { tool_call_id, .. } if tool_call_id == call))
        .and_then(|m| serde_json::to_value(m).ok())
}

fn preview(message: &Message) -> Value {
    let mut value = serde_json::to_value(message).unwrap_or_default();
    match message {
        Message::ToolResult {
            content, details, ..
        } => {
            let mut trimmed = false;
            let mut images = 0;
            let blocks: Vec<Value> = content
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } if text.len() > PREVIEW => {
                        trimmed = true;
                        Some(json!({"type":"text","text":&text[..boundary(text, PREVIEW)]}))
                    }
                    Block::Image { .. } => {
                        images += 1;
                        None
                    }
                    b => serde_json::to_value(b).ok(),
                })
                .collect();
            value["content"] = json!(blocks);
            if details["diff"].as_str().is_some_and(|d| d.len() > DIFF) {
                trimmed = true;
                value["details"]["diff"] = Value::Null;
            }
            if trimmed {
                value["trimmed"] = json!(true);
            }
            if images > 0 {
                value["omittedImages"] = json!(images);
            }
        }
        // A prompt's images only show as a count, so their data stays behind.
        Message::User { content } => {
            let blocks: Vec<Value> = content
                .iter()
                .map(|b| match b {
                    Block::Image { mime_type, .. } => json!({"type":"image","mimeType":mime_type}),
                    b => serde_json::to_value(b).unwrap_or_default(),
                })
                .collect();
            value["content"] = json!(blocks);
        }
        Message::Assistant { .. } => {}
    }
    value
}

/// Files written or edited without error, newest first, for the session panel.
fn edited(messages: &[Message]) -> Vec<String> {
    let failed: HashMap<&str, bool> = messages
        .iter()
        .filter_map(|m| match m {
            Message::ToolResult {
                tool_call_id,
                is_error,
                ..
            } => Some((tool_call_id.as_str(), *is_error)),
            _ => None,
        })
        .collect();
    let mut seen = HashSet::new();
    messages
        .iter()
        .rev()
        .filter_map(|m| match m {
            Message::Assistant { content, .. } => Some(content),
            _ => None,
        })
        .flat_map(|content| content.iter().rev())
        .filter_map(|b| match b {
            Block::ToolCall {
                id,
                name,
                arguments,
            } if (name == "write" || name == "edit") && failed.get(id.as_str()) == Some(&false) => {
                arguments["path"]
                    .as_str()
                    .or_else(|| arguments["filePath"].as_str())
            }
            _ => None,
        })
        .filter(|path| !path.is_empty() && seen.insert(*path))
        .map(String::from)
        .collect()
}

fn boundary(text: &str, max: usize) -> usize {
    let mut len = max.min(text.len());
    while !text.is_char_boundary(len) {
        len -= 1;
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Message {
        Message::User {
            content: vec![Block::Text { text: text.into() }],
        }
    }
    fn call(id: &str, name: &str, path: &str) -> Message {
        Message::Assistant {
            content: vec![Block::ToolCall {
                id: id.into(),
                name: name.into(),
                arguments: json!({ "path": path }),
            }],
            stop_reason: "toolUse".into(),
            provider_data: None,
        }
    }
    fn result(id: &str, content: Vec<Block>, failed: bool) -> Message {
        Message::ToolResult {
            tool_call_id: id.into(),
            tool_name: "edit".into(),
            content,
            is_error: failed,
            details: json!({}),
        }
    }

    #[test]
    fn pages_end_at_the_latest_message_and_keep_calls_with_results() {
        let messages = vec![
            user("one"),
            call("a", "edit", "a.rs"),
            result("a", vec![Block::Text { text: "ok".into() }], false),
            user("two"),
        ];
        let latest = page(&messages, 2, None);
        // Starting at the result would split it from its call, so the page starts one earlier.
        assert_eq!(latest["start"], 1);
        assert_eq!(latest["total"], 4);
        assert_eq!(latest["messages"].as_array().unwrap().len(), 3);
        let older = page(&messages, 2, Some(1));
        assert_eq!(older["start"], 0);
        assert_eq!(older["messages"][0]["content"][0]["text"], "one");
    }

    #[test]
    fn big_outputs_and_images_are_left_for_get_tool_result() {
        let long = "é".repeat(PREVIEW);
        let messages = vec![
            Message::User {
                content: vec![Block::Image {
                    mime_type: "image/png".into(),
                    data: "AAAA".into(),
                }],
            },
            call("a", "browser", ""),
            result(
                "a",
                vec![
                    Block::Text { text: long.clone() },
                    Block::Image {
                        mime_type: "image/jpeg".into(),
                        data: "BBBB".into(),
                    },
                ],
                false,
            ),
        ];
        let p = page(&messages, 10, None);
        assert_eq!(p["messages"][0]["content"][0]["data"], Value::Null);
        assert_eq!(p["messages"][0]["content"][0]["type"], "image");
        let shown = &p["messages"][2];
        assert_eq!(shown["trimmed"], true);
        assert_eq!(shown["omittedImages"], 1);
        assert!(shown["content"][0]["text"].as_str().unwrap().len() <= PREVIEW);
        let full = tool_result(&messages, "a").unwrap();
        assert_eq!(full["content"][0]["text"], long.as_str());
        assert_eq!(full["content"][1]["data"], "BBBB");
    }

    #[test]
    fn edited_lists_successful_writes_newest_first() {
        let ok = |id| result(id, vec![], false);
        let messages = vec![
            call("a", "edit", "a.rs"),
            ok("a"),
            call("b", "write", "b.rs"),
            result("b", vec![], true),
            call("c", "write", "c.rs"),
            ok("c"),
            call("d", "edit", "a.rs"),
            ok("d"),
        ];
        assert_eq!(edited(&messages), ["a.rs", "c.rs"]);
    }
}
