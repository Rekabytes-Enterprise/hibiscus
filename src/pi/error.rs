//! Classify for presentation only. Pi owns retry policy; strings never decide
//! whether a connection is usable or whether a prompt should be replayed.
use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorSource {
    Command,
    Assistant,
    Compaction,
    Extension,
    Transport,
    Protocol,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorCategory {
    Authentication,
    Access,
    Billing,
    RateLimit,
    Unavailable,
    Network,
    ContextLimit,
    InvalidRequest,
    Cancelled,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecoveryAction {
    KeepChatting,
    Reconnect,
}

#[derive(Clone, Debug)]
pub(crate) struct ChatError {
    pub(crate) source: ErrorSource,
    pub(crate) category: ErrorCategory,
    pub(crate) message: String,
    pub(crate) command_id: Option<String>,
}

impl ChatError {
    pub(crate) fn new(source: ErrorSource, message: &str) -> Self {
        Self {
            source,
            category: classify(message),
            message: safe_message(message),
            command_id: None,
        }
    }
    pub(crate) fn cancelled(command: &str) -> Self {
        let mut error = Self::new(ErrorSource::Command, &format!("Cancelled {command}"));
        error.category = ErrorCategory::Cancelled;
        error
    }
    pub(crate) fn recovery(&self) -> RecoveryAction {
        match self.source {
            ErrorSource::Transport | ErrorSource::Protocol => RecoveryAction::Reconnect,
            _ => RecoveryAction::KeepChatting,
        }
    }
    pub(crate) fn hint(&self) -> &'static str {
        if self.recovery() == RecoveryAction::Reconnect {
            return "Pi disconnected. The last operation's outcome may be unknown. Use /reconnect; nothing is resent automatically.";
        }
        if self.source == ErrorSource::Local {
            return "Check the local configuration or command input. You can keep chatting.";
        }
        match self.category {
            ErrorCategory::Authentication => "Use /login or check your Pi credentials.",
            ErrorCategory::Access => "Check model access or choose another model with /models.",
            ErrorCategory::Billing => {
                "Check provider billing/subscription/quota, or switch with /models."
            }
            ErrorCategory::RateLimit => {
                "Pi has finished its automatic work. Wait before retrying, or use /models."
            }
            ErrorCategory::Unavailable | ErrorCategory::Network => {
                "Check connectivity or provider availability; you can retry later or use /models."
            }
            ErrorCategory::ContextLimit => {
                "Reduce the prompt, choose a larger-context model, or start /new."
            }
            ErrorCategory::InvalidRequest => {
                "Check your request, image support, or selected model."
            }
            ErrorCategory::Cancelled => "You can keep chatting.",
            ErrorCategory::Unknown => "You can keep chatting or choose another model with /models.",
        }
    }
}
impl fmt::Display for ChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.source, self.message)
    }
}
impl Error for ChatError {}

pub(crate) fn local(error: impl fmt::Display) -> ChatError {
    ChatError::new(ErrorSource::Local, &error.to_string())
}

pub(crate) fn transport(error: impl fmt::Display) -> ChatError {
    ChatError::new(
        ErrorSource::Transport,
        &format!("Pi connection failed: {error}"),
    )
}
pub(crate) fn protocol(message: &str) -> ChatError {
    ChatError::new(ErrorSource::Protocol, message)
}

fn classify(message: &str) -> ErrorCategory {
    let text = message.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| text.contains(word));
    // Billing/quota wins over 429; context overflow wins over transient wording.
    if has(&[
        "insufficient_quota",
        "billing",
        "subscription",
        "quota exceeded",
        "out of budget",
        "available balance",
        "monthly usage limit",
        "gousagelimiterror",
        "freeusagelimiterror",
    ]) {
        ErrorCategory::Billing
    } else if has(&[
        "context window",
        "context length",
        "context_length",
        "context limit",
        "too many tokens",
        "prompt is too long",
        "context overflow",
    ]) {
        ErrorCategory::ContextLimit
    } else if has(&[
        "unauthorized",
        "invalid api key",
        "api key not found",
        "no api key",
        "authentication",
        "token expired",
        "expired token",
        "401",
    ]) {
        ErrorCategory::Authentication
    } else if has(&["forbidden", "permission denied", "access denied", "403"]) {
        ErrorCategory::Access
    } else if has(&[
        "rate limit",
        "rate_limit",
        "too many requests",
        "429",
        "resourceexhausted",
    ]) {
        ErrorCategory::RateLimit
    } else if has(&[
        "overloaded",
        "high demand",
        "unavailable",
        "500",
        "502",
        "503",
        "504",
        "529",
        "server error",
    ]) {
        ErrorCategory::Unavailable
    } else if has(&[
        "network",
        "connection",
        "fetch failed",
        "enotfound",
        "eai_again",
        "timeout",
        "timed out",
        "socket",
        "terminated",
        "stream ended",
    ]) {
        ErrorCategory::Network
    } else if has(&[
        "invalid request",
        "unsupported",
        "no model",
        "model not found",
        "400",
        "404",
        "413",
    ]) {
        ErrorCategory::InvalidRequest
    } else {
        ErrorCategory::Unknown
    }
}

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
        "Unknown Pi error".into()
    } else {
        text.chars().take(600).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classification_is_advisory_and_billing_wins_over_throttling() {
        for (message, category) in [
            ("429 insufficient_quota", ErrorCategory::Billing),
            ("429 rate limit", ErrorCategory::RateLimit),
            ("401 unauthorized", ErrorCategory::Authentication),
            ("403 forbidden", ErrorCategory::Access),
            ("503 overloaded", ErrorCategory::Unavailable),
            ("fetch failed", ErrorCategory::Network),
            ("context length exceeded", ErrorCategory::ContextLimit),
            ("unsupported image", ErrorCategory::InvalidRequest),
            ("something new", ErrorCategory::Unknown),
        ] {
            let error = ChatError::new(ErrorSource::Assistant, message);
            assert_eq!(error.category, category);
            assert_eq!(error.recovery(), RecoveryAction::KeepChatting);
        }
        assert_eq!(transport("closed").recovery(), RecoveryAction::Reconnect);
    }
    #[test]
    fn error_details_are_bounded_and_terminal_safe() {
        let safe = safe_message("\x1b[2J429 authorization: Bearer secret-value https://example.com/?token=private sk-private");
        assert!(!safe.contains('\x1b'));
        for secret in ["secret-value", "example.com", "sk-private"] {
            assert!(!safe.contains(secret));
        }
        assert!(safe_message(&"word ".repeat(1000)).chars().count() <= 600);
    }
}
