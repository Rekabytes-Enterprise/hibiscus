pub(crate) mod auth;
pub(crate) mod bridge;
pub(crate) mod catalog;
pub(crate) mod config;
mod context;
pub(crate) mod diagnostics;
pub(crate) mod dialog;
pub(crate) mod embed;
mod engine;
pub(crate) mod error;
pub(crate) mod logout;
mod provider;
mod rooxcode;
mod run;
mod session;
mod thinking;
pub(crate) mod tool_output;
mod tools;
pub(crate) mod transport;

mod event;
mod oauth;
pub use oauth::CodexSignIn;
mod policy;

/// Quota derives from the actual native catalog, including extension authoring tools.
pub(crate) fn native_tool_count() -> usize {
    tools::definitions().len()
}

pub(crate) use tools::edit_diff;

#[cfg(feature = "fuzzing")]
pub(crate) mod fuzzing;
