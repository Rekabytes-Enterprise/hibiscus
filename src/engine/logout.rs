//! Stored credentials that /logout can remove. The prompts live in chat/logout.rs.
use crate::Result;

/// Providers with stored API keys, followed by signed-in ChatGPT and Reka Bytes accounts.
pub(crate) fn stored() -> Result<Vec<String>> {
    let mut providers = super::auth::keys()?.into_keys().collect::<Vec<_>>();
    providers.retain(|id| id != super::config::RETIRED_CODEX_ID);
    providers.sort();
    if super::chatgpt::has_registration() {
        providers.push(super::chatgpt::ID.into());
    }
    if super::reka::account().is_some() {
        providers.push(super::rooxcode::ID.into());
    }
    Ok(providers)
}

/// Removes the stored API key, ChatGPT plan login, or Reka Bytes session.
pub(crate) fn remove(provider: &str) -> Result<()> {
    if provider == super::chatgpt::ID {
        return super::chatgpt::remove();
    }
    if provider == super::rooxcode::ID {
        return super::reka::sign_out();
    }
    let mut keys = super::auth::keys()?;
    if keys.remove(provider).is_some() {
        super::auth::save(&keys)?;
    }
    Ok(())
}
