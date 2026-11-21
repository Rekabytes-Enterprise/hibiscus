//! Display data decoded by the runtime adapter, without transport JSON.
pub(crate) fn web_activity(name: &str, running: bool) -> Option<&'static str> {
    match (name, running) {
        ("web_search", true) => Some("Searching the web…"),
        ("web_search", false) => Some("Web search"),
        ("web_fetch", true) => Some("Reading a web source…"),
        ("web_fetch", false) => Some("Web source"),
        _ => None,
    }
}

#[derive(Clone)]
pub(crate) struct Call {
    pub name: String,
    /// File path for file tools, command for the shell tool.
    pub target: String,
}

pub(crate) struct Outcome<'a> {
    pub failed: bool,
    pub diff: Option<&'a str>,
    pub output: &'a str,
    pub exit_code: Option<i64>,
    pub changes: Option<(u64, u64)>,
    pub elapsed: Option<std::time::Duration>,
}
