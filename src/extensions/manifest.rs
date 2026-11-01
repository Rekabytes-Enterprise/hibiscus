use crate::Result;
use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashSet, io::Read, path::Path, sync::Arc};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Manifest {
    pub api_version: u32,
    pub id: String,
    pub description: String,
    pub command: Vec<String>,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub commands: Vec<Command>,
    #[serde(default)]
    pub checks: Vec<super::validation::Check>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Command {
    #[serde(default, rename = "slashCommand")]
    pub slash_command: Option<String>,
    pub name: String,
    pub description: String,
}
fn timeout() -> u64 {
    120
}
pub(super) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 28
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 1000 && !value.chars().any(char::is_control)
}
pub(super) fn read(directory: &Path) -> Result<(Manifest, Vec<Arc<jsonschema::Validator>>)> {
    let path = directory.join("extension.json");
    if !std::fs::metadata(&path)?.is_file() {
        return Err("extension.json must be a regular file".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    parse(&bytes)
}
pub(super) fn parse(bytes: &[u8]) -> Result<(Manifest, Vec<Arc<jsonschema::Validator>>)> {
    if bytes.len() > 65536 {
        return Err("extension.json exceeds 64 KiB".into());
    }
    if !serde_json::from_slice::<Value>(bytes).is_ok_and(|value| value.is_object()) {
        return Err("extension.json must be a JSON object".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|_| "Invalid extension.json fields or JSON")?;
    if manifest.api_version != 1 {
        return Err("Unsupported extension apiVersion; expected 1".into());
    }
    if !identifier(&manifest.id) || !label(&manifest.description) {
        return Err("Invalid extension ID or description".into());
    }
    if manifest.command.is_empty()
        || manifest.command.len() > 32
        || manifest
            .command
            .iter()
            .any(|a| a.is_empty() || a.len() > 4096 || a.chars().any(char::is_control))
    {
        return Err(
            "command must contain 1–32 nonempty arguments without control characters".into(),
        );
    }
    if !(1..=600).contains(&manifest.timeout_seconds) {
        return Err("timeoutSeconds must be 1–600".into());
    }
    if manifest.tools.len() > 32
        || manifest.commands.len() > 32
        || manifest.tools.is_empty() && manifest.commands.is_empty()
    {
        return Err("Extension must define tools or commands, at most 32 each".into());
    }
    let mut names = HashSet::new();
    let mut validators = Vec::new();
    for tool in &manifest.tools {
        if !identifier(&tool.name) || !label(&tool.description) || !names.insert(&tool.name) {
            return Err("Invalid or duplicate extension tool name/description".into());
        }
        if tool.parameters["type"] != "object" {
            return Err("Tool parameters must have type object".into());
        }
        // Schemas are compiled once. Network/file resolver features are disabled;
        // use the linear-time regex engine for user-supplied patterns.
        let validator = jsonschema::draft7::options()
            .with_pattern_options(jsonschema::PatternOptions::regex())
            .build(&tool.parameters)
            .map_err(|_| "Invalid tool parameters schema; external references are unavailable")?;
        validators.push(Arc::new(validator));
    }
    names.clear();
    for command in &manifest.commands {
        if !identifier(&command.name)
            || !label(&command.description)
            || !names.insert(&command.name)
        {
            return Err("Invalid or duplicate extension command name/description".into());
        }
        if command
            .slash_command
            .as_deref()
            .is_some_and(|name| !slash_name(name))
        {
            return Err("slashCommand must start with / and a lowercase letter, with at most 64 letters, digits, underscores or hyphens".into());
        }
    }
    super::validation::declarations(&manifest.checks)?;
    Ok((manifest, validators))
}

pub(super) fn slash_name(name: &str) -> bool {
    name.strip_prefix('/').is_some_and(|name| {
        !name.is_empty()
            && name.len() <= 64
            && name.as_bytes()[0].is_ascii_lowercase()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
    })
}
impl Command {
    pub(super) fn public_name(&self, id: &str) -> String {
        self.slash_command
            .clone()
            .unwrap_or_else(|| format!("/{id}:{}", self.name))
    }
}
