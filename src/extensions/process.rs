use super::Extension;
use crate::{process, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, time::Duration};
use tokio::{process::Command, sync::watch};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Request<'a> {
    api_version: u32,
    id: &'a str,
    kind: &'a str,
    name: &'a str,
    arguments: &'a Value,
    cwd: &'a Path,
    extension_dir: &'a Path,
    session_id: &'a str,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Reply {
    api_version: u32,
    id: String,
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) is_error: bool,
    #[serde(default)]
    pub(crate) data: Value,
}
#[derive(Debug)]
pub(crate) struct Exit {
    pub(crate) code: Option<i32>,
    text: String,
}
impl std::fmt::Display for Exit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Exit {}
pub(super) async fn invoke(
    extension: &Extension,
    id: &str,
    kind: &str,
    name: &str,
    arguments: &Value,
    context: (&Path, &str),
    cancel: &mut watch::Receiver<bool>,
) -> Result<Reply> {
    let (cwd, session_id) = context;
    let request = Request {
        api_version: 1,
        id,
        kind,
        name,
        arguments,
        cwd,
        extension_dir: &extension.directory,
        session_id,
    };
    let mut input = serde_json::to_vec(&request)?;
    if input.len() > 131072 {
        return Err("Extension request exceeds 128 KiB".into());
    }
    input.push(b'\n');
    let mut command = Command::new(&extension.command[0]);
    command
        .args(&extension.command[1..])
        .current_dir(cwd)
        .env("HIBISCUS_EXTENSION_DIR", &extension.directory)
        .env("HIBISCUS_WORKSPACE", cwd);
    let output = process::run(
        command,
        Some(input),
        Duration::from_secs(extension.timeout),
        cancel,
    )
    .await?;
    if !output.status.success() {
        let stderr = clean(&String::from_utf8_lossy(&output.stderr));
        return Err(Exit {
            code: output.status.code(),
            text: format!(
                "Extension {} exited with {}\n{stderr}",
                extension.id, output.status
            ),
        }
        .into());
    }
    let mut reply: Reply = serde_json::from_slice(&output.stdout).map_err(|_| {
        "Extension must write one valid JSON reply to stdout; diagnostics belong on stderr"
    })?;
    if reply.api_version != 1 || reply.id != id {
        return Err("Extension reply has an unsupported version or mismatched request ID".into());
    }
    reply.text = clean(&reply.text);
    Ok(reply)
}
fn clean(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}
