pub(crate) mod auth;
pub(crate) mod dialog;
pub(crate) mod rpc;

/// Keep Hibiscus runs limited to Pi's built-in tools. Pi extensions may start
/// background services (including MCP) even when their tools are not active.
pub(crate) fn configure_builtin_tools(command: &mut std::process::Command) {
    command.args(["--no-extensions", "--tools", "read,bash,edit,write"]);
}
