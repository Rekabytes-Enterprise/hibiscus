pub(crate) mod auth;
pub(crate) mod bridge;
pub(crate) mod config;
pub(crate) mod diagnostics;
pub(crate) mod dialog;
mod engine;
pub(crate) mod error;
pub(crate) mod logout;
mod provider;
mod run;
mod session;
pub(crate) mod tool_output;
mod tools;
pub(crate) mod transport;

mod event;
mod oauth;
mod policy;

/// Quota derives from the actual native catalog, including extension authoring tools.
pub(crate) fn native_tool_count() -> usize {
    tools::definitions().len()
}

pub(crate) use tools::edit_diff;
