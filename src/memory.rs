//! Private plain-text memory layout, legacy import and native-write backups.
use crate::Result;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const MAX_FILE: u64 = 16 * 1024 * 1024;
const REVISIONS: usize = 20;
pub(crate) mod manager;

pub(crate) struct Layout {
    pub global: PathBuf,
    pub project: PathBuf,
    legacy_global: PathBuf,
    legacy_project: PathBuf,
}

impl Layout {
    pub fn new(home: &Path, cwd: &Path) -> Self {
        let hash = format!("{:x}", Sha256::digest(cwd.as_os_str().as_encoded_bytes()));
        let root = home.join("memory");
        Self {
            global: root.join("global/MEMORY.md"),
            project: root.join("projects").join(&hash).join("MEMORY.md"),
            legacy_global: home.join("MEMORY.md"),
            legacy_project: root.join(hash).join("MEMORY.md"),
        }
    }

    pub fn prepare(&self, cwd: &Path) -> Result<()> {
        let root = self.global.parent().unwrap().parent().unwrap();
        directory(root)?;
        directory(&root.join("global"))?;
        directory(&root.join("projects"))?;
        directory(self.project.parent().unwrap())?;
        for (target, old) in [
            (&self.global, &self.legacy_global),
            (&self.project, &self.legacy_project),
        ] {
            let scope = target.parent().ok_or("Memory file has no parent")?;
            directory(&scope.join("revisions"))?;
            // A durable marker prevents deleted memory from being imported again.
            let marker = scope.join(".migration-complete");
            if !exists(&marker)? {
                if !exists(target)? {
                    if let Some(bytes) = read(old)? {
                        publish(target, &bytes)?;
                        // Leave the original in place even after a verified copy.
                        if read(target)?.as_deref() != Some(bytes.as_slice()) {
                            return Err(
                                "Memory migration verification failed; original kept".into()
                            );
                        }
                    }
                }
                publish(&marker, b"1\n")?;
            }
        }
        let metadata = self.project.with_file_name("workspace.json");
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "name": cwd.file_name().unwrap_or_default().to_string_lossy(),
            "path": cwd,
        }))?;
        publish(&metadata, &bytes)?;
        Ok(())
    }
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(true),
        Ok(_) => Err("Memory path must be a regular file, not a link or directory".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => return Ok(()),
        Ok(_) => return Err("Memory directory must not be a symbolic link or file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    directory(path.parent().ok_or("Memory directory has no parent")?)?;
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => directory(path),
        Err(e) => Err(e.into()),
    }
}

fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    if !exists(path)? {
        return Ok(None);
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if file.metadata()?.len() > MAX_FILE {
        return Err("Memory migration exceeds 16 MiB; original kept".into());
    }
    let mut bytes = vec![];
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("Memory migration exceeds 16 MiB; original kept".into());
    }
    Ok(Some(bytes))
}

/// Publish a complete private file atomically without replacing an existing file.
fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    if exists(path)? {
        return Ok(());
    }
    let parent = path.parent().ok_or("Memory file has no parent")?;
    directory(parent)?;
    let temp = parent.join(format!(
        ".memory-{:032x}.tmp",
        crate::runtime::config::unique()
    ));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        match fs::hard_link(&temp, path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                exists(path)?;
            }
            Err(e) => return Err(e.into()),
        }
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(temp);
    result
}

/// Called before a native write replaces an existing active memory file.
pub(crate) fn scope(home: &Path, target: &Path) -> Option<manager::Scope> {
    let root = home.join("memory");
    let Ok(relative) = target.strip_prefix(&root) else {
        return None;
    };
    let parts: Vec<_> = relative.iter().collect();
    let global = relative == Path::new("global/MEMORY.md");
    let project = parts.len() == 3
        && parts[0] == "projects"
        && parts[2] == "MEMORY.md"
        && parts[1].to_str().is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        });
    if global {
        Some(manager::Scope::Global)
    } else if project {
        Some(manager::Scope::Project)
    } else {
        None
    }
}

pub(crate) fn backup(home: &Path, target: &Path, previous: &[u8]) -> Result<()> {
    if scope(home, target).is_none() {
        return Ok(());
    }
    let revisions = target.with_file_name("revisions");
    directory(&revisions)?;
    publish(
        &revisions.join(format!("{:032x}.md", crate::runtime::config::unique())),
        previous,
    )?;
    let mut files = fs::read_dir(&revisions)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    files.retain(|p| {
        p.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
            s.len() == 35
                && s.ends_with(".md")
                && s.as_bytes()[..32].iter().all(|c| c.is_ascii_hexdigit())
        })
    });
    files.sort();
    for path in files.iter().take(files.len().saturating_sub(REVISIONS)) {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
