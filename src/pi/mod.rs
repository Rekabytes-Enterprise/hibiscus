pub(crate) mod auth;
pub(crate) mod dialog;
pub(crate) mod rpc;

/// Keep Hibiscus runs limited to Pi's built-in tools. Pi extensions may start
/// background services (including MCP) even when their tools are not active.
pub(crate) fn configure_builtin_tools(command: &mut std::process::Command) {
    command.args(["--no-extensions", "--tools", "read,bash,edit,write"]);
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
