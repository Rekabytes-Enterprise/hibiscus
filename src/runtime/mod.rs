mod approval_settings;
pub(crate) mod auth;
pub(crate) mod bridge;
pub(crate) mod config;
pub(crate) mod diagnostics;
pub(crate) mod dialog;
mod engine;
pub(crate) mod error;
pub(crate) mod logout;
pub(crate) mod mermaid;
mod provider;
mod run;
mod session;
mod tools;
pub(crate) mod transport;

pub(crate) fn approval_enabled(setting: Option<bool>) -> std::io::Result<bool> {
    if let Some(enabled) = setting {
        approval_settings::save(enabled)?;
    }
    approval_settings::load()
}
mod event;
mod oauth;
mod policy;
