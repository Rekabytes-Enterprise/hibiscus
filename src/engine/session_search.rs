//! The session_search tool: finds earlier conversations in this workspace's
//! saved sessions, for details that never made it into memory. Only what the
//! user and the agent wrote is searched, not tool output.
use super::session::Message;
use crate::Result;
use serde_json::Value;
use std::path::Path;
use std::time::SystemTime;

/// Matches returned at most, and at most this many from one session.
const MAX_MATCHES: usize = 20;
const PER_SESSION: usize = 5;
/// Characters of context shown around a match.
const SNIPPET: usize = 300;

/// Messages in this workspace's saved sessions, other than `current`, that
/// contain every word of `query` (any case), newest session first.
pub(super) fn search(cwd: &Path, current: &str, query: &str) -> Result<String> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Err("session_search needs words to look for".into());
    }
    let mut sessions = crate::chat::saved_sessions::list(cwd)?;
    sessions.sort_by(|a, b| b.modified.cmp(&a.modified));
    let mut out = String::new();
    let mut found = 0;
    for session in sessions {
        if session.path.file_stem().and_then(|s| s.to_str()) == Some(current) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&session.path) else {
            continue;
        };
        let hits = matches(&text, &words);
        if hits.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "## {} ({})\n",
            session.title,
            age(session.modified)
        ));
        for (who, snippet) in hits {
            if found == MAX_MATCHES {
                out.push_str(&format!(
                    "\n[Showing the first {MAX_MATCHES} matches; add words to narrow the search.]"
                ));
                return Ok(out);
            }
            found += 1;
            out.push_str(&format!("- {who}: {snippet}\n"));
        }
    }
    Ok(if out.is_empty() {
        "No earlier session in this workspace mentions that.".into()
    } else {
        out
    })
}

/// The user and assistant messages in one session file that hold every word.
fn matches(file: &str, words: &[String]) -> Vec<(&'static str, String)> {
    let mut hits = vec![];
    for line in file.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if record["type"] != "message" {
            continue;
        }
        let Ok(message) = serde_json::from_value::<Message>(record["message"].clone()) else {
            continue;
        };
        let who = match message {
            Message::User { .. } => "user",
            Message::Assistant { .. } => "agent",
            Message::ToolResult { .. } => continue,
        };
        let text = message.text();
        let lower = text.to_lowercase();
        if !words.iter().all(|w| lower.contains(w.as_str())) {
            continue;
        }
        hits.push((who, snippet(&text, &lower, &words[0])));
        if hits.len() == PER_SESSION {
            break;
        }
    }
    hits
}

/// About `SNIPPET` characters of `text` on one line, around the first `word`.
fn snippet(text: &str, lower: &str, word: &str) -> String {
    // Lowercasing can change byte lengths, so find the match by characters.
    let at = lower
        .find(word)
        .map_or(0, |byte| lower[..byte].chars().count());
    let chars: Vec<char> = text.chars().collect();
    let start = at.saturating_sub(SNIPPET / 3).min(chars.len());
    let end = (start + SNIPPET).min(chars.len());
    let body: String = chars[start..end]
        .iter()
        .map(|c| if c.is_whitespace() { ' ' } else { *c })
        .collect();
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        body.trim(),
        if end < chars.len() { "…" } else { "" }
    )
}

fn age(modified: SystemTime) -> String {
    let days = SystemTime::now()
        .duration_since(modified)
        .map_or(0, |d| d.as_secs() / 86_400);
    match days {
        0 => "today".into(),
        1 => "1 day ago".into(),
        n => format!("{n} days ago"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(message: Value) -> String {
        json!({"type":"message","message":message}).to_string()
    }

    #[test]
    fn finds_user_and_agent_lines_with_every_word() {
        let file = [
            json!({"type":"session","cwd":"/x"}).to_string(),
            line(json!({"role":"user","content":[{"type":"text","text":"Publish the Release only after all 16 files are in the draft."}]})),
            line(json!({"role":"assistant","content":[{"type":"text","text":"Noted: the release waits for 16 files."}],"stopReason":"stop"})),
            line(json!({"role":"toolResult","toolCallId":"1","toolName":"bash","content":[{"type":"text","text":"release 16 files"}],"isError":false,"details":{}})),
            line(json!({"role":"user","content":[{"type":"text","text":"Release notes go in the changelog."}]})),
        ]
        .join("\n");
        let words = ["release".to_owned(), "16".to_owned()];
        let hits = matches(&file, &words);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, "user");
        assert!(hits[0].1.starts_with("Publish the Release"));
        assert_eq!(hits[1].0, "agent");
    }

    #[test]
    fn long_messages_show_the_text_around_the_match() {
        let text = format!("{}İ needle here{}", "a ".repeat(400), " b".repeat(400));
        let lower = text.to_lowercase();
        let shown = snippet(&text, &lower, "needle");
        assert!(shown.starts_with('…') && shown.ends_with('…'));
        assert!(shown.contains("needle here"));
        assert!(shown.chars().count() <= SNIPPET + 2);
    }
}
