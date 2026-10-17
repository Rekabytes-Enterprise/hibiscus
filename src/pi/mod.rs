mod approval_settings;
pub(crate) mod auth;
mod context_settings;
pub(crate) mod diagnostics;
pub(crate) mod dialog;
pub(crate) mod error;
pub(crate) mod logout;
pub(crate) mod mermaid;
pub(crate) mod rpc;
mod run;
pub(crate) mod transport;

use std::{fs, io, os::unix::fs::DirBuilderExt, path::PathBuf, sync::OnceLock};

static APPROVAL_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Stage the trusted built-in extensions for this process. Do not load
/// discovered Pi extensions; the files live in a private temporary directory
/// and is removed when the Hibiscus process returns.
pub(crate) struct BuiltinExtensions(PathBuf);
impl BuiltinExtensions {
    pub(crate) fn new() -> io::Result<Self> {
        let context = context_settings::load().map_err(|error| {
            io::Error::other(format!("could not load Hibiscus config.json: {error}"))
        })?;
        let enabled = approval_settings::load().map_err(|error| {
            io::Error::other(format!("could not load Hibiscus approval setting: {error}"))
        })?;
        let root = std::env::temp_dir();
        for attempt in 0..32 {
            let dir = root.join(format!(
                "hibiscus-extensions-{}-{}-{}",
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
                    if let Err(error) = fs::write(&path, include_str!("approval.mjs"))
                        .and_then(|_| {
                            fs::write(
                                dir.join("writing-style.mjs"),
                                include_str!("writing-style.mjs"),
                            )
                        })
                        .and_then(|_| {
                            fs::write(
                                dir.join("model-context.mjs"),
                                include_str!("model-context.mjs"),
                            )?;
                            fs::write(
                                dir.join("context-config.json"),
                                serde_json::to_vec(&context)?,
                            )
                        })
                    {
                        let _ = fs::remove_dir_all(&dir);
                        return Err(error);
                    }
                    if !enabled {
                        if let Err(error) = fs::write(dir.join("approval-disabled"), b"disabled") {
                            let _ = fs::remove_dir_all(&dir);
                            return Err(error);
                        }
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
impl Drop for BuiltinExtensions {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Save changes for future launches and update this process's approval marker.
pub(crate) fn approval_enabled(setting: Option<bool>) -> io::Result<bool> {
    let path = APPROVAL_PATH
        .get()
        .and_then(|extension| extension.parent())
        .ok_or_else(|| io::Error::other("approval extension not staged"))?
        .join("approval-disabled");
    if let Some(enabled) = setting {
        approval_settings::save(enabled).map_err(|error| {
            io::Error::other(format!("could not save Hibiscus approval setting: {error}"))
        })?;
    }
    match setting {
        Some(false) => fs::write(&path, b"disabled")?,
        Some(true) => match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        },
        None => {}
    }
    Ok(!path.try_exists()?)
}

/// Keep built-in tools and only the explicit bundled extensions. Standalone Pi and
/// user-installed extensions remain unchanged.
pub(crate) fn configure_builtin_tools(command: &mut std::process::Command) {
    command.args(["--no-extensions", "--tools", "read,bash,edit,write,goal"]);
    let approval = APPROVAL_PATH
        .get()
        .expect("extensions must be staged before launching Pi");
    command.arg("--extension").arg(approval);
    command
        .arg("--extension")
        .arg(approval.with_file_name("model-context.mjs"));
    command
        .arg("--extension")
        .arg(approval.with_file_name("writing-style.mjs"));
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
