use std::{env, io, path::PathBuf};

/// All runtime data belongs to Hibiscus; no external agent directory is read.
pub(crate) fn home() -> io::Result<PathBuf> {
    if let Some(custom) = env::var_os("HIBISCUS_HOME").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(custom);
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "HIBISCUS_HOME must be an absolute path",
            ));
        }
        return Ok(path);
    }
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(|home| PathBuf::from(home).join(".hibiscus"))
        .ok_or_else(|| io::Error::other("HOME or HIBISCUS_HOME is required"))
}
