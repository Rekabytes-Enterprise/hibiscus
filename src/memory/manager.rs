//! Local memory operations; no provider, session transcript or model calls.
use super::*;
use std::os::fd::AsRawFd;

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Scope {
    Global,
    Project,
}
impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Project => "Current Project",
        }
    }
}

pub(crate) struct Manager {
    home: PathBuf,
    layout: Layout,
}
pub(crate) struct Revision {
    pub id: String,
    pub bytes: u64,
}
impl Manager {
    pub fn open(home: &Path, cwd: &Path) -> Result<Self> {
        let layout = Layout::new(home, cwd);
        layout.prepare(cwd)?;
        Ok(Self {
            home: home.to_path_buf(),
            layout,
        })
    }
    pub fn path(&self, scope: Scope) -> &Path {
        match scope {
            Scope::Global => &self.layout.global,
            Scope::Project => &self.layout.project,
        }
    }
    pub fn load(&self, scope: Scope) -> Result<Option<String>> {
        read(self.path(scope))?
            .map(String::from_utf8)
            .transpose()
            .map_err(Into::into)
    }
    pub fn save(&self, scope: Scope, expected: Option<&str>, text: &str) -> Result<bool> {
        if text.len() > 65536 {
            return Err("Memory exceeds 64 KiB; shorten it before saving.".into());
        }
        change(
            &self.home,
            self.path(scope),
            expected.map(str::as_bytes),
            Some(text.as_bytes()),
        )
    }
    pub fn delete(&self, scope: Scope, expected: Option<&str>) -> Result<bool> {
        change(
            &self.home,
            self.path(scope),
            expected.map(str::as_bytes),
            None,
        )
    }
    pub fn revisions(&self, scope: Scope) -> Result<Vec<Revision>> {
        let dir = self.path(scope).with_file_name("revisions");
        directory(&dir)?;
        let mut result = vec![];
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(id) = name.to_str().filter(|s| revision_id(s)) else {
                continue;
            };
            let meta = fs::symlink_metadata(entry.path())?;
            if meta.is_file() {
                result.push(Revision {
                    id: id.into(),
                    bytes: meta.len(),
                });
            }
        }
        result.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(result)
    }
    pub fn revision(&self, scope: Scope, id: &str) -> Result<String> {
        if !revision_id(id) {
            return Err("Invalid memory revision.".into());
        }
        let bytes = read(&self.path(scope).with_file_name("revisions").join(id))?
            .ok_or("Memory revision no longer exists.")?;
        Ok(String::from_utf8(bytes)?)
    }
}
fn revision_id(s: &str) -> bool {
    s.len() == 35 && s.ends_with(".md") && s.as_bytes()[..32].iter().all(|c| c.is_ascii_hexdigit())
}

struct Lock(fs::File);
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// Serialize native memory changes across workspaces and reject stale editors.
pub(crate) fn change(
    home: &Path,
    target: &Path,
    expected: Option<&[u8]>,
    next: Option<&[u8]>,
) -> Result<bool> {
    let dir = target.parent().ok_or("Memory path has no parent")?;
    directory(dir)?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(dir.join(".lock"))?;
    if !file.metadata()?.is_file() {
        return Err("Memory lock must be a regular file.".into());
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Memory is being updated; reopen it and try again.".into());
    }
    let _lock = Lock(file);
    let current = read(target)?;
    if current.as_deref() != expected {
        return Err(
            "Memory changed while open; reopen it before saving. Your changes were not applied."
                .into(),
        );
    }
    if current.as_deref() == next {
        return Ok(false);
    }
    if let Some(old) = &current {
        backup(home, target, old)?;
    }
    match next {
        Some(bytes) => crate::runtime::config::atomic(target, bytes)?,
        None => {
            fs::remove_file(target)?;
            fs::File::open(dir)?.sync_all()?;
        }
    }
    Ok(true)
}
