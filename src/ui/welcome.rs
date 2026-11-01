//! Welcome metadata and command hints shared by both views.
use super::mode::Mode;
pub(crate) struct Context<'a> {
    pub workspace: &'a str,
    pub model: &'a str,
    pub mode: Mode,
    pub has_credentials: bool,
}
pub(crate) fn commands(context: &Context<'_>) -> String {
    let login = if context.has_credentials {
        ""
    } else {
        "/login  ·  "
    };
    format!(
        "{login}{}  ·  /models  ·  /help",
        context.mode.switch_command()
    )
}
