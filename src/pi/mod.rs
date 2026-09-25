pub(crate) mod auth;
pub(crate) mod dialog;
pub(crate) mod error;
pub(crate) mod logout;
pub(crate) mod rpc;
mod run;

use std::{fs, io, os::unix::fs::DirBuilderExt, path::PathBuf, sync::OnceLock};

static APPROVAL_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Stage the one trusted built-in extension for this process. Do not load
/// discovered Pi extensions; the file lives in a private temporary directory
/// and is removed when the Hibiscus process returns.
pub(crate) struct ApprovalGuard(PathBuf);
impl ApprovalGuard {
    pub(crate) fn new() -> io::Result<Self> {
        let root = std::env::temp_dir();
        for attempt in 0..32 {
            let dir = root.join(format!(
                "hibiscus-approval-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                attempt
            ));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&dir) {
                Ok(()) => {
                    let path = dir.join("approval.mjs");
                    if let Err(error) = fs::write(&path, include_str!("approval.mjs")) {
                        let _ = fs::remove_dir(&dir);
                        return Err(error);
                    }
                    if APPROVAL_PATH.set(path).is_err() {
                        let _ = fs::remove_dir_all(&dir);
                        return Err(io::Error::other("approval extension already staged"));
                    }
                    return Ok(Self(dir));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other(
            "could not create private approval directory",
        ))
    }
}
impl Drop for ApprovalGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Keep built-in tools and only the explicit approval gate. Standalone Pi and
/// user-installed extensions remain unchanged.
pub(crate) fn configure_builtin_tools(command: &mut std::process::Command) {
    command.args(["--no-extensions", "--tools", "read,bash,edit,write"]);
    command.arg("--extension").arg(
        APPROVAL_PATH
            .get()
            .expect("approval guard must be staged before launching Pi"),
    );
}

/// Pi's installer can bootstrap standalone Node without changing the caller's
/// shell environment. Resolve that Node before starting Pi or the auth helper.
pub(crate) fn ensure_node_path() {
    let supported = std::process::Command::new("node")
        .args([
            "-e",
            "const [major, minor] = process.versions.node.split('.').map(Number); process.exit(major > 22 || (major === 22 && minor >= 19) ? 0 : 1)",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if supported {
        return;
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        });
    let Some(bin) = base.map(|path| path.join("pi-node/current/bin")) else {
        return;
    };
    if !bin.join("node").is_file() {
        return;
    }
    let original = std::env::var_os("PATH").unwrap_or_default();
    let paths = std::iter::once(bin).chain(std::env::split_paths(&original));
    if let Ok(path) = std::env::join_paths(paths) {
        std::env::set_var("PATH", path);
    }
}
