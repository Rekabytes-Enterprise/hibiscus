//! Native tools for agent-authored extensions. No user-facing setup commands.
use super::*;
use serde::Deserialize;
use std::{
    collections::HashSet,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Component,
    sync::RwLock,
};

#[derive(Clone, Copy)]
pub(crate) enum AgentTool {
    Create,
    List,
    Reload,
}
impl AgentTool {
    const ALL: [Self; 3] = [Self::Create, Self::List, Self::Reload];
    fn name(self) -> &'static str {
        match self {
            Self::Create => "create_extension",
            Self::List => "list_extensions",
            Self::Reload => "reload_extensions",
        }
    }
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    manifest: Value,
    #[serde(default)]
    files: Vec<SourceFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFile {
    path: String,
    content: String,
    #[serde(default)]
    executable: bool,
}

/// Each provider response executes against a snapshot. Agent changes publish a
/// new catalog for the next provider step and the next user command.
pub(crate) struct Catalog {
    current: RwLock<Arc<Registry>>,
}
impl Catalog {
    pub(crate) fn new(registry: Registry) -> Self {
        Self {
            current: RwLock::new(Arc::new(registry)),
        }
    }
    pub(crate) fn snapshot(&self) -> Arc<Registry> {
        self.current.read().unwrap().clone()
    }
    fn reload(&self) -> Result<Arc<Registry>> {
        let registry = Arc::new(Registry::discover()?);
        *self.current.write().unwrap() = registry.clone();
        Ok(registry)
    }
    pub(crate) fn execute(&self, tool: AgentTool, args: &Value) -> Result<Value> {
        let (report, directory) = match tool {
            AgentTool::List => {
                empty(args)?;
                (self.snapshot().report(), None)
            }
            AgentTool::Reload => {
                empty(args)?;
                (self.reload()?.report(), None)
            }
            AgentTool::Create => {
                let root = crate::paths::home()?.join("extensions");
                // Validate against current files too: a preceding native edit
                // may have changed another manifest since the model snapshot.
                let latest = Registry::load(&root)?;
                let directory = create_at(&root, args, &latest)?;
                let registry = self.reload()?;
                let id = directory
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("Invalid extension directory")?;
                if !registry.extensions.contains_key(id) {
                    return Err("Extension files were created, but registration failed. Inspect list_extensions errors before editing or retrying.".into());
                }
                (registry.report(), Some(directory))
            }
        };
        let text = if let Some(directory) = &directory {
            format!(
                "Created extension at {}. Its tools and slash commands are registered.\n{}",
                directory.display(),
                serde_json::to_string_pretty(&report)?
            )
        } else {
            serde_json::to_string_pretty(&report)?
        };
        Ok(
            json!({"content":[{"type":"text","text":text}],"details":{"directory":directory,"catalog":report}}),
        )
    }
}
fn empty(args: &Value) -> Result<()> {
    if args.as_object().is_some_and(|object| object.is_empty()) {
        Ok(())
    } else {
        Err("Expected an empty arguments object".into())
    }
}
pub(super) fn create_at(root: &Path, args: &Value, registry: &Registry) -> Result<PathBuf> {
    if !args.is_object() || !args["manifest"].is_object() {
        return Err("create_extension needs a manifest object and optional files array".into());
    }
    let input: Create = serde_json::from_value(args.clone())
        .map_err(|_| "create_extension needs a manifest object and optional files array")?;
    let bytes = serde_json::to_vec(&input.manifest)?;
    let (manifest, _validators) = manifest::parse(&bytes)?;
    registry.check_registration(&manifest)?;
    if input.files.len() > 32
        || input.files.iter().any(|file| file.content.len() > 65536)
        || input
            .files
            .iter()
            .map(|file| file.content.len())
            .sum::<usize>()
            > 1024 * 1024
    {
        return Err("Extension source is limited to 32 files, 64 KiB each and 1 MiB total".into());
    }
    let mut paths = HashSet::new();
    for file in &input.files {
        let path = Path::new(&file.path);
        if file.path.is_empty()
            || file.path.len() > 256
            || file.path.chars().any(char::is_control)
            || file
                .path
                .split('/')
                .any(|part| part.is_empty() || part == ".")
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || path
                .components()
                .next()
                .is_some_and(|part| part.as_os_str() == "extension.json")
        {
            return Err("Extension file paths must be relative normal paths; extension.json is supplied by manifest".into());
        }
        let normalized = path.components().collect::<PathBuf>();
        if !paths.insert(normalized) {
            return Err("Duplicate extension source path".into());
        }
    }
    if paths.iter().any(|path| {
        paths
            .iter()
            .any(|other| other != path && other.starts_with(path))
    }) {
        return Err("Extension source files conflict with a parent directory".into());
    }
    // Match discovery limits before writing, so a new folder cannot displace
    // a previously loaded extension after the sorted directory scan.
    match std::fs::read_dir(root) {
        Ok(entries) => {
            let mut directories = 0;
            for (index, entry) in entries.take(129).enumerate() {
                if entry?.path().is_dir() {
                    directories += 1;
                }
                if index >= 127 || directories >= 32 {
                    return Err("Extension directory limit reached; no files were created".into());
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)?;
    let directory = root.join(&manifest.id);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|_| {
            "Cannot create extension directory; it may already exist. Existing files were kept."
        })?;
    for file in input.files {
        let path = directory.join(file.path);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path.parent().ok_or("Missing source parent")?)?;
        let mut output = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(if file.executable { 0o700 } else { 0o600 })
            .open(path)?;
        output.write_all(file.content.as_bytes())?;
    }
    // The manifest is last: discovery never loads a half-written declaration.
    crate::runtime::config::atomic(&directory.join("extension.json"), &bytes)?;
    Ok(directory)
}
pub(crate) fn definitions() -> Vec<Value> {
    AgentTool::ALL.into_iter().map(|tool| {
        let (description,parameters)=match tool {
            AgentTool::List=>("Inspect installed user extensions, their directories, tool/command names and load errors.",json!({"type":"object","properties":{},"additionalProperties":false})),
            AgentTool::Reload=>("Reload extension manifests after editing files with read/write/edit. Publishes validated tools and slash commands for the next model step. Does not execute extension programs.",json!({"type":"object","properties":{},"additionalProperties":false})),
            AgentTool::Create=>("Create and register a complete user extension in ~/.hibiscus/extensions (or HIBISCUS_HOME/extensions). Supply your manifest and source files; no language or starter is imposed. Never overwrites an existing extension. For updates use read/write/edit then reload_extensions. Tools use id__name; commands default to /id:name or choose slashCommand such as /review. Native command names are reserved. Programs run in the workspace, read one JSON request from stdin until EOF and write one JSON reply. Request: {apiVersion:1,id,kind:'tool'|'command',name,arguments,cwd,extensionDir,sessionId}; command arguments are {input:string}. Reply: {apiVersion:1,id:request.id,text:string,isError?:bool,data?:any}. Each output stream is bounded to 64 KiB. Put logs on stderr. Use {extensionDir} in manifest command arguments for script paths. Files are private by default; executable:true marks a direct program executable.",json!({"type":"object","properties":{
                "manifest":{"type":"object","properties":{
                    "apiVersion":{"type":"integer","enum":[1]},"id":{"type":"string","pattern":"^[a-z][a-z0-9_]{0,27}$"},"description":{"type":"string"},"command":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":32},"timeoutSeconds":{"type":"integer","minimum":1,"maximum":600},
                    "tools":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"description":{"type":"string"},"parameters":{"type":"object"}},"required":["name","description","parameters"],"additionalProperties":false}},
                    "commands":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"description":{"type":"string"},"slashCommand":{"type":"string"}},"required":["name","description"],"additionalProperties":false}}
                },"required":["apiVersion","id","description","command"],"additionalProperties":false},
                "files":{"type":"array","maxItems":32,"items":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"},"executable":{"type":"boolean"}},"required":["path","content"],"additionalProperties":false}}
            },"required":["manifest"],"additionalProperties":false})),
        };
        json!({"name":tool.name(),"description":description,"parameters":parameters})
    }).collect()
}
