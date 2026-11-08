//! Stored credentials that /logout can remove. The prompts live in chat/logout.rs.
use crate::Result;

/// Providers with a stored API key, sorted, then openai-codex when signed in.
pub(crate) fn stored() -> Result<Vec<String>> {
    let mut providers = super::auth::keys()?.into_keys().collect::<Vec<_>>();
    providers.sort();
    if super::oauth::stored_access_token().is_ok() {
        providers.push("openai-codex".into());
    }
    Ok(providers)
}

/// Removes the stored API key, or the Codex login for openai-codex.
pub(crate) fn remove(provider: &str) -> Result<()> {
    if provider == "openai-codex" {
        return super::oauth::remove();
    }
    let mut keys = super::auth::keys()?;
    if keys.remove(provider).is_some() {
        super::auth::save(&keys)?;
    }
    Ok(())
}
