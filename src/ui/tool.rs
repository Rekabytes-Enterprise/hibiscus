//! Display data decoded by the runtime adapter, without transport JSON.
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
