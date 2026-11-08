use crate::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Token {
    pub access: String,
    pub refresh: String,
    pub expires: u64,
    pub account_id: String,
}
impl Token {
    pub(super) fn validate(&self) -> Result<()> {
        if [&self.access, &self.refresh, &self.account_id]
            .iter()
            .any(|s| {
                s.is_empty() || s.len() > 32768 || !s.is_ascii() || s.chars().any(char::is_control)
            })
            || self.expires == 0
        {
            return Err("Invalid stored Codex credential. Use /login to sign in again.".into());
        }
        Ok(())
    }
}
pub(super) struct Store {
    path: PathBuf,
}
impl Store {
    pub(super) fn app() -> Result<Self> {
        Ok(Self {
            path: crate::paths::home()?.join("codex-auth.json"),
        })
    }
    #[cfg(test)]
    pub(super) fn at(path: PathBuf) -> Self {
        Self { path }
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn read(&self) -> Result<Option<Token>> {
        let file = match fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(
                    "Cannot read native Codex credentials. Check codex-auth.json permissions."
                        .into(),
                )
            }
        };
        let meta = file.metadata()?;
        if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 || meta.len() > 128 * 1024 {
            return Err("codex-auth.json must be a bounded regular private file, mode 0600".into());
        }
        let mut bytes = vec![];
        file.take(128 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 128 * 1024 {
            return Err("Codex credential exceeds supported bounds".into());
        }
        let token: Token = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid codex-auth.json. Use /login to replace it.")?;
        token.validate()?;
        Ok(Some(token))
    }
    pub(super) fn write(&self, token: &Token) -> Result<()> {
        token.validate()?;
        super::super::config::atomic(&self.path, &serde_json::to_vec(token)?)
    }
    pub(super) fn try_lock(&self) -> Result<fs::File> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(self.path.parent().ok_or("Invalid credential path")?)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.path.with_extension("lock"))?;
        if !file.metadata()?.is_file() || file.metadata()?.permissions().mode() & 0o077 != 0 {
            return Err("Codex credential lock must be a regular private file".into());
        }
        // Advisory locks survive atomic auth-file replacement and cover processes.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(file)
    }
    pub(super) async fn lock(&self) -> Result<fs::File> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            match self.try_lock() {
                Ok(lock) => return Ok(lock),
                Err(e)
                    if e.downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock) =>
                {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(e);
                    }
                    tokio::time::sleep(Duration::from_millis(40)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}
