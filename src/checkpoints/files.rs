use super::*;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Component;

pub(super) fn directory(path: &Path) -> Result<()> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => return Ok(()),
        Ok(_) => return Err("Expected a checkpoint directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = path.parent().ok_or("Missing directory parent")?;
    directory(parent)?;
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => {}
        Err(error) => return Err(error.into()),
    }
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub(super) fn read(path: &Path) -> Result<(Image, Vec<u8>)> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => m,
        Ok(_) => return Err("Checkpoint target must be a regular file; symbolic links and special files are unsupported".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound => return Ok((Image::Missing, vec![])),
        Err(e) => return Err(e.into()),
    };
    if metadata.len() > MAX_FILE {
        return Err("Checkpoint file exceeds 16 MiB; no protected edit was made".into());
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let mode = file.metadata()?.permissions().mode() & 0o777;
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("Checkpoint file exceeds 16 MiB".into());
    }
    Ok((Image::present(&bytes, mode), bytes))
}
fn normal(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}
/// Canonical workspace aliases are accepted; links *inside* a target path are
/// refused so a restore cannot follow a directory moved outside the workspace.
pub(super) fn target(workspace: &Path, path: &Path) -> Result<PathBuf> {
    let mut absolute = normal(
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace.join(path)
        }
        .as_path(),
    );
    if !absolute.starts_with(workspace) {
        let mut ancestor = absolute.clone();
        let mut tail = Vec::new();
        while fs::symlink_metadata(&ancestor)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            tail.push(
                ancestor
                    .file_name()
                    .ok_or("Invalid target path")?
                    .to_os_string(),
            );
            if !ancestor.pop() {
                return Err("Invalid target path".into());
            }
        }
        if fs::symlink_metadata(&ancestor)?.file_type().is_symlink() {
            return Err("Refusing a symbolic-link checkpoint target".into());
        }
        absolute = ancestor.canonicalize()?;
        for part in tail.into_iter().rev() {
            absolute.push(part);
        }
    }
    if let Ok(relative) = absolute.strip_prefix(workspace) {
        let mut current = workspace.to_path_buf();
        for part in relative.components() {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(m) if m.file_type().is_symlink() => {
                    return Err("Refusing a symbolic link in a checkpoint path".into())
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(absolute)
}
pub(super) fn atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = path.parent().ok_or("Missing file parent")?;
    directory(parent)?;
    let temp = parent.join(format!(".hibiscus-{}.tmp", Id::new()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
pub(super) fn apply(path: &Path, image: &Image, bytes: &[u8]) -> Result<()> {
    match image {
        Image::Missing => {
            fs::remove_file(path)?;
            fs::File::open(path.parent().ok_or("Missing parent")?)?.sync_all()?;
            Ok(())
        }
        Image::Present { mode, .. } => atomic(path, bytes, *mode),
    }
}
pub(super) fn json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > 262144 {
        return Err("Invalid checkpoint record file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(262145).read_to_end(&mut bytes)?;
    if bytes.len() > 262144 {
        return Err("Checkpoint record exceeds 256 KiB".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}
