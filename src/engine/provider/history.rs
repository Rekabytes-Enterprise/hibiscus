//! What the model gets of older turns. Recent tool outputs and tool-call
//! arguments go in full; older large ones are replaced by a short stub, so a
//! long session stops re-sending every file it ever read. The stored session
//! keeps everything; this only shapes the request.
use crate::engine::session::{Block, Message};
use serde_json::{json, Value};
use std::borrow::Cow;

/// The latest tool results always go in full.
const KEEP: usize = 10;
/// The cut moves in steps of this many results, so the start of the request
/// stays identical between steps and provider prompt caches keep hitting.
const STEP: usize = 10;
/// Outputs and argument strings up to this size stay as they are.
const SMALL: usize = 2048;
/// How much of an older output's start stays ahead of its stub.
const HEAD: usize = 400;

/// The first message sent in full: past the stepped cut of older tool results,
/// and past the latest canvas step, whose prompt carries what that step needs.
fn boundary(messages: &[Message]) -> usize {
    let results: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(m, Message::ToolResult { .. }))
        .map(|(i, _)| i)
        .collect();
    let cut = results.len().saturating_sub(KEEP) / STEP * STEP;
    // The kept result's own call stays whole too.
    let by_count = if cut == 0 {
        0
    } else {
        messages[..results[cut]]
            .iter()
            .rposition(|m| matches!(m, Message::Assistant { .. }))
            .unwrap_or(0)
    };
    let canvas = messages
        .iter()
        .rposition(|m| matches!(m, Message::User { .. }) && m.text().starts_with("[Canvas "))
        .unwrap_or(0);
    by_count.max(canvas)
}

/// The conversation as the model gets it.
pub(super) fn lean(messages: &[Message]) -> Cow<'_, [Message]> {
    let end = boundary(messages);
    if end == 0 {
        return Cow::Borrowed(messages);
    }
    let mut out = messages.to_vec();
    for message in &mut out[..end] {
        match message {
            Message::ToolResult { content, .. } => shrink_result(content),
            Message::Assistant {
                content,
                provider_data,
                ..
            } => {
                for block in content {
                    if let Block::ToolCall { arguments, .. } = block {
                        shrink_value(arguments);
                    }
                }
                if let Some(raw) = provider_data {
                    shrink_raw(raw);
                }
            }
            Message::User { .. } => {}
        }
    }
    Cow::Owned(out)
}

fn kb(bytes: usize) -> String {
    format!("{:.1} KB", bytes as f64 / 1024.0)
}

fn shrink_result(content: &mut Vec<Block>) {
    let text: String = content
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let images = content
        .iter()
        .filter(|b| matches!(b, Block::Image { .. }))
        .count();
    if text.len() <= SMALL && images == 0 {
        return;
    }
    let mut head = text.len().min(HEAD);
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let pictures = match images {
        0 => String::new(),
        1 => " and 1 image".into(),
        n => format!(" and {n} images"),
    };
    *content = vec![Block::Text {
        text: format!(
            "{}\n[Older output left out to save context: {}{pictures}. Run the tool again if you need it.]",
            &text[..head],
            kb(text.len())
        ),
    }];
}

/// Long strings in tool-call arguments, such as a written file or an SVG.
fn shrink_value(value: &mut Value) {
    match value {
        Value::String(s) if s.len() > SMALL => {
            *s = format!("[{} left out of older history]", kb(s.len()));
        }
        Value::Array(items) => items.iter_mut().for_each(shrink_value),
        Value::Object(map) => map.values_mut().for_each(shrink_value),
        _ => {}
    }
}

/// The same inside a provider's replayed reply: `arguments` JSON strings
/// (Responses, Chat Completions) and `tool_use` inputs (Anthropic). Text and
/// reasoning stay as they are.
fn shrink_raw(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(shrink_raw),
        Value::Object(map) => {
            let tool_use = map.get("type").and_then(Value::as_str) == Some("tool_use");
            for (key, child) in map.iter_mut() {
                match (key.as_str(), &mut *child) {
                    ("arguments", Value::String(s)) if s.len() > SMALL => {
                        let mut parsed = serde_json::from_str(s)
                            .unwrap_or_else(|_| json!({"arguments": s.clone()}));
                        shrink_value(&mut parsed);
                        *s = parsed.to_string();
                    }
                    ("input", _) if tool_use => shrink_value(child),
                    _ => shrink_raw(child),
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, content: &str) -> Message {
        Message::Assistant {
            content: vec![Block::ToolCall {
                id: id.into(),
                name: "write".into(),
                arguments: json!({"path":"a.txt","content":content}),
            }],
            stop_reason: "toolUse".into(),
            provider_data: Some(
                json!({"api":"chat_completions","message":{"tool_calls":[{"function":{"arguments":json!({"path":"a.txt","content":content}).to_string()}}]}}),
            ),
        }
    }
    fn result(id: &str, text: &str) -> Message {
        Message::ToolResult {
            tool_call_id: id.into(),
            tool_name: "read".into(),
            content: vec![Block::Text { text: text.into() }],
            is_error: false,
            details: json!({}),
        }
    }
    fn user(text: &str) -> Message {
        Message::User {
            content: vec![Block::Text { text: text.into() }],
        }
    }
    fn turns(n: usize) -> Vec<Message> {
        let big = "x".repeat(5000);
        let mut messages = vec![user("start")];
        for i in 0..n {
            messages.push(call(&format!("c{i}"), &big));
            messages.push(result(&format!("c{i}"), &big));
        }
        messages
    }
    fn full(m: &Message) -> bool {
        m.text().len() == 5000
    }

    #[test]
    fn short_sessions_go_unchanged() {
        assert!(matches!(lean(&turns(19)), Cow::Borrowed(_)));
    }

    #[test]
    fn older_results_and_arguments_shrink_in_steps() {
        let messages = turns(25);
        let sent = lean(&messages);
        // 25 results: the cut sits at 10, a step of 10, so results 10 to 24 stay whole.
        let whole: Vec<bool> = sent
            .iter()
            .filter(|m| matches!(m, Message::ToolResult { .. }))
            .map(full)
            .collect();
        assert_eq!(whole.iter().filter(|w| !**w).count(), 10);
        assert!(whole[10..].iter().all(|w| *w));
        let Message::ToolResult { content, .. } = &sent[2] else {
            panic!()
        };
        let Block::Text { text } = &content[0] else {
            panic!()
        };
        assert!(text.starts_with(&"x".repeat(400)));
        assert!(text.contains("left out to save context: 4.9 KB"), "{text}");
        let Message::Assistant {
            content,
            provider_data,
            ..
        } = &sent[1]
        else {
            panic!()
        };
        let Block::ToolCall { arguments, .. } = &content[0] else {
            panic!()
        };
        assert_eq!(arguments["path"], "a.txt");
        assert_eq!(arguments["content"], "[4.9 KB left out of older history]");
        let raw = provider_data.as_ref().unwrap()["message"]["tool_calls"][0]["function"]
            ["arguments"]
            .as_str()
            .unwrap();
        assert!(raw.len() < 100 && raw.contains("a.txt"), "{raw}");
        // Until 10 more results arrive, the request's start stays the same.
        let more = turns(29);
        let later = lean(&more);
        assert_eq!(
            serde_json::to_string(&sent[..21]).unwrap(),
            serde_json::to_string(&later[..21]).unwrap()
        );
    }

    #[test]
    fn a_canvas_step_starts_from_its_own_prompt() {
        let mut messages = turns(3);
        let mut image = result("shot", "frames");
        if let Message::ToolResult { content, .. } = &mut image {
            content.push(Block::Image {
                mime_type: "image/jpeg".into(),
                data: "AAAA".into(),
            });
        }
        messages.insert(1, image);
        messages.push(user("[Canvas Video card] Build it"));
        messages.push(call("c9", &"y".repeat(5000)));
        let sent = lean(&messages);
        assert!(sent[1].text().contains("and 1 image"));
        assert!(!sent[..8].iter().any(full));
        // The step's own work stays whole.
        let Message::Assistant { content, .. } = &sent[9] else {
            panic!()
        };
        let Block::ToolCall { arguments, .. } = &content[0] else {
            panic!()
        };
        assert_eq!(arguments["content"].as_str().unwrap().len(), 5000);
    }
}

/// Replays saved sessions through the old and new request building, one
/// request per assistant reply: `HIBISCUS_REPLAY=a.jsonl:b.jsonl cargo test
/// replay_saved_sessions -- --ignored --nocapture`.
#[cfg(test)]
mod replay {
    use super::*;
    use crate::engine::capped;

    /// Text bytes of what the model gets, and its images (counted apart:
    /// a provider bills an image by its size in pixels, not its base64).
    fn size(messages: &[Message]) -> (usize, usize) {
        let mut bytes = 0;
        let mut images = 0;
        for m in messages {
            let mut m = m.clone();
            let blocks = match &mut m {
                Message::User { content } | Message::ToolResult { content, .. } => content,
                Message::Assistant {
                    content,
                    provider_data,
                    ..
                } => {
                    *provider_data = None;
                    content
                }
            };
            blocks.retain(|b| {
                let image = matches!(b, Block::Image { .. });
                images += image as usize;
                !image
            });
            bytes += serde_json::to_string(&m).unwrap().len();
        }
        (bytes, images)
    }

    /// Stored outputs as the new cap would have sent them.
    fn capped(messages: &[Message]) -> Vec<Message> {
        let mut out = messages.to_vec();
        for m in &mut out {
            if let Message::ToolResult {
                tool_name, content, ..
            } = m
            {
                if capped::exempt(tool_name) {
                    continue;
                }
                if let Some(Block::Text { text }) = content.first_mut() {
                    let clean = capped::clean(text);
                    *text = if clean.len() > capped::INLINE {
                        capped::shorten(&clean, Ok("replay".into()))
                    } else {
                        clean
                    };
                }
            }
        }
        out
    }

    #[test]
    #[ignore]
    fn replay_saved_sessions() {
        let Ok(paths) = std::env::var("HIBISCUS_REPLAY") else {
            return;
        };
        let (mut all_before, mut all_after) = (0usize, 0usize);
        for path in paths.split(':') {
            let messages: Vec<Message> = std::fs::read_to_string(path)
                .unwrap()
                .lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter(|e| e["type"] == "message")
                .filter_map(|e| serde_json::from_value(e["message"].clone()).ok())
                .collect();
            let (mut before, mut after, mut img_before, mut img_after, mut requests, mut biggest) =
                (0, 0, 0, 0, 0, 0);
            let capped_all = capped(&messages);
            for i in 0..messages.len() {
                if !matches!(messages[i], Message::Assistant { .. }) {
                    continue;
                }
                requests += 1;
                let (b, ib) = size(&messages[..i]);
                let (a, ia) = size(&lean(&capped_all[..i]));
                before += b;
                after += a;
                img_before += ib;
                img_after += ia;
                biggest = biggest.max(b);
            }
            all_before += before;
            all_after += after;
            let name = path.rsplit('/').next().unwrap();
            println!(
                "{name}: {requests} requests, largest {:.0}K tokens | sent {:.2}M -> {:.2}M tokens ({:.0}% less) | images {img_before} -> {img_after}",
                biggest as f64 / 4000.0,
                before as f64 / 4e6,
                after as f64 / 4e6,
                100.0 - after as f64 * 100.0 / before.max(1) as f64
            );
        }
        println!(
            "ALL: {:.2}M -> {:.2}M tokens ({:.0}% less)",
            all_before as f64 / 4e6,
            all_after as f64 / 4e6,
            100.0 - all_after as f64 * 100.0 / all_before.max(1) as f64
        );
    }
}
