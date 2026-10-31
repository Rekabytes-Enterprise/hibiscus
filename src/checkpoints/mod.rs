//! Workspace recovery for native file writes. No Git operations or approvals.
mod files;
mod store;
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use store::Store;
use tokio::sync::watch;
const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_REQUEST: u64 = 64 * 1024 * 1024;
const MAX_WORKSPACE: u64 = 512 * 1024 * 1024;
const MAX_REQUESTS: usize = 256;
const MAX_FILES: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
struct Id(String);
impl Id {
    fn new() -> Self {
        Self(format!("{:032x}", rand::random::<u128>()))
    }
}
impl TryFrom<String> for Id {
    type Error = String;
    fn try_from(s: String) -> std::result::Result<Self, String> {
        if s.len() == 32
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            Ok(Self(s))
        } else {
            Err("Invalid checkpoint ID; use checkpoint_history".into())
        }
    }
}
impl From<Id> for String {
    fn from(id: Id) -> Self {
        id.0
    }
}
impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
struct Relative(String);
impl TryFrom<String> for Relative {
    type Error = String;
    fn try_from(s: String) -> std::result::Result<Self, String> {
        use std::path::Component;
        if s.is_empty()
            || s.len() > 512
            || s.chars().any(char::is_control)
            || s.split('/').any(|s| s.is_empty() || s == ".")
            || Path::new(&s).components().any(|p| {
                !matches!(p, Component::Normal(_))
                    || p.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git")
            })
        {
            return Err("Checkpoint paths must be normal relative UTF-8 file paths up to 512 bytes, outside Git metadata".into());
        }
        Ok(Self(s))
    }
}
impl From<Relative> for String {
    fn from(path: Relative) -> Self {
        path.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Image {
    Missing,
    Present { hash: String, bytes: u64, mode: u32 },
}
impl Image {
    fn present(bytes: &[u8], mode: u32) -> Self {
        Self::Present {
            hash: files::digest(bytes),
            bytes: bytes.len() as u64,
            mode,
        }
    }
    fn size(&self) -> u64 {
        match self {
            Self::Missing => 0,
            Self::Present { bytes, .. } => *bytes,
        }
    }
    fn validate(&self) -> Result<()> {
        match self {
            Self::Present { hash, bytes, mode }
                if !files::hash(hash) || *bytes > MAX_FILE || *mode > 0o777 =>
            {
                Err("Invalid checkpoint image metadata".into())
            }
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase", deny_unknown_fields)]
enum Phase {
    PendingWrite { previous: Image },
    Applied,
    Restoring,
    Restored,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    before: Image,
    after: Image,
    phase: Phase,
    calls: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    format: u32,
    id: Id,
    workspace: PathBuf,
    session_id: String,
    created_at: String,
    files: BTreeMap<Relative, Entry>,
}
impl Record {
    fn validate(&self, workspace: &Path, id: &Id) -> Result<()> {
        if self.format != 1
            || self.workspace != workspace
            || &self.id != id
            || self.files.len() > MAX_FILES
            || self.session_id.len() > 128
            || self.created_at.parse::<u128>().is_err()
        {
            return Err("Invalid checkpoint record identity or bounds".into());
        }
        let mut calls = 0;
        let mut bytes = 0;
        for entry in self.files.values() {
            entry.before.validate()?;
            entry.after.validate()?;
            if let Phase::PendingWrite { previous } = &entry.phase {
                previous.validate()?;
            }
            if entry
                .calls
                .iter()
                .any(|s| s.len() > 256 || s.chars().any(char::is_control))
            {
                return Err("Invalid checkpoint call trace".into());
            }
            calls += entry.calls.len();
            bytes += entry.before.size() + entry.after.size();
        }
        if calls > 256 || bytes > MAX_REQUEST {
            return Err("Checkpoint exceeds request limits".into());
        }
        Ok(())
    }
}
/// A lease is acquired lazily at the first write and retained through all model
/// steps for one user request. Dropping the request releases it on every exit.
pub(crate) struct Request {
    id: Id,
    session_id: String,
    lease: Option<fs::File>,
}
impl Request {
    pub(crate) fn new(session_id: &str) -> Self {
        Self {
            id: Id::new(),
            session_id: session_id.into(),
            lease: None,
        }
    }
}
pub(crate) struct Context<'a> {
    pub(crate) cwd: &'a Path,
    pub(crate) request: &'a mut Request,
    pub(crate) call_id: &'a str,
}
pub(crate) fn write(
    context: &mut Context<'_>,
    path: &Path,
    bytes: &[u8],
    expected: Option<&[u8]>,
) -> Result<Value> {
    Store::open(context.cwd)?.write(context, path, bytes, expected)
}
#[derive(Clone, Copy)]
pub(crate) enum Tool {
    History,
    Diff,
    Restore,
    Prune,
}
impl Tool {
    const ALL: [Self; 4] = [Self::History, Self::Diff, Self::Restore, Self::Prune];
    fn name(self) -> &'static str {
        match self {
            Self::History => "checkpoint_history",
            Self::Diff => "checkpoint_diff",
            Self::Restore => "checkpoint_restore",
            Self::Prune => "checkpoint_prune",
        }
    }
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
    pub(crate) async fn execute(
        self,
        args: &Value,
        context: &mut Context<'_>,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Value> {
        if !args.is_object() {
            return Err("Checkpoint arguments must be a JSON object".into());
        }
        if *cancel.borrow() {
            return Err("Checkpoint operation cancelled".into());
        }
        let store = Store::open(context.cwd)?;
        let report = match self {
            Self::History => {
                let a: History = serde_json::from_value(args.clone())?;
                {
                    let mut report = store.history(
                        a.limit,
                        if a.all_sessions {
                            None
                        } else {
                            Some(&context.request.session_id)
                        },
                    )?;
                    report["currentSession"] = json!(context.request.session_id);
                    report["allSessions"] = json!(a.all_sessions);
                    report
                }
            }
            Self::Diff => {
                let a: Selection = serde_json::from_value(args.clone())?;
                store.diff(a)?
            }
            Self::Restore => {
                let a: Selection = serde_json::from_value(args.clone())?;
                store.restore(a, cancel).await?
            }
            Self::Prune => {
                let a: Prune = serde_json::from_value(args.clone())?;
                store.prune(a.keep)?
            }
        };
        let text = serde_json::to_string_pretty(&report)?;
        let text = if text.len() > 65536 {
            let mut end = 65536;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            format!(
                "{}\n[Checkpoint output truncated; request fewer paths]",
                &text[..end]
            )
        } else {
            text
        };
        Ok(json!({"content":[{"type":"text","text":text}],"details":report}))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    #[serde(default = "history_limit")]
    limit: usize,
    #[serde(default, rename = "allSessions")]
    all_sessions: bool,
}
fn history_limit() -> usize {
    10
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prune {
    #[serde(default = "keep_limit")]
    keep: usize,
}
fn keep_limit() -> usize {
    20
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    checkpoint: Id,
    #[serde(default)]
    paths: Option<Vec<Relative>>,
}
impl Selection {
    fn paths(self, record: &Record) -> Result<Vec<Relative>> {
        let paths = self
            .paths
            .unwrap_or_else(|| record.files.keys().cloned().collect());
        if paths.is_empty()
            || paths.len() > MAX_FILES
            || paths
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != paths.len()
            || paths.iter().any(|p| !record.files.contains_key(p))
        {
            return Err("Select 1–32 unique paths belonging to this checkpoint".into());
        }
        Ok(paths)
    }
}
pub(crate) fn definitions() -> Vec<Value> {
    Tool::ALL.into_iter().map(|tool|{
        let (description,properties,required)=match tool{
            Tool::History=>("List native write/edit checkpoints for this canonical workspace and current session, newest first. Set allSessions:true to inspect older/other conversations. The report identifies currentSession. Includes checkpoint ID, session, active lease, file counts and pending/restored state. Use this before undoing a user request. Coverage excludes bash, extensions, Git metadata, Hibiscus data and paths outside this workspace. limit defaults to 10, maximum 50.",json!({"limit":{"type":"integer","minimum":1,"maximum":50},"allSessions":{"type":"boolean"}}),json!([])),
            Tool::Diff=>("Inspect a native checkpoint's recorded before/result diff, current conflicts and file status. Does not change project files. Supply checkpoint from checkpoint_history and optionally a subset of its relative paths.",json!({"checkpoint":{"type":"string"},"paths":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string"}}}),json!(["checkpoint"])),
            Tool::Restore=>("Undo native write/edit changes from one checkpoint, optionally selecting relative file paths. Preflights all selected paths; refuses later edits/conflicts without overwriting them. Restores original bytes/mode or removes unchanged newly created files, preserving existing user work and Git state. Interrupted restores can be retried with the same checkpoint and paths. No bash/extension rollback guarantee. Inspect history/diff first; never substitute a destructive Git reset.",json!({"checkpoint":{"type":"string"},"paths":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string"}}}),json!(["checkpoint"])),
            Tool::Prune=>("Explicitly prune old inactive, resolved native checkpoints and unreferenced snapshot images in this workspace. Keeps newest checkpoints, defaults to 20. Active and unfinished operations are always retained. Use when storage reaches its limit; deleted recovery history cannot be restored.",json!({"keep":{"type":"integer","minimum":1,"maximum":256}}),json!([])),
        };
        json!({"name":tool.name(),"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
    }).collect()
}
