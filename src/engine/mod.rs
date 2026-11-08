//! The agent engine: providers, tools, sessions, configuration and credentials.
//! It never imports terminal code; the terminal talks to it through `client`.
pub(crate) mod agent;
pub(crate) mod auth;
pub(crate) mod catalog;
pub(crate) mod config;
mod context;
mod event;
pub(crate) mod logout;
pub(crate) mod oauth;
mod policy;
pub(crate) mod privacy;
mod provider;
mod rooxcode;
pub(crate) mod session;
mod thinking;
pub(crate) mod tool_output;
mod tools;

pub use oauth::CodexSignIn;
pub(crate) use session::SessionStart;

/// Quota derives from the actual native catalog, including extension authoring tools.
pub(crate) fn native_tool_count() -> usize {
    tools::definitions().len()
}

pub(crate) use tools::edit_diff;

/// Bounded, single-line terminal-safe detail. Drop URLs and common credential
/// representations; never retain the raw provider error in recovery state.
pub(crate) fn safe_message(message: &str) -> String {
    let cleaned: String = message
        .chars()
        .take(4096)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut redact_next = false;
    let mut words = Vec::new();
    for word in cleaned.split_whitespace() {
        let lower = word.to_lowercase();
        let credential = [
            "authorization",
            "bearer",
            "api_key",
            "apikey",
            "api-key",
            "access_token",
            "refresh_token",
            "password",
            "secret",
        ]
        .iter()
        .any(|key| lower.contains(key));
        let redact = redact_next
            || credential
            || lower.contains("http://")
            || lower.contains("https://")
            || lower.contains("sk-")
            || word.len() > 120;
        words.push(if redact { "[redacted]" } else { word });
        redact_next = credential;
    }
    let text = words.join(" ");
    if text.is_empty() {
        "Unknown Runtime error".into()
    } else {
        text.chars().take(600).collect()
    }
}

#[cfg(feature = "fuzzing")]
pub(crate) mod fuzzing;
