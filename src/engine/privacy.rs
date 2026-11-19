//! The user's choice about model providers training on their prompts, stored in
//! `privacy.json` in the Hibiscus home. Shared by the desktop app and the terminal.
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Privacy {
    /// Ask OpenRouter to use only providers that don't store or train on prompts.
    #[serde(default)]
    deny_training: bool,
}

fn path() -> Result<std::path::PathBuf> {
    Ok(crate::paths::home()?.join("privacy.json"))
}

/// Whether the user asked providers not to train on their prompts. A missing or
/// unreadable file means the default: no preference sent.
pub(crate) fn training_denied() -> bool {
    path()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Privacy>(&bytes).ok())
        .is_some_and(|privacy| privacy.deny_training)
}

pub(crate) fn set_training_denied(deny: bool) -> Result<()> {
    let bytes = serde_json::to_vec(&Privacy {
        deny_training: deny,
    })?;
    super::config::atomic(&path()?, &bytes)
}

/// Adds OpenRouter's data-collection preference to a request body. Other
/// providers have no such request field, so their bodies are unchanged.
pub(super) fn apply(provider_id: &str, deny: bool, body: &mut Value) {
    if deny && provider_id == "openrouter" {
        body["provider"] = json!({"data_collection": "deny"});
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_openrouter_requests_carry_the_preference_and_only_when_denied() {
        let mut body = json!({"model": "m"});
        apply("openrouter", false, &mut body);
        assert!(body.get("provider").is_none());
        apply("openai", true, &mut body);
        assert!(body.get("provider").is_none());
        apply("openrouter", true, &mut body);
        assert_eq!(body["provider"], json!({"data_collection": "deny"}));
    }

    #[test]
    fn stored_choice_round_trips_and_defaults_to_allow() {
        assert!(
            !serde_json::from_slice::<Privacy>(b"{}")
                .unwrap()
                .deny_training
        );
        let stored = serde_json::to_vec(&Privacy {
            deny_training: true,
        })
        .unwrap();
        assert_eq!(stored, br#"{"denyTraining":true}"#);
        assert!(
            serde_json::from_slice::<Privacy>(&stored)
                .unwrap()
                .deny_training
        );
    }
}
