use crate::{tui::ui::Ui, Result};
use serde_json::Value;
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub(crate) struct SessionInfo {
    pub(crate) path: PathBuf,
    pub(crate) title: String,
    modified: SystemTime,
}

fn session_dir(cwd: &Path) -> Result<PathBuf> {
    if let Some(path) = env::var_os("PI_CODING_AGENT_SESSION_DIR") {
        if !path.is_empty() {
            return Ok(resolve(PathBuf::from(path), cwd));
        }
    }
    let project_settings = cwd.join(".pi/settings.json");
    let agent_dir = match env::var_os("PI_CODING_AGENT_DIR") {
        Some(path) if !path.is_empty() => resolve(PathBuf::from(path), cwd),
        _ => PathBuf::from(env::var_os("HOME").ok_or("HOME is not set")?).join(".pi/agent"),
    };
    for settings in [project_settings, agent_dir.join("settings.json")] {
        match File::open(settings) {
            Ok(file) => {
                let data: Value = serde_json::from_reader(file)?;
                if let Some(dir) = data["sessionDir"].as_str() {
                    if !dir.is_empty() {
                        return Ok(resolve(PathBuf::from(dir), cwd));
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let encoded = cwd
        .to_string_lossy()
        .trim_start_matches('/')
        .replace(['/', '\\', ':'], "-");
    Ok(agent_dir.join("sessions").join(format!("--{encoded}--")))
}

fn resolve(path: PathBuf, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

pub(crate) fn list(cwd: &Path) -> Result<Vec<SessionInfo>> {
    list_in_dir(cwd, &session_dir(cwd)?)
}

fn list_in_dir(cwd: &Path, dir: &Path) -> Result<Vec<SessionInfo>> {
    let mut sessions = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(sessions),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "jsonl") || !entry.file_type()?.is_file() {
            continue;
        }
        // List only sessions with a valid header for this workspace, even when
        // the session directory is shared by multiple projects.
        if let Some(title) = inspect(&path, cwd)? {
            let modified = entry.metadata()?.modified()?;
            sessions.push(SessionInfo {
                path,
                title,
                modified,
            });
        }
    }
    sessions.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(sessions)
}

fn inspect(path: &Path, cwd: &Path) -> Result<Option<String>> {
    let file = File::open(path)?;
    let mut lines = BufReader::new(file).lines();
    let Some(header) = lines.next() else {
        return Ok(None);
    };
    let header = header?;
    let Ok(header) = serde_json::from_str::<Value>(&header) else {
        return Ok(None);
    };
    if header["type"] != "session"
        || !header["id"].is_string()
        || header["cwd"].as_str() != cwd.to_str()
    {
        return Ok(None);
    }
    let mut title = None;
    let mut first = None;
    let mut message_count = 0;
    for line in lines {
        let line = line?;
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if record["type"] == "session_info" {
            title = record["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
        } else if record["type"] == "message" {
            message_count += 1;
            if first.is_none() && record["message"]["role"] == "user" {
                first = text(&record["message"]["content"]);
            }
        }
    }
    if message_count == 0 {
        return Ok(None);
    }
    Ok(Some(
        title.or(first).unwrap_or_else(|| "(no messages)".into()),
    ))
}

fn text(content: &Value) -> Option<String> {
    match content {
        Value::String(value) => Some(value.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| match part["type"].as_str() {
                    Some("text") => part["text"].as_str(),
                    Some("image") => Some("[image attached]"),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

pub(crate) fn pick<R: BufRead, W: Write>(
    sessions: &[SessionInfo],
    input: &mut R,
    output: &mut W,
    interactive: bool,
) -> Result<Option<PathBuf>> {
    pick_with(sessions, output, |display| {
        if interactive {
            write!(display, "Choose a session number (Enter to cancel): ")?;
            display.flush()?;
        }
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            Ok(None)
        } else {
            Ok(Some(line.trim().to_owned()))
        }
    })
}

pub(crate) fn pick_with<W: Write>(
    sessions: &[SessionInfo],
    output: &mut W,
    mut ask: impl FnMut(&mut W) -> Result<Option<String>>,
) -> Result<Option<PathBuf>> {
    if sessions.is_empty() {
        writeln!(output, "No saved sessions for this directory.")?;
        return Ok(None);
    }
    writeln!(
        output,
        "Sessions for this directory (most recently updated first):"
    )?;
    for (i, session) in sessions.iter().enumerate() {
        let title = session.title.replace(['\r', '\n'], " ");
        let preview = title.chars().take(80).collect::<String>();
        writeln!(output, "  {}. {}", i + 1, preview)?;
    }
    let line = ask(output)?;
    let Some(line) = line else { return Ok(None) };
    if line.trim().is_empty() {
        return Ok(None);
    }
    let number: usize = line
        .trim()
        .parse()
        .map_err(|_| "enter a session number from the list")?;
    let session = sessions
        .get(number.checked_sub(1).ok_or("invalid session number")?)
        .ok_or("invalid session number")?;
    Ok(Some(session.path.clone()))
}

#[cfg(test)]
pub(crate) fn show_recent<W: Write>(messages: &[Value], output: &mut W) -> Result<()> {
    show_recent_with(messages, output, &Ui::new(false))
}

pub(crate) fn show_recent_with<W: Write>(
    messages: &[Value],
    output: &mut W,
    ui: &Ui,
) -> Result<()> {
    // Select the last five user turns, then show only visible user/assistant text
    // from those turns. Tool, system, summary and thinking content stays hidden.
    let starts: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(i, msg)| (msg["role"] == "user").then_some(i))
        .collect();
    let Some(&start) = starts.iter().rev().take(5).next_back() else {
        return Ok(());
    };
    let mut shown = false;
    for message in &messages[start..] {
        let label = match message["role"].as_str() {
            Some("user") => "you",
            Some("assistant") => "assistant",
            _ => continue,
        };
        if let Some(content) = text(&message["content"]) {
            if !content.is_empty() {
                if ui.interactive {
                    if label == "you" {
                        ui.user(output)?;
                    } else {
                        ui.assistant(output)?;
                    }
                    writeln!(output, "{content}")?;
                } else {
                    writeln!(output, "{label}> {content}")?;
                }
                shown = true;
            }
        }
    }
    if shown {
        writeln!(output)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn history_shows_image_placeholders_without_base64() {
        let mut output = Vec::new();
        show_recent(&[json!({"role":"user","content":[{"type":"image","mimeType":"image/png","data":"private-image-bytes"}]})], &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("you> [image attached]"));
        assert!(!output.contains("private-image-bytes"));
    }

    #[test]
    fn interactive_history_preserves_user_and_agent_roles() {
        let messages = vec![
            json!({"role":"user","content":"question\ncontinued"}),
            json!({"role":"assistant","content":"answer"}),
        ];
        let mut output = Vec::new();
        show_recent_with(&messages, &mut output, &Ui::new(true)).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("you ›"), "{output}");
        assert!(output.contains("hibi ›"), "{output}");
        assert_eq!(output.matches("you ›").count(), 1);
        assert_eq!(output.matches("hibi ›").count(), 1);
    }

    #[test]
    fn shows_only_last_five_user_turns_and_their_replies() {
        let mut messages = vec![json!({"role":"system","content":"secret"})];
        for i in 0..7 {
            messages.push(json!({"role":"user","content":format!("question {i}")}));
            messages.push(json!({"role":"assistant","content":[{"type":"thinking","thinking":"hidden"},{"type":"text","text":format!("answer {i}")},{"type":"toolCall","name":"bash"}]}));
            messages
                .push(json!({"role":"toolResult","content":[{"type":"text","text":"private"}]}));
        }
        let mut output = Vec::new();
        show_recent(&messages, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("question 1"));
        assert!(!output.contains("answer 1"));
        assert!(output.contains("you> question 2\nassistant> answer 2"));
        assert!(output.contains("you> question 6\nassistant> answer 6"));
        assert!(!output.contains("secret"));
        assert!(!output.contains("private"));
        assert!(!output.contains("hidden"));
    }

    #[test]
    fn lists_only_this_directory_and_sorts_newest_first() {
        let root =
            std::env::temp_dir().join(format!("hibiscus-sessions-test-{}", std::process::id()));
        let cwd = root.join("project");
        let dir = root.join("sessions");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.jsonl");
        let b = dir.join("b.jsonl");
        fs::write(
            &a,
            format!(
                "{}\n{}\n",
                json!({"type":"session","id":"a","cwd":cwd}),
                json!({"type":"message","message":{"role":"user","content":"first"}})
            ),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(
            &b,
            format!(
                "{}\n{}\n",
                json!({"type":"session","id":"b","cwd":cwd}),
                json!({"type":"message","message":{"role":"user","content":"second"}})
            ),
        )
        .unwrap();
        fs::write(
            dir.join("other.jsonl"),
            format!(
                "{}\n{}\n",
                json!({"type":"session","id":"other","cwd":"/unrelated"}),
                json!({"type":"message","message":{"role":"user","content":"private"}})
            ),
        )
        .unwrap();
        fs::write(
            dir.join("empty.jsonl"),
            format!("{}\n", json!({"type":"session","id":"empty","cwd":cwd})),
        )
        .unwrap();
        let found = list_in_dir(&cwd, &dir).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].title, "second");
        assert_eq!(found[1].title, "first");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn picker_handles_cancel_and_selection() {
        let sessions = vec![SessionInfo {
            path: PathBuf::from("a.jsonl"),
            title: "one".into(),
            modified: SystemTime::UNIX_EPOCH,
        }];
        assert_eq!(
            pick(&sessions, &mut b"1\n".as_slice(), &mut Vec::new(), false).unwrap(),
            Some(PathBuf::from("a.jsonl"))
        );
        assert!(
            pick(&sessions, &mut b"\n".as_slice(), &mut Vec::new(), false)
                .unwrap()
                .is_none()
        );
        assert!(pick(&sessions, &mut b"2\n".as_slice(), &mut Vec::new(), false).is_err());
    }
}
