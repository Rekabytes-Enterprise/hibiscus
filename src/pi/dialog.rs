use crate::tui::terminal::{self, RawMode};
use crate::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

pub(crate) struct Dialogs;

impl Dialogs {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn handle<R: BufRead, W: Write>(
        &mut self,
        input: &mut impl Write,
        event: &Value,
        fallback: &mut R,
        display: &mut W,
        raw: &mut Option<RawMode>,
        events: Option<&Receiver<u8>>,
        interactive: bool,
    ) -> Result<()> {
        if event["type"] == "extension_error" {
            let error = super::error::ChatError::new(
                super::error::ErrorSource::Extension,
                event["error"].as_str().unwrap_or("Extension failed"),
            );
            if interactive {
                writeln!(display, "\n  · Warning: {error}")?;
            } else {
                eprintln!("pi: {error}");
            }
            return Ok(());
        }
        if event["type"] != "extension_ui_request" {
            return Ok(());
        }
        let method = event["method"].as_str().unwrap_or("");
        if method == "notify" {
            writeln!(
                display,
                "[pi: {}]",
                super::error::safe_message(event["message"].as_str().unwrap_or("notification"))
            )?;
            return Ok(());
        }
        if !matches!(method, "select" | "confirm" | "input" | "editor") {
            return Ok(());
        }
        // Only a real terminal can approve. Fallback input is not used for
        // authorizing tool actions, even if prompts were piped into stdin.
        let response = if interactive {
            if let (Some(raw), Some(events)) = (raw.as_mut(), events) {
                response_for(event, |question| {
                    raw.write(question)?;
                    if method == "select"
                        && event["title"]
                            .as_str()
                            .is_some_and(|title| title.starts_with("Approval needed:"))
                    {
                        read_approval(events, raw, event["timeout"].as_u64().unwrap_or(60_000))
                    } else {
                        terminal::read_line(events, raw)
                    }
                })?
            } else {
                response_for(event, |question| {
                    write!(display, "{question}")?;
                    display.flush()?;
                    let mut line = String::new();
                    if fallback.read_line(&mut line)? == 0 {
                        Ok(None)
                    } else {
                        Ok(Some(line.trim_end_matches(['\r', '\n']).to_owned()))
                    }
                })?
            }
        } else {
            writeln!(display, "[pi dialog cancelled: no interactive terminal]")?;
            json!({"cancelled":true})
        };
        let mut reply = json!({"type":"extension_ui_response","id":event["id"]});
        for field in ["cancelled", "confirmed", "value"] {
            if let Some(value) = response.get(field) {
                reply[field] = value.clone();
            }
        }
        writeln!(input, "{reply}").map_err(super::error::transport)?;
        input.flush().map_err(super::error::transport)?;
        Ok(())
    }
}

/// A timed, default-deny reader for tool approval. Ordinary terminal
/// read_line ignores Esc, which must never leave an approval pending.
fn read_approval(
    events: &Receiver<u8>,
    raw: &mut RawMode,
    timeout_ms: u64,
) -> Result<Option<String>> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(60_000));
    let mut choice = None;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        match events.recv_timeout(remaining) {
            Ok(b'\r' | b'\n') => {
                raw.write("\r\n")?;
                return Ok(choice.map(str::to_owned));
            }
            Ok(27 | 3 | 4) => return Ok(None),
            Ok(8 | 127) => {
                choice = None;
                raw.write("\x08 \x08")?;
            }
            Ok(key @ b'1'..=b'3') => {
                choice = Some(match key {
                    b'1' => "1",
                    b'2' => "2",
                    _ => "3",
                });
                raw.write(&char::from(key).to_string())?;
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return Ok(None),
        }
    }
}

fn response_for(
    event: &Value,
    mut ask: impl FnMut(&str) -> Result<Option<String>>,
) -> Result<Value> {
    let title = event["title"].as_str().unwrap_or("Pi request");
    match event["method"].as_str().unwrap_or("") {
        "confirm" => {
            let message = event["message"].as_str().unwrap_or("");
            let answer = ask(&format!("\r\n[pi] {title}: {message} [y/N] "))?;
            Ok(
                json!({"confirmed":answer.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("y") || s.eq_ignore_ascii_case("yes"))}),
            )
        }
        "select" => {
            let Some(options) = event["options"].as_array() else {
                return Ok(json!({"cancelled":true}));
            };
            let mut question = format!("\r\n[pi] {title}\r\n");
            for (i, option) in options.iter().enumerate() {
                question.push_str(&format!(
                    "  {}. {}\r\n",
                    i + 1,
                    option.as_str().unwrap_or("?")
                ));
            }
            question.push_str("Choose a number (Enter to cancel): ");
            let answer = ask(&question)?;
            let selected = answer
                .as_deref()
                .and_then(|s| s.trim().parse::<usize>().ok())
                .and_then(|n| n.checked_sub(1))
                .and_then(|n| options.get(n))
                .and_then(Value::as_str);
            Ok(match selected {
                Some(value) => json!({"value":value}),
                None => json!({"cancelled":true}),
            })
        }
        "input" | "editor" => {
            let hint = event["placeholder"]
                .as_str()
                .or_else(|| event["prefill"].as_str())
                .unwrap_or("");
            let answer = ask(&format!("\r\n[pi] {title} {hint}: "))?;
            Ok(match answer {
                Some(value) if !value.is_empty() => json!({"value":value}),
                _ => json!({"cancelled":true}),
            })
        }
        _ => Ok(json!({"cancelled":true})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approvals_default_to_no_and_select_requires_valid_option() {
        let confirm = json!({"method":"confirm","title":"Run command","message":"rm something"});
        assert_eq!(
            response_for(&confirm, |_| Ok(Some("".into()))).unwrap()["confirmed"],
            false
        );
        assert_eq!(
            response_for(&confirm, |_| Ok(Some("yes".into()))).unwrap()["confirmed"],
            true
        );
        let select = json!({"method":"select","options":["Allow","Block"]});
        assert_eq!(
            response_for(&select, |_| Ok(Some("2".into()))).unwrap()["value"],
            "Block"
        );
        assert_eq!(
            response_for(&select, |_| Ok(Some("3".into()))).unwrap()["cancelled"],
            true
        );
    }

    #[test]
    fn dialog_responses_are_valid_rpc_and_noninteractive_is_safe() {
        let event = json!({"type":"extension_ui_request","id":"abc","method":"confirm","title":"Approve","message":"tool"});
        let mut sent = Vec::new();
        let mut shown = Vec::new();
        Dialogs
            .handle(
                &mut sent,
                &event,
                &mut std::io::Cursor::new(b"yes\n"),
                &mut shown,
                &mut None,
                None,
                false,
            )
            .unwrap();
        let reply: Value = serde_json::from_slice(&sent).unwrap();
        assert_eq!(reply["id"], "abc");
        assert_eq!(reply["cancelled"], true);
        assert!(reply.get("confirmed").is_none());
    }
}
