//! The persistent shell approval preference is read by the native runtime.
use std::{env, fs, io, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

fn legacy_path() -> Option<PathBuf> {
    let root = env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(root.join("hibiscus/approval.json"))
}

pub(super) fn load() -> io::Result<bool> {
    let path = crate::paths::home()?.join("approval.json");
    let (bytes, migrate) = match fs::read(path) {
        Ok(bytes) => (bytes, false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let Some(legacy) = legacy_path() else {
                return Ok(true);
            };
            match fs::read(legacy) {
                Ok(bytes) => (bytes, true),
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let enabled = value["enabled"].as_bool().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "approval.json must contain a boolean enabled field",
        )
    })?;
    if migrate {
        save(enabled)?;
    }
    Ok(enabled)
}

pub(super) fn save(enabled: bool) -> io::Result<()> {
    let path = crate::paths::home()?.join("approval.json");
    let parent = path.parent().expect("settings path has a parent");
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let temp = parent.join(format!(
        ".approval-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let result = (|| {
        writeln!(file, "{{\"enabled\":{enabled}}}")?;
        file.sync_all()?;
        fs::rename(&temp, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
