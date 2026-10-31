use super::*;
use std::{
    collections::BTreeSet,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
};

pub(super) struct Store {
    workspace: PathBuf,
    home: PathBuf,
    root: PathBuf,
}
struct Lock(fs::File);
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
fn lease(path: &Path) -> Result<fs::File> {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    // SAFETY: the owned descriptor remains open for the lifetime of the lease.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Another checkpoint operation is active; retry after it finishes".into());
    }
    Ok(file)
}
impl Store {
    pub fn open(workspace: &Path) -> Result<Self> {
        Self::at(workspace, &crate::paths::home()?)
    }
    pub(super) fn at(workspace: &Path, home: &Path) -> Result<Self> {
        let workspace = workspace.canonicalize()?;
        files::directory(home)?;
        let home = home.canonicalize()?;
        let root = home
            .join("checkpoints")
            .join(files::digest(workspace.as_os_str().as_encoded_bytes()));
        files::directory(&root.join("requests"))?;
        files::directory(&root.join("blobs"))?;
        Ok(Self {
            workspace,
            home,
            root,
        })
    }
    fn lock(&self) -> Result<Lock> {
        Ok(Lock(lease(&self.root.join("lock"))?))
    }
    fn directory(&self, id: &Id) -> PathBuf {
        self.root.join("requests").join(&id.0)
    }
    fn path(&self, path: &Relative) -> Result<PathBuf> {
        let target = files::target(&self.workspace, &self.workspace.join(&path.0))?;
        if self.covered(&target)?.as_ref() != Some(path) {
            return Err("Checkpoint path is outside protected workspace files".into());
        }
        Ok(target)
    }
    fn covered(&self, target: &Path) -> Result<Option<Relative>> {
        let Ok(path) = target.strip_prefix(&self.workspace) else {
            return Ok(None);
        };
        if target.starts_with(&self.home)
            || path
                .components()
                .any(|p| p.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
        {
            return Ok(None);
        }
        Ok(Some(Relative::try_from(
            path.to_str()
                .ok_or("Checkpoint paths must be UTF-8")?
                .to_owned(),
        )?))
    }
    fn load(&self, id: &Id) -> Result<Record> {
        let record: Record = files::json(&self.directory(id).join("record.json"))?;
        record.validate(&self.workspace, id)?;
        Ok(record)
    }
    fn save(&self, record: &Record) -> Result<()> {
        record.validate(&self.workspace, &record.id)?;
        let bytes = serde_json::to_vec_pretty(record)?;
        if bytes.len() > 262144 {
            return Err("Checkpoint record exceeds 256 KiB".into());
        }
        files::atomic(
            &self.directory(&record.id).join("record.json"),
            &bytes,
            0o600,
        )
    }
    fn ids(&self) -> Result<Vec<Id>> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(self.root.join("requests"))?.take(MAX_REQUESTS + 1) {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                return Err("Unexpected checkpoint directory entry".into());
            }
            ids.push(Id::try_from(
                entry
                    .file_name()
                    .to_str()
                    .ok_or("Invalid checkpoint directory")?
                    .to_owned(),
            )?);
        }
        if ids.len() > MAX_REQUESTS {
            return Err("Checkpoint directory exceeds its 256-request limit".into());
        }
        Ok(ids)
    }
    fn active(&self, id: &Id) -> bool {
        lease(&self.directory(id).join("lease")).is_err()
    }
    fn blobs(&self) -> Result<(usize, u64)> {
        let mut count = 0;
        let mut bytes = 0;
        for entry in fs::read_dir(self.root.join("blobs"))?.take(8193) {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.is_file() {
                return Err("Invalid checkpoint image storage entry".into());
            }
            count += 1;
            bytes += metadata.len();
        }
        Ok((count, bytes))
    }
    fn put(&self, image: &Image, bytes: &[u8]) -> Result<()> {
        let Image::Present { hash, .. } = image else {
            return Ok(());
        };
        let path = self.root.join("blobs").join(hash);
        if path.exists() {
            self.bytes(image)?;
            return Ok(());
        }
        let (count, total) = self.blobs()?;
        if count >= 8192 || total + bytes.len() as u64 > MAX_WORKSPACE {
            return Err("Checkpoint storage limit reached (512 MiB / 8192 images). Use checkpoint_prune; protected file was not changed".into());
        }
        files::atomic(&path, bytes, 0o600)
    }
    fn bytes(&self, image: &Image) -> Result<Vec<u8>> {
        let Image::Present { hash, bytes, .. } = image else {
            return Ok(vec![]);
        };
        let path = self.root.join("blobs").join(hash);
        let (actual, data) = files::read(&path)?;
        if let Image::Present {
            hash: actual_hash,
            bytes: actual_size,
            ..
        } = actual
        {
            if actual_hash == *hash && actual_size == *bytes {
                return Ok(data);
            }
        }
        Err("Checkpoint image is missing or corrupt; project files were kept".into())
    }
    pub fn write(
        &self,
        context: &mut Context<'_>,
        path: &Path,
        bytes: &[u8],
        expected: Option<&[u8]>,
    ) -> Result<Value> {
        if bytes.len() as u64 > MAX_FILE {
            return Err("File exceeds 16 MiB".into());
        }
        let target = files::target(&self.workspace, path)?;
        let relative = self.covered(&target)?;
        let _lock = self.lock()?;
        let (current, original) = files::read(&target)?;
        if expected.is_some_and(|expected| expected != original) {
            return Err("File changed after it was read; edit was not applied".into());
        }
        let mode = match current {
            Image::Present { mode, .. } => mode,
            Image::Missing => 0o600,
        };
        let Some(relative) = relative else {
            files::atomic(&target, bytes, mode)?;
            return Ok(
                json!({"protected":false,"reason":"Outside workspace checkpoint coverage: external paths, Git metadata and Hibiscus data are excluded"}),
            );
        };
        let request = &mut context.request;
        let directory = self.directory(&request.id);
        let mut record = if directory.join("record.json").exists() {
            self.load(&request.id)?
        } else {
            if self.ids()?.len() >= MAX_REQUESTS {
                return Err("Checkpoint request limit reached (256). Use checkpoint_prune; protected file was not changed".into());
            }
            let record = Record {
                format: 1,
                id: request.id.clone(),
                workspace: self.workspace.clone(),
                session_id: request.session_id.clone(),
                created_at: crate::runtime::config::unique().to_string(),
                files: BTreeMap::new(),
            };
            self.save(&record)?;
            record
        };
        if request.lease.is_none() {
            request.lease = Some(lease(&directory.join("lease"))?);
        }
        if let Some(existing) = record.files.get(&relative) {
            let resolved = self.resolve(&relative, existing)?;
            if !matches!(resolved.phase, Phase::Applied) || resolved.after != current {
                return Err(format!("{} changed after its checkpoint; start a new request after inspecting the file",relative.0).into());
            }
        }
        let after = Image::present(bytes, mode);
        let before = record
            .files
            .get(&relative)
            .map_or_else(|| current.clone(), |e| e.before.clone());
        let mut calls = record
            .files
            .get(&relative)
            .map(|e| e.calls.clone())
            .unwrap_or_default();
        calls.push(context.call_id.into());
        record.files.insert(
            relative.clone(),
            Entry {
                before: before.clone(),
                after: after.clone(),
                phase: Phase::PendingWrite {
                    previous: current.clone(),
                },
                calls,
            },
        );
        // Bound and save both images before publishing the write intent.
        record.validate(&self.workspace, &request.id)?;
        let before_bytes = if before == current {
            original.clone()
        } else {
            self.bytes(&before)?
        };
        self.put(&before, &before_bytes)?;
        self.put(&after, bytes)?;
        self.save(&record)?;
        if files::read(&self.path(&relative)?)?.0 != current {
            return Err(
                "File changed before replacement; checkpoint retained, edit refused".into(),
            );
        }
        files::atomic(&target, bytes, mode)?;
        if files::read(&target)?.0 != after {
            return Err("File changed during replacement; inspect the pending checkpoint".into());
        }
        record
            .files
            .get_mut(&relative)
            .ok_or("Missing checkpoint file")?
            .phase = Phase::Applied;
        self.save(&record)?;
        Ok(
            json!({"protected":true,"checkpoint":request.id,"path":relative,"coverage":"native write/edit only"}),
        )
    }
    fn resolve(&self, path: &Relative, entry: &Entry) -> Result<Entry> {
        let current = files::read(&self.path(path)?)?.0;
        let mut entry = entry.clone();
        match &entry.phase {
            Phase::PendingWrite { previous } => {
                if current == entry.after {
                    entry.phase = Phase::Applied;
                } else if current == *previous {
                    entry.after = previous.clone();
                    entry.phase = Phase::Applied;
                } else {
                    return Err(format!("{} conflicts with an interrupted write", path.0).into());
                }
            }
            Phase::Restoring if current == entry.before => entry.phase = Phase::Restored,
            Phase::Restoring | Phase::Applied if current != entry.after => {
                return Err(format!("{} changed since the checkpoint", path.0).into())
            }
            Phase::Restored if current != entry.before => {
                return Err(format!("{} changed after it was restored", path.0).into())
            }
            _ => {}
        }
        Ok(entry)
    }
    pub fn history(&self, limit: usize, session: Option<&str>) -> Result<Value> {
        if !(1..=50).contains(&limit) {
            return Err("Checkpoint history limit must be 1–50".into());
        }
        let _lock = self.lock()?;
        let mut records = Vec::new();
        let mut errors = Vec::new();
        for id in self.ids()? {
            match self.load(&id) {
                Ok(record) => {
                    if session.is_some_and(|session| session != record.session_id) {
                        continue;
                    }
                    let pending = record
                        .files
                        .values()
                        .filter(|e| {
                            matches!(e.phase, Phase::PendingWrite { .. } | Phase::Restoring)
                        })
                        .count();
                    let restored = record
                        .files
                        .values()
                        .filter(|e| matches!(e.phase, Phase::Restored))
                        .count();
                    records.push(json!({"checkpoint":id,"sessionId":record.session_id,"createdAtUnixNanos":record.created_at,"files":record.files.len(),"restoredFiles":restored,"pendingFiles":pending,"active":self.active(&record.id)}));
                }
                Err(error) => errors.push(json!({"checkpoint":id,"error":error.to_string()})),
            }
        }
        records.sort_by(|a, b| {
            b["createdAtUnixNanos"]
                .as_str()
                .cmp(&a["createdAtUnixNanos"].as_str())
        });
        let truncated = records.len() > limit;
        records.truncate(limit);
        Ok(
            json!({"workspace":self.workspace,"coverage":"native write/edit only; excludes shell and extension side effects","checkpoints":records,"errors":errors,"truncated":truncated}),
        )
    }
    pub fn diff(&self, selection: Selection) -> Result<Value> {
        let _lock = self.lock()?;
        let record = self.load(&selection.checkpoint)?;
        let paths = selection.paths(&record)?;
        let mut results = Vec::new();
        let mut diff = String::new();
        let mut truncated = false;
        for path in paths {
            let entry = &record.files[&path];
            let resolved = self.resolve(&path, entry);
            let current_error = resolved.as_ref().err().map(ToString::to_string);
            let entry = resolved.as_ref().unwrap_or(entry);
            let old = self.bytes(&entry.before)?;
            let new = self.bytes(&entry.after)?;
            if diff.len() < 32768 {
                diff.push_str(&format!("--- before/{}\n+++ result/{}\n", path.0, path.0));
                match (std::str::from_utf8(&old), std::str::from_utf8(&new)) {
                    (Ok(a), Ok(b)) if a.len() + b.len() <= 131072 => {
                        diff.push_str(&crate::runtime::edit_diff(a, b).0)
                    }
                    _ if old != new => diff.push_str("[Binary or large file content changed]\n"),
                    _ => {}
                }
                if entry.before != entry.after && old == new {
                    diff.push_str("[File presence or executable mode changed]\n");
                }
            } else {
                truncated = true;
            }
            results.push(json!({"path":path,"before":entry.before,"after":entry.after,"phase":entry.phase,"calls":entry.calls,"conflict":current_error}));
        }
        if diff.len() > 49152 {
            let mut end = 49152;
            while !diff.is_char_boundary(end) {
                end -= 1;
            }
            diff.truncate(end);
            truncated = true;
        }
        Ok(json!({"checkpoint":record.id,"files":results,"diff":diff,"truncated":truncated}))
    }
    pub async fn restore(
        &self,
        selection: Selection,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Value> {
        let _lock = self.lock()?;
        let mut record = self.load(&selection.checkpoint)?;
        if self.active(&record.id) {
            return Err(
                "Checkpoint belongs to an active request; wait for it to finish before restoring"
                    .into(),
            );
        }
        let paths = selection.paths(&record)?;
        let mut originals = BTreeMap::new();
        let mut conflicts = Vec::new();
        // Check every selected path and retained before-image before mutation.
        for path in &paths {
            let entry = &record.files[path];
            match self.resolve(path, entry) {
                Ok(entry) => {
                    originals.insert(path.clone(), self.bytes(&entry.before)?);
                    record.files.insert(path.clone(), entry);
                }
                Err(error) => conflicts.push(error.to_string()),
            }
        }
        if !conflicts.is_empty() {
            return Err(format!(
                "Restore conflict; no selected files were changed:\n{}",
                conflicts.join("\n")
            )
            .into());
        }
        for path in &paths {
            if !matches!(record.files[path].phase, Phase::Restored) {
                record.files.get_mut(path).ok_or("Missing file")?.phase = Phase::Restoring;
            }
        }
        self.save(&record)?;
        let mut restored = Vec::new();
        let mut already = Vec::new();
        for path in paths {
            tokio::task::yield_now().await;
            if *cancel.borrow() {
                return Err("Checkpoint restore cancelled; inspect checkpoint_diff and retry the same checkpoint/paths".into());
            }
            let entry = self.resolve(&path, &record.files[&path])?;
            if matches!(entry.phase, Phase::Restored) {
                already.push(path.clone());
            } else {
                let target = self.path(&path)?;
                let current = files::read(&target)?.0;
                if current != entry.after {
                    return Err(format!(
                        "{} changed during restore; inspect the checkpoint before retrying",
                        path.0
                    )
                    .into());
                }
                if current != entry.before {
                    files::apply(&target, &entry.before, &originals[&path])?;
                }
                if files::read(&target)?.0 != entry.before {
                    return Err("File changed during restore; operation remains pending".into());
                }
                restored.push(path.clone());
            }
            record.files.get_mut(&path).ok_or("Missing file")?.phase = Phase::Restored;
            self.save(&record)?;
        }
        Ok(
            json!({"checkpoint":record.id,"restored":restored,"alreadyRestored":already,"coverage":"native write/edit only; Git state unchanged"}),
        )
    }
    pub fn prune(&self, keep: usize) -> Result<Value> {
        if !(1..=MAX_REQUESTS).contains(&keep) {
            return Err("keep must be 1–256".into());
        }
        let _lock = self.lock()?;
        let mut records = Vec::new();
        for id in self.ids()? {
            records.push(self.load(&id)?);
        }
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        let mut deleted = Vec::new();
        let mut retained = Vec::new();
        let mut resolved_kept = 0;
        for record in records {
            let pending = record
                .files
                .values()
                .any(|e| matches!(e.phase, Phase::PendingWrite { .. } | Phase::Restoring));
            if pending || self.active(&record.id) {
                retained.push(record);
            } else if resolved_kept < keep {
                resolved_kept += 1;
                retained.push(record);
            } else {
                fs::remove_dir_all(self.directory(&record.id))?;
                deleted.push(record.id);
            }
        }
        let mut referenced = BTreeSet::new();
        for record in retained {
            for entry in record.files.values() {
                let mut images = vec![&entry.before, &entry.after];
                if let Phase::PendingWrite { previous } = &entry.phase {
                    images.push(previous);
                }
                for image in images {
                    if let Image::Present { hash, .. } = image {
                        referenced.insert(hash.clone());
                    }
                }
            }
        }
        let mut removed_bytes = 0;
        for entry in fs::read_dir(self.root.join("blobs"))? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().ok_or("Invalid image name")?;
            let temporary = name
                .strip_prefix(".hibiscus-")
                .and_then(|s| s.strip_suffix(".tmp"))
                .is_some_and(|s| Id::try_from(s.to_owned()).is_ok());
            if !files::hash(name) && !temporary {
                return Err("Unexpected checkpoint image; stopped cleanup".into());
            }
            if !referenced.contains(name) {
                removed_bytes += fs::symlink_metadata(entry.path())?.len();
                fs::remove_file(entry.path())?;
            }
        }
        Ok(
            json!({"deletedCheckpoints":deleted,"removedBytes":removed_bytes,"keptAtLeast":keep,"activeAndPendingRetained":true}),
        )
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
