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
    Update,
    History,
    Diff,
    Rollback,
}
impl AgentTool {
    const ALL: [Self; 7] = [
        Self::Create,
        Self::List,
        Self::Reload,
        Self::Update,
        Self::History,
        Self::Diff,
        Self::Rollback,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Create => "create_extension",
            Self::List => "list_extensions",
            Self::Reload => "reload_extensions",
            Self::Update => "update_extension",
            Self::History => "extension_history",
            Self::Diff => "extension_diff",
            Self::Rollback => "rollback_extension",
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
    fn root(&self) -> Result<PathBuf> {
        self.snapshot()
            .root
            .clone()
            .map(Ok)
            .unwrap_or_else(|| Ok(crate::paths::home()?.join("extensions")))
    }
    fn reload(&self, import_edits: bool) -> Result<Arc<Registry>> {
        let registry = Arc::new(Registry::load_mode(&self.root()?, import_edits)?);
        *self.current.write().unwrap() = registry.clone();
        Ok(registry)
    }
    pub(crate) async fn execute(
        &self,
        tool: AgentTool,
        args: &Value,
        context: (&Path, &str),
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Value> {
        match tool {
            AgentTool::Update | AgentTool::Rollback => {
                return self.update(tool, args, context, cancel).await;
            }
            AgentTool::History => {
                let input: History = serde_json::from_value(args.clone())?;
                return tool_result(
                    versions::Store::new(&self.root()?, &input.id)?.history(input.limit)?,
                );
            }
            AgentTool::Diff => {
                let input: Diff = serde_json::from_value(args.clone())?;
                return tool_result(
                    versions::Store::new(&self.root()?, &input.id)?.diff(&input.from, &input.to)?,
                );
            }
            AgentTool::Create | AgentTool::List | AgentTool::Reload => {}
        }

        let (report, directory) = match tool {
            AgentTool::List => {
                empty(args)?;
                (self.snapshot().report(), None)
            }
            AgentTool::Reload => {
                empty(args)?;
                (self.reload(true)?.report(), None)
            }
            AgentTool::Create => {
                let root = self.root()?;
                // Validate against current files too: a preceding native edit
                // may have changed another manifest since the model snapshot.
                let latest = Registry::load(&root)?;
                let directory = create_at(&root, args, &latest)?;
                let registry = self.reload(false)?;
                let id = directory
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("Invalid extension directory")?;
                if !registry.extensions.contains_key(id) {
                    return Err("Extension files were created, but registration failed. Inspect list_extensions errors before editing or retrying.".into());
                }
                (registry.report(), Some(directory))
            }
            AgentTool::Update | AgentTool::Rollback | AgentTool::History | AgentTool::Diff => {
                unreachable!("handled above")
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
                let entry = entry?;
                if entry.file_name() != versions::STORAGE && entry.path().is_dir() {
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
        if matches!(tool, AgentTool::Update | AgentTool::History | AgentTool::Diff | AgentTool::Rollback) { return management_definition(tool); }
        let (description,parameters)=match tool {
            AgentTool::List=>("Inspect installed user extensions, their directories, tool/command names and load errors.",json!({"type":"object","properties":{},"additionalProperties":false})),
            AgentTool::Reload=>("Reload extension manifests after editing files with read/write/edit. Publishes validated tools and slash commands for the next model step. Does not execute extension programs.",json!({"type":"object","properties":{},"additionalProperties":false})),
            AgentTool::Create=>("Create and register a complete user extension in ~/.hibiscus/extensions (or HIBISCUS_HOME/extensions). Supply your manifest and source files; no language or starter is imposed. Never overwrites an existing extension. For versioned updates use update_extension with expectedVersion from list_extensions. For external source edits use reload_extensions, or update_extension fromSource:true when checks are configured. Tools use id__name; commands default to /id:name or choose slashCommand such as /review. Native command names are reserved. Programs run in the workspace, read one JSON request from stdin until EOF and write one JSON reply. Request: {apiVersion:1,id,kind:'tool'|'command',name,arguments,cwd,extensionDir,sessionId}; command arguments are {input:string}. Reply: {apiVersion:1,id:request.id,text:string,isError?:bool,data?:any}. Extensions inherit only PATH, HOME, TMPDIR, LANG, LC_ALL, LC_CTYPE, TZ and TERM when set, plus Hibiscus context paths. Optional inheritEnv lists additional parent variable names, never values; missing declared variables fail before execution. Declare credentials only when the extension needs them. Each output stream is bounded to 64 KiB. Put logs on stderr. Use {extensionDir} in manifest command arguments for script paths. Files are private by default; executable:true marks a direct program executable.",json!({"type":"object","properties":{
                "manifest":{"type":"object","properties":{
                    "apiVersion":{"type":"integer","enum":[1]},"id":{"type":"string","pattern":"^[a-z][a-z0-9_]{0,27}$"},"description":{"type":"string"},"command":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":32},"inheritEnv":{"type":"array","maxItems":32,"uniqueItems":true,"items":{"type":"string","maxLength":128,"pattern":"^[A-Za-z_][A-Za-z0-9_]*$","not":{"enum":["HIBISCUS_EXTENSION_DIR","HIBISCUS_WORKSPACE"]}}},"timeoutSeconds":{"type":"integer","minimum":1,"maximum":600},"checks":{"type":"array","maxItems":8,"items":{"type":"object"}},
                    "tools":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"description":{"type":"string"},"parameters":{"type":"object"}},"required":["name","description","parameters"],"additionalProperties":false}},
                    "commands":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"description":{"type":"string"},"slashCommand":{"type":"string"}},"required":["name","description"],"additionalProperties":false}}
                },"required":["apiVersion","id","description","command"],"additionalProperties":false},
                "files":{"type":"array","maxItems":32,"items":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"},"executable":{"type":"boolean"}},"required":["path","content"],"additionalProperties":false}}
            },"required":["manifest"],"additionalProperties":false})),
            AgentTool::Update | AgentTool::History | AgentTool::Diff | AgentTool::Rollback => unreachable!("handled above"),
        };
        json!({"name":tool.name(),"description":description,"parameters":parameters})
    }).collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Update {
    id: String,
    expected_version: String,
    #[serde(default)]
    manifest: Option<Value>,
    #[serde(default)]
    files: Vec<SourceFile>,
    #[serde(default)]
    remove: Vec<String>,
    #[serde(default)]
    from_source: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Rollback {
    id: String,
    expected_version: String,
    version: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    id: String,
    #[serde(default = "history_limit")]
    limit: usize,
}
fn history_limit() -> usize {
    20
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Diff {
    id: String,
    from: String,
    to: String,
}
fn tool_result(report: Value) -> Result<Value> {
    Ok(
        json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&report)?}],"details":report}),
    )
}
impl Catalog {
    async fn update(
        &self,
        tool: AgentTool,
        args: &Value,
        context: (&Path, &str),
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Value> {
        let (input, restore) = match tool {
            AgentTool::Update => (serde_json::from_value::<Update>(args.clone())?, None),
            AgentTool::Rollback => {
                let input: Rollback = serde_json::from_value(args.clone())?;
                (
                    Update {
                        id: input.id,
                        expected_version: input.expected_version,
                        manifest: None,
                        files: vec![],
                        remove: vec![],
                        from_source: false,
                    },
                    Some(input.version),
                )
            }
            _ => return Err("Expected an extension update or rollback".into()),
        };
        let root = self.root()?;
        let store = versions::Store::new(&root, &input.id)?;
        let (state, active) = store
            .active()?
            .ok_or("Extension has no active version; create or reload it first")?;
        if state.active != input.expected_version {
            return Err("expectedVersion does not match the current active version; inspect list_extensions before retrying".into());
        }
        validate_patch(&input)?;
        let source = if let Some(revision) = &restore {
            store.retained(revision)?.1
        } else if input.from_source {
            store.source()
        } else {
            active
        };
        let mut candidate = store.prepare(&source, Some(&state), restore, input.from_source)?;
        let result: Result<Vec<versions::CheckResult>> = async {
            for path in &input.remove {
                candidate.remove(path)?;
            }
            for file in &input.files {
                candidate.replace(&file.path, file.content.as_bytes(), file.executable)?;
            }
            if let Some(manifest) = &input.manifest {
                candidate.replace("extension.json", &serde_json::to_vec(manifest)?, false)?;
            }
            candidate.refresh()?;
            Registry::check_candidate(&root, &input.id, &candidate.files())?;
            validation::run(&candidate, context, cancel).await
        }
        .await;
        let checks = match result {
            Ok(checks) => checks,
            Err(error) => {
                candidate.fail(&error.to_string(), vec![])?;
                return Err(format!("Candidate {} rejected: {error}. Active version unchanged. Inspect extension_history and extension_diff.", candidate.record.version).into());
            }
        };
        if checks.iter().any(|c| !c.passed) || *cancel.borrow() {
            let message = checks
                .iter()
                .find(|c| !c.passed)
                .map(|c| c.text.as_str())
                .unwrap_or("Extension validation cancelled");
            candidate.fail(message, checks.clone())?;
            return Err(format!(
                "Candidate {} failed validation: {message}. Active version unchanged.",
                candidate.record.version
            )
            .into());
        }
        let record = match candidate.activate(versions::Validation::Passed {
            checks: checks.clone(),
        }) {
            Ok(record) => record,
            Err(error) => {
                candidate.fail(&error.to_string(), checks)?;
                return Err(error);
            }
        };
        let registry = self.reload(false)?;
        tool_result(
            json!({"id":input.id,"activeVersion":record.version,"record":record,"catalog":registry.report()}),
        )
    }
}
fn validate_patch(input: &Update) -> Result<()> {
    if input.files.len() > 32
        || input.remove.len() > 32
        || input.files.iter().any(|f| f.content.len() > 65536)
        || input.files.iter().map(|f| f.content.len()).sum::<usize>() > 1024 * 1024
    {
        return Err("Extension patch is limited to 32 writes, 32 removals, 64 KiB per write and 1 MiB total".into());
    }
    if input.manifest.as_ref().is_some_and(|v| !v.is_object()) {
        return Err("manifest must be an object".into());
    }
    let mut paths = HashSet::new();
    for value in input.files.iter().map(|f| &f.path).chain(&input.remove) {
        let path = versions::relative(value)?;
        if path
            .components()
            .next()
            .is_some_and(|p| p.as_os_str() == "extension.json")
            || !paths.insert(path)
        {
            return Err(
                "Duplicate patch path or reserved extension.json; supply manifest separately"
                    .into(),
            );
        }
    }
    if paths.iter().any(|path| {
        paths
            .iter()
            .any(|other| other != path && other.starts_with(path))
    }) {
        return Err("Extension patch paths conflict with a parent directory".into());
    }
    Ok(())
}
fn management_definition(tool: AgentTool) -> Value {
    let (description, properties, required) = match tool {
        AgentTool::Update => ("Prepare a versioned extension update without changing the active files. Supply id and expectedVersion from list_extensions. Files patch the active version, preserving unspecified files; remove explicitly deletes files. Optional manifest replaces the whole manifest with the same id. fromSource:true imports the editable installation folder instead. Validates the entire candidate and configured manifest checks before atomic activation. Failed candidates remain inspectable; stale updates never replace another update.", json!({"id":{"type":"string"},"expectedVersion":{"type":"string"},"manifest":{"type":"object"},"fromSource":{"type":"boolean"},"files":{"type":"array","maxItems":32,"items":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"},"executable":{"type":"boolean"}},"required":["path","content"],"additionalProperties":false}},"remove":{"type":"array","maxItems":32,"items":{"type":"string"}}}), json!(["id","expectedVersion"])),
        AgentTool::History => ("Inspect retained versions and candidate updates, including active version, parent, changed files, validation results and interrupted/failed candidates. Version files are retained automatically. limit defaults to 20, maximum 100.", json!({"id":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":100}}), json!(["id"])),
        AgentTool::Diff => ("Compare two extension version or candidate IDs returned by extension_history. Reports changed paths, executable modes and a bounded text diff. Never executes the extension.", json!({"id":{"type":"string"},"from":{"type":"string"},"to":{"type":"string"}}), json!(["id","from","to"])),
        AgentTool::Rollback => ("Restore a retained extension version as a new version whose parent is the current active version. Supply expectedVersion to avoid overwriting a concurrent update. Revalidates the full manifest, namespace, executable and configured regression checks before atomic activation. Keeps all existing versions and author files.", json!({"id":{"type":"string"},"expectedVersion":{"type":"string"},"version":{"type":"string"}}), json!(["id","expectedVersion","version"])),
        AgentTool::Create | AgentTool::List | AgentTool::Reload => unreachable!("management tools only"),
    };
    json!({"name":tool.name(),"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
