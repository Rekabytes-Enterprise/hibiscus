use crate::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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
/// A private, locked credential file in the Hibiscus home. Codex uses
/// `codex-auth.json`; the Reka Bytes sign-in (engine/reka.rs) its own file.
pub(in crate::engine) struct Store {
    path: PathBuf,
}
impl Store {
    pub(super) fn app() -> Result<Self> {
        Self::named("codex-auth.json")
    }
    pub(in crate::engine) fn named(file: &str) -> Result<Self> {
        Ok(Self {
            path: crate::paths::home()?.join(file),
        })
    }
    #[cfg(test)]
    pub(in crate::engine) fn at(path: PathBuf) -> Self {
        Self { path }
    }
    pub(in crate::engine) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn read(&self) -> Result<Option<Token>> {
        let token: Option<Token> = self.read_json()?;
        if let Some(token) = &token {
            token.validate()?;
        }
        Ok(token)
    }
    pub(in crate::engine) fn read_json<T: DeserializeOwned>(&self) -> Result<Option<T>> {
        let file = match fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(format!(
                    "Cannot read {}. Check its permissions.",
                    self.path.display()
                )
                .into())
            }
        };
        let meta = file.metadata()?;
        if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 || meta.len() > 128 * 1024 {
            return Err(format!(
                "{} must be a bounded regular private file, mode 0600",
                self.path.display()
            )
            .into());
        }
        let mut bytes = vec![];
        file.take(128 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 128 * 1024 {
            return Err("Stored credential exceeds supported bounds".into());
        }
        let value = serde_json::from_slice(&bytes)
            .map_err(|_| format!("Invalid {}. Use /login to replace it.", self.path.display()))?;
        Ok(Some(value))
    }
    pub(super) fn write(&self, token: &Token) -> Result<()> {
        token.validate()?;
        self.write_json(token)
    }
    pub(in crate::engine) fn write_json<T: Serialize>(&self, value: &T) -> Result<()> {
        super::super::config::atomic(&self.path, &serde_json::to_vec(value)?)
    }
    pub(in crate::engine) fn try_lock(&self) -> Result<fs::File> {
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
            return Err("Credential lock must be a regular private file".into());
        }
        // Advisory locks survive atomic auth-file replacement and cover processes.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(file)
    }
    pub(in crate::engine) async fn lock(&self) -> Result<fs::File> {
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
