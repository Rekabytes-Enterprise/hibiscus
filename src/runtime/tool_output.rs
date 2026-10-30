//! Read-only projections of results already stored in the active session.
use super::session::{Block, Message};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

#[derive(Serialize, Deserialize)]
pub(crate) struct Summary {
    pub index: usize,
    pub name: String,
    pub target: String,
    pub failed: bool,
    pub exit_code: Option<i64>,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Output {
    pub summary: Summary,
    pub text: String,
    pub diff: Option<String>,
}

fn results(messages: &[Message]) -> Vec<(usize, &Message)> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(m, Message::ToolResult { .. }))
        .collect()
}
fn summary(messages: &[Message], position: usize, index: usize) -> Summary {
    let Message::ToolResult {
        tool_call_id,
        tool_name,
        is_error,
        details,
        ..
    } = &messages[position]
    else {
        unreachable!()
    };
    let args = messages[..position].iter().rev().find_map(|message| {
        let Message::Assistant { content, .. } = message else {
            return None;
        };
        content.iter().find_map(|block| match block {
            Block::ToolCall { id, arguments, .. } if id == tool_call_id => Some(arguments),
            _ => None,
        })
    });
    let target = args
        .and_then(|args| match tool_name.as_str() {
            "bash" => args["command"].as_str(),
            "read" | "write" | "edit" => {
                args["path"].as_str().or_else(|| args["filePath"].as_str())
            }
            _ => None,
        })
        .unwrap_or("")
        .to_owned();
    Summary {
        index,
        name: tool_name.clone(),
        target,
        failed: *is_error,
        exit_code: details["exitCode"].as_i64(),
    }
}
pub(super) fn list(messages: &[Message]) -> Vec<Summary> {
    let results = results(messages);
    let start = results.len().saturating_sub(50);
    results[start..]
        .iter()
        .enumerate()
        .map(|(i, (position, _))| {
            let mut item = summary(messages, *position, start + i + 1);
            item.target = item.target.chars().take(200).collect();
            item
        })
        .collect()
}
pub(super) fn get(messages: &[Message], index: Option<NonZeroUsize>) -> Option<Output> {
    let results = results(messages);
    let index = index.map(NonZeroUsize::get).unwrap_or(results.len());
    let (position, message) = results.get(index.checked_sub(1)?)?;
    let diff = match message {
        Message::ToolResult { details, .. } => details["diff"].as_str().map(str::to_owned),
        _ => None,
    };
    Some(Output {
        summary: summary(messages, *position, index),
        text: message.text(),
        diff,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn recent_list_is_bounded_and_reused_call_ids_match_their_own_result() {
        let mut messages = Vec::new();
        for index in 1..=55 {
            messages.push(Message::Assistant {
                content: vec![Block::ToolCall {
                    id: "reused".into(),
                    name: "bash".into(),
                    arguments: json!({"command":format!("printf {index}")}),
                }],
                stop_reason: "toolUse".into(),
                provider_data: None,
            });
            messages.push(Message::ToolResult {
                tool_call_id: "reused".into(),
                tool_name: "bash".into(),
                content: vec![Block::Text {
                    text: format!("detail {index}\n\n"),
                }],
                is_error: false,
                details: json!({"exitCode":0}),
            });
        }
        let recent = list(&messages);
        assert_eq!(recent.len(), 50);
        assert_eq!((recent[0].index, recent[49].index), (6, 55));
        let first = get(&messages, NonZeroUsize::new(1)).unwrap();
        assert_eq!(first.summary.target, "printf 1");
        assert_eq!(first.text, "detail 1\n\n");
        assert_eq!(get(&messages, None).unwrap().summary.target, "printf 55");
        assert!(get(&messages, NonZeroUsize::new(56)).is_none());
        assert!(list(&[]).is_empty() && get(&[], None).is_none());
    }
}
