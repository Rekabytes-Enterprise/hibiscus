//! User-owned executable extensions. Discovery reads metadata without running code.
mod agent;
mod manifest;
pub(crate) use agent::{definitions as agent_definitions, AgentTool, Catalog};
mod process;
#[cfg(test)]
mod tests;

use crate::Result;
pub(crate) use process::Exit;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::watch;

struct Extension {
    id: String,
    description: String,
    directory: PathBuf,
    command: Vec<String>,
    timeout: u64,
}
struct Tool {
    extension: Arc<Extension>,
    name: String,
    definition: Value,
    validator: Arc<jsonschema::Validator>,
}
struct Command {
    extension: Arc<Extension>,
    name: String,
    description: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct CommandInfo {
    pub(crate) name: String,
    pub(crate) description: String,
}
#[derive(Default)]
pub(crate) struct Registry {
    root: Option<PathBuf>,
    extensions: BTreeMap<String, Arc<Extension>>,
    tools: BTreeMap<String, Tool>,
    commands: BTreeMap<String, Command>,
    issues: Vec<String>,
}
/// Classify namespace-shaped input before registry lookup, including typos.
/// Absolute file paths with another slash stay ordinary prompt text.
pub(crate) fn is_command_token(name: &str) -> bool {
    name.strip_prefix('/')
        .is_some_and(|name| name.contains(':') && !name.contains('/'))
}

impl Registry {
    pub(crate) fn discover() -> Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let root = crate::paths::home()?.join("extensions");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)?;
        Self::load(&root)
    }
    fn load(root: &Path) -> Result<Self> {
        let mut registry = Self {
            root: Some(root.to_path_buf()),
            ..Self::default()
        };
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(registry),
            Err(_) => return Err("Cannot read the Hibiscus extensions directory".into()),
        };
        let mut directories = Vec::new();
        for (index, entry) in entries.take(129).enumerate() {
            if index == 128 {
                registry
                    .issues
                    .push("Extension scan reached its 128-entry limit".into());
                break;
            }
            let entry = entry.map_err(|_| "Cannot read an extension directory entry")?;
            if entry.path().is_dir() {
                directories.push(entry.path());
            }
        }
        directories.sort();
        if directories.len() > 32 {
            registry
                .issues
                .push("Only the first 32 extension directories are considered".into());
            directories.truncate(32);
        }
        for directory in directories {
            if let Err(e) = registry.add(&directory) {
                let name = directory
                    .file_name()
                    .and_then(|n| n.to_str())
                    .filter(|n| manifest::identifier(n))
                    .unwrap_or("unnamed");
                registry.issues.push(format!(
                    "{name}: {}",
                    super::runtime::error::safe_message(&e.to_string())
                ));
            }
        }
        Ok(registry)
    }
    fn add(&mut self, directory: &Path) -> Result<()> {
        let (manifest, validators) = manifest::read(directory)?;
        if directory.file_name().and_then(|n| n.to_str()) != Some(&manifest.id) {
            return Err("Directory name must match the extension ID".into());
        }
        self.check_registration(&manifest)?;
        let tool_names = manifest
            .tools
            .iter()
            .map(|t| format!("{}__{}", manifest.id, t.name))
            .collect::<Vec<_>>();
        let directory = directory.canonicalize()?;
        let dir = directory
            .to_str()
            .ok_or("Extension directory must be UTF-8")?;
        let mut command = manifest
            .command
            .iter()
            .map(|arg| arg.replace("{extensionDir}", dir))
            .collect::<Vec<_>>();
        if command[0].contains('/') && !Path::new(&command[0]).is_absolute() {
            command[0] = directory
                .join(&command[0])
                .to_str()
                .ok_or("Invalid extension executable path")?
                .into();
        }
        let extension = Arc::new(Extension {
            id: manifest.id.clone(),
            description: manifest.description,
            directory,
            command,
            timeout: manifest.timeout_seconds,
        });
        // Publish registrations only after the entire manifest and namespace pass.
        for ((tool, name), validator) in manifest.tools.into_iter().zip(tool_names).zip(validators)
        {
            self.tools.insert(name.clone(),Tool {extension:extension.clone(),name:tool.name,definition:json!({"name":name,"description":tool.description,"parameters":tool.parameters}),validator});
        }
        for command in manifest.commands {
            self.commands.insert(
                command
                    .public_name(&extension.id)
                    .trim_start_matches('/')
                    .to_owned(),
                Command {
                    extension: extension.clone(),
                    name: command.name,
                    description: command.description,
                },
            );
        }
        self.extensions.insert(extension.id.clone(), extension);
        Ok(())
    }
    fn check_registration(&self, manifest: &manifest::Manifest) -> Result<()> {
        if self.extensions.contains_key(&manifest.id) {
            return Err("An extension with this ID already exists".into());
        }
        if self.tools.len() + manifest.tools.len() + crate::runtime::native_tool_count() > 128 {
            return Err("Combined native/extension catalog exceeds 128 tools".into());
        }
        if self.commands.len() + manifest.commands.len() > 128 {
            return Err("Extension commands exceed the 128-command limit".into());
        }
        if manifest.tools.iter().any(|tool| {
            self.tools
                .contains_key(&format!("{}__{}", manifest.id, tool.name))
        }) {
            return Err("Extension tool namespace collision".into());
        }
        let mut commands = std::collections::HashSet::new();
        for command in &manifest.commands {
            let name = command.public_name(&manifest.id);
            if crate::commands::reserved(&name)
                || self.commands.contains_key(name.trim_start_matches('/'))
                || !commands.insert(name)
            {
                return Err(
                    "Extension slash command collides with a native or registered command".into(),
                );
            }
        }
        Ok(())
    }
    pub(crate) fn unavailable(message: &str) -> Self {
        Self {
            issues: vec![format!(
                "Extension discovery failed: {}",
                crate::runtime::error::safe_message(message)
            )],
            ..Self::default()
        }
    }
    pub(crate) fn warnings(&self) -> &[String] {
        &self.issues
    }
    pub(crate) fn definitions(&self) -> impl Iterator<Item = &Value> {
        self.tools.values().map(|t| &t.definition)
    }
    pub(crate) fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
    pub(crate) fn commands(&self) -> Vec<CommandInfo> {
        self.commands
            .iter()
            .map(|(name, c)| CommandInfo {
                name: format!("/{name}"),
                description: c.description.clone(),
            })
            .collect()
    }
    pub(crate) fn report(&self) -> Value {
        let extensions=self.extensions.values().map(|e|json!({"id":e.id,"description":e.description,"tools":self.tools.iter().filter(|(_,t)|t.extension.id==e.id).map(|(name,_)|name).collect::<Vec<_>>(),"commands":self.commands.iter().filter(|(_,c)|c.extension.id==e.id).map(|(name,_)|format!("/{name}")).collect::<Vec<_>>()})).collect::<Vec<_>>();
        json!({"directory":self.root,"extensions":extensions,"errors":self.issues})
    }
    pub(crate) async fn execute_tool(
        &self,
        id: &str,
        name: &str,
        args: &Value,
        context: (&Path, &str),
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Value> {
        let tool = self.tools.get(name).ok_or("Unknown extension tool")?;
        if args.to_string().len() > 65536 || !tool.validator.is_valid(args) {
            return Err(
                "Extension tool arguments do not match its parameters schema or exceed 64 KiB"
                    .into(),
            );
        }
        let reply = process::invoke(
            &tool.extension,
            id,
            "tool",
            &tool.name,
            args,
            context,
            cancel,
        )
        .await?;
        Ok(
            json!({"content":[{"type":"text","text":reply.text}],"isError":reply.is_error,"details":{"extension":tool.extension.id,"exitCode":0,"data":reply.data}}),
        )
    }
    pub(crate) async fn execute_command(
        &self,
        id: &str,
        name: &str,
        input: &str,
        context: (&Path, &str),
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Value> {
        let name = name.strip_prefix('/').unwrap_or(name);
        let command = self
            .commands
            .get(name)
            .ok_or("Unknown extension command. Ask the agent to inspect loaded extensions.")?;
        if input.len() > 65536 {
            return Err("Extension command input exceeds 64 KiB".into());
        }
        let reply = process::invoke(
            &command.extension,
            id,
            "command",
            &command.name,
            &json!({"input":input}),
            context,
            cancel,
        )
        .await?;
        Ok(json!({"text":reply.text,"isError":reply.is_error}))
    }
}
