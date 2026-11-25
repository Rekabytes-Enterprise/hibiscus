//! The agent engine: providers, tools, sessions, configuration and credentials.
//! It never imports terminal code; the terminal talks to it through `client`.
pub(crate) mod agent;
pub(crate) mod auth;
mod batch_edit;
mod capped;
pub(crate) mod catalog;
pub(crate) mod chatgpt;
pub(crate) mod config;
mod context;
mod edit_match;
pub(crate) mod elevenlabs_auth;
mod event;
mod goal;
mod local_http_check;
pub(crate) mod logout;
pub(crate) mod oauth;
mod paging;
mod policy;
pub(crate) mod privacy;
mod provider;
pub(crate) mod reka;
mod rooxcode;
mod search;
pub(crate) mod session;
mod session_search;
mod stall;
mod thinking;
mod tool_call;
pub(crate) mod tool_output;
mod tools;

pub use reka::RekaSignIn;
pub(crate) use session::SessionStart;

/// Quota derives from the actual native catalog, including extension authoring tools.
pub(crate) fn native_tool_count() -> usize {
    tools::definitions(None, tools::Surface::Canvas, false).len()
}

pub(crate) use tools::edit_diff;
pub(crate) use tools::BROWSER_SESSION;

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

fn uuid_v4() -> crate::Result<String> {
    use rand::TryRng;
    let mut bytes = [0u8; 16];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| "Cannot generate a random UUID")?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}
