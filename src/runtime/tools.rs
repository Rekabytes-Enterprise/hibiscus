use crate::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command, sync::watch};

pub fn definitions() -> Vec<Value> {
    vec![
        json!({"name":"read","description":"Read a UTF-8 text file. Offset and limit are one-based line bounds. Output is bounded.","parameters":{"type":"object","properties":{"path":{"type":"string"},"offset":{"type":"integer","minimum":1},"limit":{"type":"integer","minimum":1}},"required":["path"],"additionalProperties":false}}),
        json!({"name":"write","description":"Write UTF-8 contents to a file. Creates parent directories. Replaces existing contents.","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}}),
        json!({"name":"edit","description":"Replace exactly one occurrence of oldText with newText. Fails if missing or ambiguous.","parameters":{"type":"object","properties":{"path":{"type":"string"},"oldText":{"type":"string"},"newText":{"type":"string"}},"required":["path","oldText","newText"],"additionalProperties":false}}),
        json!({"name":"bash","description":"Run a POSIX /bin/sh command in the workspace. Output and duration are bounded. Shell execution requires approval when enabled.","parameters":{"type":"object","properties":{"command":{"type":"string"},"timeout":{"type":"integer","minimum":1,"maximum":600}},"required":["command"],"additionalProperties":false}}),
        json!({"name":"goal","description":"Track an explicit checklist. Set the list before work, complete a one-based step only when finished, or clear it.","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["set","complete","clear"]},"steps":{"type":"array","items":{"type":"string"}},"id":{"type":"integer"}},"required":["action"],"additionalProperties":false}}),
    ]
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Write {
    path: String,
    content: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Edit {
    path: String,
    old_text: String,
    new_text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bash {
    command: String,
    timeout: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Goal {
    action: String,
    steps: Option<Vec<String>>,
    id: Option<usize>,
}
fn path(cwd: &Path, path: &str) -> Result<PathBuf> {
    if path.is_empty() {
        return Err("Empty file path".into());
    }
    let p = PathBuf::from(path);
    Ok(if p.is_absolute() { p } else { cwd.join(p) })
}
fn read_file(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() || meta.len() > 16 * 1024 * 1024 {
        return Err("Expected a regular UTF-8 file no larger than 16 MiB".into());
    }
    Ok(std::fs::read_to_string(path)?)
}
fn result(text: String, details: Value) -> Value {
    json!({"content":[{"type":"text","text":text}],"details":details})
}
pub fn validate(name: &str, args: &Value) -> Result<()> {
    match name {
        "read" => {
            let a: Read = serde_json::from_value(args.clone())?;
            if a.path.is_empty() || a.offset == Some(0) || a.limit == Some(0) {
                return Err("Invalid read bounds/path".into());
            }
        }
        "write" => {
            let a: Write = serde_json::from_value(args.clone())?;
            if a.path.is_empty() || a.content.len() > 16 * 1024 * 1024 {
                return Err("Invalid write path/content size".into());
            }
        }
        "edit" => {
            let a: Edit = serde_json::from_value(args.clone())?;
            if a.path.is_empty() || a.old_text.is_empty() || a.new_text.len() > 16 * 1024 * 1024 {
                return Err("Invalid edit arguments".into());
            }
        }
        "bash" => {
            let a: Bash = serde_json::from_value(args.clone())?;
            if a.command.trim().is_empty()
                || a.command.len() > 65536
                || !(1..=600).contains(&a.timeout.unwrap_or(120))
            {
                return Err("Invalid command or timeout".into());
            }
        }
        "goal" => {
            let _: Goal = serde_json::from_value(args.clone())?;
        }
        _ => return Err("Unknown tool".into()),
    }
    Ok(())
}
pub async fn execute(
    name: &str,
    args: &Value,
    cwd: &Path,
    steps: &mut Vec<Value>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Value> {
    validate(name, args)?;
    if *cancel.borrow() {
        return Err("Tool cancelled before execution".into());
    }
    match name {
        "read" => {
            let a: Read = serde_json::from_value(args.clone())?;
            let text = read_file(&path(cwd, &a.path)?)?;
            let lines: Vec<_> = text.lines().collect();
            let offset = a.offset.unwrap_or(1) - 1;
            let end = (offset.saturating_add(a.limit.unwrap_or(2000))).min(lines.len());
            if offset >= lines.len() && !lines.is_empty() {
                return Err("Read offset is past end of file".into());
            }
            let selected = lines.get(offset..end).unwrap_or_default().join("\n");
            let (short, truncated) = bounded(&selected, 65536);
            Ok(result(
                format!(
                    "{short}{}",
                    if truncated || end < lines.len() {
                        "\n[Output truncated. Read another line range.]"
                    } else {
                        ""
                    }
                ),
                json!({"totalLines":lines.len()}),
            ))
        }
        "write" => {
            let a: Write = serde_json::from_value(args.clone())?;
            let target = path(cwd, &a.path)?;
            write_file(&target, a.content.as_bytes())?;
            Ok(result(
                format!("Wrote {} bytes to {}", a.content.len(), a.path),
                json!({}),
            ))
        }
        "edit" => {
            let a: Edit = serde_json::from_value(args.clone())?;
            let target = path(cwd, &a.path)?;
            let old = read_file(&target)?;
            if old.matches(&a.old_text).count() != 1 {
                return Err(
                    "Edit requires exactly one occurrence of oldText; file unchanged".into(),
                );
            }
            let next = old.replacen(&a.old_text, &a.new_text, 1);
            if next.len() > 16 * 1024 * 1024 {
                return Err("Edited file would exceed 16 MiB; file unchanged".into());
            }
            let first = old
                .find(&a.old_text)
                .map(|i| old[..i].bytes().filter(|c| *c == b'\n').count() + 1)
                .unwrap();
            let diff = format!(
                "@@ line {first} @@\n{}\n{}",
                a.old_text
                    .lines()
                    .map(|l| format!("-{l}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                a.new_text
                    .lines()
                    .map(|l| format!("+{l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            write_file(&target, next.as_bytes())?;
            Ok(result(
                format!("Edited {}", a.path),
                json!({"diff":bounded(&diff,65536).0}),
            ))
        }
        "bash" => {
            let a: Bash = serde_json::from_value(args.clone())?;
            let mut cmd = Command::new("/bin/sh");
            cmd.args(["-c", &a.command])
                .current_dir(cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .process_group(0);
            let mut child = cmd.spawn()?;
            let group = Group(child.id().ok_or("Shell PID unavailable")?);
            let stdout = child.stdout.take().ok_or("Shell stdout unavailable")?;
            let stderr = child.stderr.take().ok_or("Shell stderr unavailable")?;
            let running = async {
                let mut out = vec![];
                let mut err = vec![];
                let mut stdout = stdout.take(65537);
                let mut stderr = stderr.take(65537);
                let (a, b) =
                    tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
                a?;
                b?;
                if out.len() > 65536 || err.len() > 65536 {
                    return Err(
                        "Shell output exceeded 64 KiB per stream; command terminated".into(),
                    );
                }
                let status = child.wait().await?;
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out),
                    String::from_utf8_lossy(&err)
                );
                if !status.success() {
                    return Err(
                        format!("Shell exited with {status}\n{}", bounded(&text, 65536).0).into(),
                    );
                }
                Ok(result(text, json!({"exitCode":status.code()})))
            };
            let outcome = tokio::select! {r=tokio::time::timeout(Duration::from_secs(a.timeout.unwrap_or(120)),running)=>match r{Ok(r)=>r,Err(_)=>Err("Shell timed out; command group terminated".into())},_=cancel.changed()=>Err("Shell cancelled; command group terminated".into())};
            drop(group);
            let _ = child.start_kill();
            let _ = child.wait().await;
            outcome
        }
        "goal" => {
            let a: Goal = serde_json::from_value(args.clone())?;
            match a.action.as_str() {
                "set" => {
                    let labels = a.steps.ok_or("set needs steps")?;
                    let mut seen = std::collections::HashSet::new();
                    if labels.is_empty()
                        || labels.len() > 50
                        || labels
                            .iter()
                            .any(|s| s.trim().is_empty() || s.len() > 200 || !seen.insert(s))
                    {
                        return Err(
                            "Set needs 1–50 distinct named steps, each at most 200 bytes".into(),
                        );
                    }
                    *steps = labels
                        .into_iter()
                        .map(|label| json!({"label":label,"done":false}))
                        .collect();
                }
                "complete" => {
                    let index =
                        a.id.and_then(|n| n.checked_sub(1))
                            .filter(|i| *i < steps.len())
                            .ok_or("Invalid step number")?;
                    if steps[index]["done"] == true {
                        return Err("Step already complete".into());
                    }
                    steps[index]["done"] = json!(true);
                }
                "clear" => steps.clear(),
                _ => return Err("Unknown goal action".into()),
            }
            let completed = steps.iter().filter(|s| s["done"] == true).count();
            Ok(result(
                format!("Goal: {completed}/{} steps completed", steps.len()),
                json!({"hibiscusGoal":{"steps":steps,"completed":completed,"total":steps.len()}}),
            ))
        }
        _ => unreachable!("validated tool"),
    }
}
struct Group(u32);
impl Drop for Group {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}
fn bounded(text: &str, max: usize) -> (String, bool) {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].into(), end < text.len())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let permissions = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Some(meta.permissions()),
        Ok(_) => return Err("Refusing to replace a non-regular file or symbolic link".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    super::config::atomic(path, bytes)?;
    if let Some(permissions) = permissions {
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}
