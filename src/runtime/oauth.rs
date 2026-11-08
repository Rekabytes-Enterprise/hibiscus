//! Native Codex subscription OAuth. No external agent or credential-store import.
//! Protocol reference: Pi 1.1.0's legacy openai-codex adapter, not its separate
//! dynamically registered openai-chatgpt flow. Client ID is a public identifier.
mod protocol;
mod store;

use crate::Result;
pub(super) const RESOURCE: &str = "https://chatgpt.com/backend-api/codex";
pub(crate) use store::Token;

pub(crate) fn stored_access_token() -> Result<String> {
    store::Store::app()?
        .read()?
        .map(|t| t.access)
        .ok_or_else(|| {
            "Authentication required for openai-codex. Use /login → Browser Login → Codex.".into()
        })
}

pub(super) async fn credential() -> Result<Token> {
    credential_at(&store::Store::app()?, &protocol::Endpoints::production()).await
}

async fn credential_at(store: &store::Store, endpoints: &protocol::Endpoints) -> Result<Token> {
    let _lock = store.lock().await?;
    let token = store.read()?.ok_or("Codex is signed out. Use /login.")?;
    if token.expires > protocol::now().saturating_add(60) {
        return Ok(token);
    }
    let refreshed = protocol::refresh(endpoints, &token).await?;
    store.write(&refreshed)?;
    Ok(refreshed)
}

pub(crate) fn remove() -> Result<()> {
    let store = store::Store::app()?;
    let _lock = store.try_lock()?;
    match std::fs::remove_file(store.path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// One Codex browser sign-in, without any UI. The /login screen that drives it
/// lives in chat/login.rs.
pub(crate) struct BrowserSignIn {
    pending: protocol::Pending,
    url: String,
}
impl BrowserSignIn {
    /// Checks the credential store, listens on the loopback callback port and
    /// builds the validated fixed-origin authorization URL.
    pub(crate) fn start() -> Result<Self> {
        store::Store::app()?;
        let pending = protocol::Pending::bind()?;
        let url = pending.url(&protocol::Endpoints::production())?.to_string();
        Ok(Self { pending, url })
    }
    pub(crate) fn url(&self) -> &str {
        &self.url
    }
    /// Waits for the browser callback or a pasted redirect URL, then exchanges the code.
    pub(crate) async fn complete(
        self,
        pasted: tokio::sync::mpsc::Receiver<String>,
    ) -> Result<Token> {
        self.pending
            .complete(pasted, &protocol::Endpoints::production())
            .await
    }
}

/// Stores a completed sign-in. Cancelled or failed attempts never reach this,
/// so the previous login is preserved.
pub(crate) fn save(token: &Token) -> Result<()> {
    let store = store::Store::app()?;
    let _lock = store.try_lock()?;
    store.write(token)
}

#[cfg(test)]
mod tests;

#[cfg(feature = "fuzzing")]
pub(super) fn fuzz_callback(bytes: &[u8]) {
    protocol::fuzz_callback(bytes);
}
