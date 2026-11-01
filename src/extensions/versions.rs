//! Complete version bundles and one atomic commit point. No process execution.
use super::{manifest, Registry};
use crate::{runtime::config, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
};

pub(super) const STORAGE: &str = ".versions";
const MAX_FILES: usize = 256;
const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_TOTAL: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct State {
    pub active: String,
    pub source_digest: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Fingerprint {
    sha256: String,
    bytes: usize,
    executable: bool,
}
type Inventory = BTreeMap<String, Fingerprint>;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub text: String,
    pub duration_millis: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum Validation {
    Preparing,
    Imported {
        deferred_checks: usize,
    },
    Passed {
        checks: Vec<CheckResult>,
    },
    Failed {
        message: String,
        checks: Vec<CheckResult>,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Record {
    pub version: String,
    pub parent: Option<String>,
    pub restored_from: Option<String>,
    pub created_at: String,
    pub changed_files: Vec<Change>,
    pub validation: Validation,
    files: Inventory,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Change {
    pub path: String,
    pub kind: String,
}
pub(super) struct Store {
    root: PathBuf,
    id: String,
}
pub(super) struct Candidate {
    pub store: Store,
    pub directory: PathBuf,
    pub record: Record,
    source_digest: String,
}

pub(super) fn private_dir(path: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    Ok(())
}
pub(super) fn relative(value: &str) -> Result<&Path> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(char::is_control)
        || value.split('/').any(|p| p.is_empty() || p == ".")
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(
            "Extension paths must be relative normal UTF-8 paths, at most 256 bytes".into(),
        );
    }
    Ok(path)
}
fn version(value: &str) -> Result<&str> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(
            "Invalid extension version; use a version returned by extension_history".into(),
        );
    }
    Ok(value)
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = read_file(path, 262144)?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err("Extension snapshots require regular files; nested symlinks and special files are unsupported".into());
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("Extension file exceeds its size limit".into());
    }
    Ok(bytes)
}
fn write_file(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    private_dir(path.parent().ok_or("Missing extension file parent")?)?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(if executable { 0o700 } else { 0o600 })
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn tree(directory: &Path) -> Result<(Inventory, BTreeMap<String, Vec<u8>>)> {
    fn visit(
        base: &Path,
        dir: &Path,
        files: &mut Inventory,
        data: &mut BTreeMap<String, Vec<u8>>,
        total: &mut usize,
        entries: &mut usize,
    ) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            *entries += 1;
            if *entries > 512 {
                return Err("Extension snapshot exceeds 512 directory entries".into());
            }
            let entry = entry?;
            let path = entry.path();
            let rel = path
                .strip_prefix(base)?
                .to_str()
                .ok_or("Extension paths must be UTF-8")?;
            relative(rel)?;
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.is_dir() {
                visit(base, &path, files, data, total, entries)?;
            } else {
                if files.len() >= MAX_FILES {
                    return Err("Extension snapshot exceeds 256 files".into());
                }
                let bytes = read_file(&path, MAX_FILE)?;
                *total += bytes.len();
                if *total > MAX_TOTAL {
                    return Err("Extension snapshot exceeds 32 MiB".into());
                }
                files.insert(
                    rel.into(),
                    Fingerprint {
                        sha256: format!("{:x}", Sha256::digest(&bytes)),
                        bytes: bytes.len(),
                        executable: metadata.permissions().mode() & 0o111 != 0,
                    },
                );
                data.insert(rel.into(), bytes);
            }
        }
        Ok(())
    }
    let mut files = Inventory::new();
    let mut data = BTreeMap::new();
    visit(directory, directory, &mut files, &mut data, &mut 0, &mut 0)?;
    Ok((files, data))
}
fn digest(files: &Inventory) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(files)?)))
}
fn changes(before: &Inventory, after: &Inventory) -> Vec<Change> {
    before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|path| {
            let kind = match (before.get(path), after.get(path)) {
                (None, Some(_)) => "added",
                (Some(_), None) => "deleted",
                (Some(a), Some(b)) if a != b => "modified",
                _ => return None,
            };
            Some(Change {
                path: path.clone(),
                kind: kind.into(),
            })
        })
        .collect()
}
/// Advisory lock is nonblocking: another update never blocks the runtime thread.
pub(super) struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
pub(super) fn lock(root: &Path) -> Result<Lock> {
    private_dir(&root.join(STORAGE))?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(root.join(STORAGE).join("lock"))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(
            "Another extension activation is in progress; retry with the current version".into(),
        );
    }
    Ok(Lock(file))
}
impl Store {
    pub fn new(root: &Path, id: &str) -> Result<Self> {
        if !manifest::identifier(id) {
            return Err("Invalid extension ID".into());
        }
        Ok(Self {
            root: root.to_path_buf(),
            id: id.into(),
        })
    }
    fn home(&self) -> PathBuf {
        self.root.join(STORAGE).join(&self.id)
    }
    pub fn source(&self) -> PathBuf {
        self.root.join(&self.id)
    }
    pub fn state(&self) -> Result<Option<State>> {
        let path = self.home().join("active.json");
        match read_json::<State>(&path) {
            Ok(state) => {
                version(&state.active)?;
                Ok(Some(state))
            }
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
    pub fn active(&self) -> Result<Option<(State, PathBuf)>> {
        self.state()?
            .map(|s| self.retained(&s.active).map(|(_, dir)| (s, dir)))
            .transpose()
    }
    pub fn retained(&self, revision: &str) -> Result<(Record, PathBuf)> {
        let dir = self.home().join("versions").join(version(revision)?);
        let record: Record = read_json(&dir.join("record.json"))?;
        if record.version != revision
            || !matches!(
                record.validation,
                Validation::Passed { .. } | Validation::Imported { .. }
            )
        {
            return Err("Extension version is incomplete or was never validated".into());
        }
        let payload = dir.join("files");
        if tree(&payload)?.0 != record.files {
            return Err(
                "Extension version integrity check failed; retained files were modified".into(),
            );
        }
        Ok((record, payload))
    }
    pub fn source_changed(&self, state: &State) -> Result<bool> {
        Ok(digest(&tree(&self.source())?.0)? != state.source_digest)
    }
    pub fn prepare(
        &self,
        source: &Path,
        parent: Option<&State>,
        restored_from: Option<String>,
        importing: bool,
    ) -> Result<Candidate> {
        let (files, bytes) = tree(source)?;
        let source_digest = if importing {
            digest(&files)?
        } else {
            parent
                .ok_or("Missing active extension")?
                .source_digest
                .clone()
        };
        let before = match parent {
            Some(p) => self.retained(&p.active)?.0.files,
            None => Inventory::new(),
        };
        let revision = format!("{:032x}", rand::random::<u128>());
        let directory = self.home().join("candidates").join(&revision);
        private_dir(directory.parent().ok_or("Missing candidate parent")?)?;
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let record = Record {
            version: revision,
            parent: parent.map(|p| p.active.clone()),
            restored_from,
            created_at: config::unique().to_string(),
            changed_files: changes(&before, &files),
            validation: Validation::Preparing,
            files,
        };
        let mut candidate = Candidate {
            store: Self::new(&self.root, &self.id)?,
            directory,
            record,
            source_digest,
        };
        candidate.save()?;
        private_dir(&candidate.files())?;
        for (path, bytes) in bytes {
            write_file(
                &candidate.files().join(&path),
                &bytes,
                candidate.record.files[&path].executable,
            )?;
        }
        candidate.refresh()?;
        Ok(candidate)
    }
    pub fn history(&self, limit: usize) -> Result<Value> {
        if !(1..=100).contains(&limit) {
            return Err("History limit must be 1–100".into());
        }
        let mut records = Vec::new();
        let mut errors = Vec::new();
        let mut truncated = false;
        for area in ["versions", "candidates"] {
            let entries = match fs::read_dir(self.home().join(area)) {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            for (index, entry) in entries.take(4097).enumerate() {
                if index == 4096 {
                    truncated = true;
                    break;
                }
                let path = entry?.path();
                match read_json::<Record>(&path.join("record.json")) {
                    Ok(record) => {
                        let content_digest = digest(&record.files)?;
                        let mut value = serde_json::to_value(record)?;
                        value
                            .as_object_mut()
                            .ok_or("Invalid record")?
                            .remove("files");
                        value["contentDigest"] = json!(content_digest);
                        if let Some(changes) = value["changedFiles"].as_array_mut() {
                            let omitted = changes.len().saturating_sub(128);
                            changes.truncate(128);
                            value["omittedChangedFiles"] = json!(omitted);
                        }
                        records.push(json!({"record":value,"location":area,"directory":path}));
                    }
                    Err(_) => {
                        errors.push(format!("Unreadable extension record: {}", path.display()))
                    }
                }
            }
        }
        records.sort_by(|a, b| {
            b["record"]["createdAt"]
                .as_str()
                .cmp(&a["record"]["createdAt"].as_str())
        });
        truncated |= records.len() > limit;
        records.truncate(limit);
        while serde_json::to_vec(&records)?.len() > 60000 && records.len() > 1 {
            records.pop();
            truncated = true;
        }
        Ok(
            json!({"id":self.id,"activeVersion":self.state()?.map(|s|s.active),"entries":records,"errors":errors,"truncated":truncated}),
        )
    }
    pub fn diff(&self, from: &str, to: &str) -> Result<Value> {
        let (a, adir) = self.inspect(from)?;
        let (b, bdir) = self.inspect(to)?;
        let changed = changes(&a.files, &b.files);
        let mut text = String::new();
        let mut truncated = false;
        for change in &changed {
            if text.len() >= 32768 {
                truncated = true;
                break;
            }
            text.push_str(&format!(
                "--- {from}/{}\n+++ {to}/{}\n",
                change.path, change.path
            ));
            let old = if a.files.contains_key(&change.path) {
                read_file(&adir.join(&change.path), MAX_FILE)?
            } else {
                vec![]
            };
            let new = if b.files.contains_key(&change.path) {
                read_file(&bdir.join(&change.path), MAX_FILE)?
            } else {
                vec![]
            };
            if old != new {
                match (std::str::from_utf8(&old), std::str::from_utf8(&new)) {
                    (Ok(old), Ok(new)) if old.len() + new.len() <= 131072 => {
                        let (preview, _, _) = crate::runtime::edit_diff(old, new);
                        text.push_str(&preview);
                    }
                    _ => text.push_str("[Binary or large file content changed]\n"),
                }
            }
            if a.files.get(&change.path).map(|f| f.executable)
                != b.files.get(&change.path).map(|f| f.executable)
            {
                text.push_str("[Executable mode changed]\n");
            }
        }
        if text.len() > 49152 {
            let mut end = 49152;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            truncated = true;
        }
        Ok(
            json!({"id":self.id,"from":from,"to":to,"changedFiles":changed,"diff":text,"truncated":truncated}),
        )
    }
    fn inspect(&self, revision: &str) -> Result<(Record, PathBuf)> {
        version(revision)?;
        for area in ["versions", "candidates"] {
            let path = self.home().join(area).join(revision);
            if path.exists() {
                let mut record: Record = read_json(&path.join("record.json"))?;
                let files = path.join("files");
                // Interrupted/failed candidates may be incomplete. Report their actual files.
                record.files = tree(&files)?.0;
                return Ok((record, files));
            }
        }
        Err("Unknown extension version".into())
    }
}
impl Candidate {
    pub fn check_copy(&self) -> Result<PathBuf> {
        let copy = self.directory.join("validation");
        private_dir(&copy)?;
        let (files, data) = tree(&self.files())?;
        for (path, bytes) in data {
            write_file(&copy.join(&path), &bytes, files[&path].executable)?;
        }
        Ok(copy.canonicalize()?)
    }
    pub fn files(&self) -> PathBuf {
        self.directory.join("files")
    }
    pub fn save(&self) -> Result<()> {
        config::atomic(
            &self.directory.join("record.json"),
            &serde_json::to_vec_pretty(&self.record)?,
        )
    }
    pub fn refresh(&mut self) -> Result<()> {
        // Legacy manifests sometimes use absolute paths to their own source.
        // Rebase those entrypoint/check arguments into the frozen bundle too.
        let manifest_path = self.files().join("extension.json");
        if let Some(mut value) = read_json::<Value>(&manifest_path)
            .ok()
            .filter(Value::is_object)
        {
            let source = self.store.source();
            let canonical = source.canonicalize().unwrap_or_else(|_| source.clone());
            let prefixes = [
                format!("{}/", source.display()),
                format!("{}/", canonical.display()),
            ];
            fn rebase(args: &mut Value, prefix: &str) {
                if let Some(args) = args.as_array_mut() {
                    for arg in args {
                        if let Some(suffix) = arg.as_str().and_then(|s| s.strip_prefix(prefix)) {
                            *arg = json!(format!("{{extensionDir}}/{suffix}"));
                        }
                    }
                }
            }
            let before = value.clone();
            for prefix in &prefixes {
                rebase(&mut value["command"], prefix);
            }
            if let Some(checks) = value.get_mut("checks").and_then(Value::as_array_mut) {
                for check in checks {
                    if check["kind"] == "exec" {
                        for prefix in &prefixes {
                            rebase(&mut check["command"], prefix);
                        }
                    }
                }
            }
            if before != value {
                config::atomic(&manifest_path, &serde_json::to_vec(&value)?)?;
            }
        }
        let files = tree(&self.files())?.0;
        let before = match &self.record.parent {
            Some(p) => self.store.retained(p)?.0.files,
            None => Inventory::new(),
        };
        self.record.changed_files = changes(&before, &files);
        self.record.files = files;
        self.save()
    }
    pub fn replace(&self, path: &str, bytes: &[u8], executable: bool) -> Result<()> {
        let path = self.files().join(relative(path)?);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        write_file(&path, bytes, executable)
    }
    pub fn remove(&self, path: &str) -> Result<()> {
        fs::remove_file(self.files().join(relative(path)?))?;
        Ok(())
    }
    pub fn fail(&mut self, message: &str, checks: Vec<CheckResult>) -> Result<()> {
        if let Ok((files, _)) = tree(&self.files()) {
            let before = match &self.record.parent {
                Some(p) => self.store.retained(p)?.0.files,
                None => Inventory::new(),
            };
            self.record.changed_files = changes(&before, &files);
            self.record.files = files;
        }
        self.record.validation = Validation::Failed {
            message: crate::runtime::error::safe_message(message),
            checks,
        };
        self.save()
    }
    pub fn activate(&mut self, validation: Validation) -> Result<Record> {
        let _lock = lock(&self.store.root)?;
        let current = self.store.state()?;
        if current.as_ref().map(|s| &s.active) != self.record.parent.as_ref() {
            return Err("Extension changed while preparing this candidate; active version was kept. Inspect history and retry with expectedVersion.".into());
        }
        if tree(&self.files())?.0 != self.record.files {
            return Err(
                "Candidate files changed during validation; active version was kept".into(),
            );
        }
        Registry::check_candidate(&self.store.root, &self.store.id, &self.files())?;
        if !matches!(
            validation,
            Validation::Passed { .. } | Validation::Imported { .. }
        ) {
            return Err("Only validated candidates can activate".into());
        }
        self.record.validation = validation;
        self.save()?;
        let destination = self
            .store
            .home()
            .join("versions")
            .join(&self.record.version);
        private_dir(destination.parent().ok_or("Missing versions parent")?)?;
        sync_dirs(&self.files())?;
        File::open(&self.directory)?.sync_all()?;
        fs::rename(&self.directory, &destination)?;
        File::open(destination.parent().ok_or("Missing versions parent")?)?.sync_all()?;
        let state = State {
            active: self.record.version.clone(),
            source_digest: self.source_digest.clone(),
        };
        config::atomic(
            &self.store.home().join("active.json"),
            &serde_json::to_vec(&state)?,
        )?;
        File::open(self.store.home())?.sync_all()?;
        Ok(self.record.clone())
    }
}
fn sync_dirs(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            sync_dirs(&path)?;
        }
    }
    File::open(dir)?.sync_all()?;
    Ok(())
}
