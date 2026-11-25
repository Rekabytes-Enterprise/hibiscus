//! Normalize completed tool calls without guessing argument values.
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(super) struct Diagnostic {
    pub(super) notes: Vec<String>,
    pub(super) error: Option<String>,
    pub(super) malformed: Option<[u8; 32]>,
}

pub(super) fn decode(name: &mut String, raw: &str) -> (Value, Diagnostic) {
    let mut diagnostic = Diagnostic::default();
    let args = match serde_json::from_str(raw) {
        Ok(args) => args,
        Err(original) => {
            let (unwrapped, fenced) = unwrap_fence(raw);
            let cleaned = strip_trailing_commas(unwrapped);
            let repaired = if cleaned == unwrapped {
                unwrapped
            } else {
                &cleaned
            };
            match serde_json::from_str(repaired) {
                Ok(args) => {
                    if fenced {
                        diagnostic.notes.push("removed the JSON code fence".into());
                    }
                    if cleaned != unwrapped {
                        diagnostic.notes.push("removed trailing JSON commas".into());
                    }
                    args
                }
                Err(_) => {
                    diagnostic.malformed = Some(Sha256::digest(raw.as_bytes()).into());
                    diagnostic.error = Some(format!(
                        "Invalid tool argument JSON: {original}. Resend a complete JSON object."
                    ));
                    Value::Object(Map::new())
                }
            }
        }
    };
    (normalize(name, args, &mut diagnostic), diagnostic)
}

pub(super) fn normalize(name: &mut String, mut args: Value, diagnostic: &mut Diagnostic) -> Value {
    if diagnostic.error.is_some() {
        return args;
    }
    let Some(fields) = args.as_object_mut() else {
        diagnostic.malformed = Some(Sha256::digest(args.to_string().as_bytes()).into());
        diagnostic.error =
            Some("Tool arguments must be a JSON object. Resend with named arguments.".into());
        // The placeholder is for provider history. The executor always returns the error.
        return Value::Object(Map::new());
    };
    let canonical = match name.as_str() {
        "read_file" => "read",
        "edit_file" => "edit",
        "write_file" => "write",
        "run_shell" | "shell" => "bash",
        _ => name,
    };
    if canonical != name {
        diagnostic
            .notes
            .push(format!("{name} was read as {canonical}"));
        *name = canonical.to_owned();
    }
    let aliases: &[(&str, &[&str])] = match name.as_str() {
        "read" | "write" => &[("path", &["file_path", "filepath", "file"])],
        "edit" => &[
            ("path", &["file_path", "filepath", "file"]),
            ("oldText", &["old_string", "old_text"]),
            ("newText", &["new_string", "new_text"]),
        ],
        "bash" => &[("command", &["cmd"])],
        _ => &[],
    };
    for (key, alternatives) in aliases {
        let mut present = alternatives
            .iter()
            .filter(|alias| fields.contains_key(**alias));
        let Some(alias) = present.next() else {
            continue;
        };
        if fields.contains_key(*key) || present.next().is_some() {
            diagnostic.error = Some(format!(
                "Multiple arguments describe {key}. Send only {key}."
            ));
            return args;
        }
        if let Some(value) = fields.remove(*alias) {
            fields.insert((*key).into(), value);
            diagnostic.notes.push(format!("{alias} was read as {key}"));
        }
    }
    let integers: &[&str] = match name.as_str() {
        "read" => &["offset", "limit"],
        "bash" => &["timeout"],
        _ => &[],
    };
    for key in integers {
        if let Some(value) = fields.get(*key).and_then(Value::as_str) {
            if let Ok(number) = value.parse::<i64>() {
                if number.to_string() == value {
                    fields.insert((*key).into(), Value::from(number));
                    diagnostic
                        .notes
                        .push(format!("{key} was read as an integer"));
                }
            }
        }
    }
    args
}

fn unwrap_fence(raw: &str) -> (&str, bool) {
    let raw = raw.trim();
    if let Some((fence, body)) = raw.split_once('\n') {
        if matches!(fence.trim_end(), "```" | "```json") {
            if let Some(body) = body.strip_suffix("```").filter(|body| body.ends_with('\n')) {
                return (body, true);
            }
        }
    }
    (raw, false)
}

fn strip_trailing_commas(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut chars = raw.chars().peekable();
    let mut previous = None;
    while let Some(ch) = chars.next() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
                previous = Some(ch);
            }
        } else if ch == '"' {
            quoted = true;
        } else if ch == ',' && !matches!(previous, None | Some('{' | '[' | ':' | ',')) {
            let mut space = String::new();
            while let Some(ws) = chars.next_if(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) {
                space.push(ws);
            }
            if matches!(chars.peek(), Some('}' | ']')) {
                output.push_str(&space);
                continue;
            }
            output.push(ch);
            output.push_str(&space);
            previous = Some(ch);
            continue;
        } else if !ch.is_ascii_whitespace() {
            previous = Some(ch);
        }
        output.push(ch);
    }
    output
}
