//! Local memory operations; no provider, session transcript or model calls.
use super::*;

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
    /// The scope's index, `MEMORY.md`.
    pub fn index(&self, scope: Scope) -> &Path {
        match scope {
            Scope::Global => &self.layout.global,
            Scope::Project => &self.layout.project,
        }
    }
    /// The scope's topic file names, such as `release.md`, sorted.
    pub fn topics(&self, scope: Scope) -> Result<Vec<String>> {
        let mut names = vec![];
        for entry in fs::read_dir(self.index(scope).parent().ok_or("No memory folder")?)? {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if name.strip_suffix(".md").is_some_and(topic_name)
                && fs::symlink_metadata(entry.path())?.is_file()
            {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }
    /// A topic file in the scope, by its name with or without `.md`.
    pub fn topic(&self, scope: Scope, name: &str) -> Result<PathBuf> {
        let name = name.strip_suffix(".md").unwrap_or(name);
        if !topic_name(name) {
            return Err("Topic names use lowercase letters, digits and dashes.".into());
        }
        Ok(self.index(scope).with_file_name(format!("{name}.md")))
    }
    fn check(&self, file: &Path) -> Result<()> {
        scope(&self.home, file)
            .map(drop)
            .ok_or_else(|| "Not a memory file.".into())
    }
    pub fn load(&self, file: &Path) -> Result<Option<String>> {
        self.check(file)?;
        read(file)?
            .map(String::from_utf8)
            .transpose()
            .map_err(Into::into)
    }
    pub fn save(&self, file: &Path, expected: Option<&str>, text: &str) -> Result<bool> {
        self.check(file)?;
        if text.len() > 65536 {
            return Err("Memory exceeds 64 KiB; shorten it before saving.".into());
        }
        change(
            &self.home,
            file,
            expected.map(str::as_bytes),
            Some(text.as_bytes()),
        )
    }
    pub fn delete(&self, file: &Path, expected: Option<&str>) -> Result<bool> {
        self.check(file)?;
        change(&self.home, file, expected.map(str::as_bytes), None)
    }
    pub fn revisions(&self, file: &Path) -> Result<Vec<Revision>> {
        self.check(file)?;
        let dir = revisions_dir(file);
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
    pub fn revision(&self, file: &Path, id: &str) -> Result<String> {
        self.check(file)?;
        if !revision_id(id) {
            return Err("Invalid memory revision.".into());
        }
        let bytes =
            read(&revisions_dir(file).join(id))?.ok_or("Memory revision no longer exists.")?;
        Ok(String::from_utf8(bytes)?)
    }
}
fn revision_id(s: &str) -> bool {
    s.len() == 35 && s.ends_with(".md") && s.as_bytes()[..32].iter().all(|c| c.is_ascii_hexdigit())
}

struct Lock(fs::File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
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
        .custom_flags(crate::sys::O_NOFOLLOW | crate::sys::O_NONBLOCK)
        .open(dir.join(".lock"))?;
    if !file.metadata()?.is_file() {
        return Err("Memory lock must be a regular file.".into());
    }
    if file.try_lock().is_err() {
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
        Some(bytes) => crate::engine::config::atomic(target, bytes)?,
        None => {
            fs::remove_file(target)?;
            crate::sys::sync_dir(dir)?;
        }
    }
    Ok(true)
}
