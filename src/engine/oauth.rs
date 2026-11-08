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
        Self::with(
            protocol::Pending::bind()?,
            &protocol::Endpoints::production(),
        )
    }
    fn with(pending: protocol::Pending, endpoints: &protocol::Endpoints) -> Result<Self> {
        let url = pending.url(endpoints)?.to_string();
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
        self.complete_at(pasted, &protocol::Endpoints::production())
            .await
    }
    async fn complete_at(
        self,
        pasted: tokio::sync::mpsc::Receiver<String>,
        endpoints: &protocol::Endpoints,
    ) -> Result<Token> {
        self.pending.complete(pasted, endpoints).await
    }
}

/// Stores a completed sign-in. Cancelled or failed attempts never reach this,
/// so the previous login is preserved.
pub(crate) fn save(token: &Token) -> Result<()> {
    save_at(&store::Store::app()?, token)
}
fn save_at(store: &store::Store, token: &Token) -> Result<()> {
    let _lock = store.try_lock()?;
    store.write(token)
}

/// Codex browser sign-in for the desktop app: the steps of [`BrowserSignIn`],
/// plus a paste channel, the 10-minute limit and saving. The terminal drives
/// `BrowserSignIn` itself (chat/login.rs).
pub struct CodexSignIn {
    sign_in: BrowserSignIn,
    paste: tokio::sync::mpsc::Sender<String>,
    pasted: tokio::sync::mpsc::Receiver<String>,
}
impl CodexSignIn {
    /// Listens on `localhost:1455` and builds the sign-in link. Open it with `url`.
    pub fn start() -> Result<Self> {
        Ok(Self::from(BrowserSignIn::start()?))
    }
    fn from(sign_in: BrowserSignIn) -> Self {
        let (paste, pasted) = tokio::sync::mpsc::channel(1);
        Self {
            sign_in,
            paste,
            pasted,
        }
    }
    #[cfg(test)]
    fn with(pending: protocol::Pending, endpoints: &protocol::Endpoints) -> Result<Self> {
        Ok(Self::from(BrowserSignIn::with(pending, endpoints)?))
    }
    /// The fixed-origin authorization URL to open in the browser.
    pub fn url(&self) -> &str {
        self.sign_in.url()
    }
    /// Sends a redirect URL pasted by the user, for when the browser cannot reach the app.
    pub fn paste(&self) -> tokio::sync::mpsc::Sender<String> {
        self.paste.clone()
    }
    /// Waits for the browser (or a pasted redirect), then saves the login.
    /// Dropping the future cancels and frees the port; failures keep the previous login.
    pub async fn finish(self) -> Result<()> {
        self.finish_at(&store::Store::app()?, &protocol::Endpoints::production())
            .await
    }
    async fn finish_at(self, store: &store::Store, endpoints: &protocol::Endpoints) -> Result<()> {
        let Self {
            sign_in,
            paste,
            pasted,
        } = self;
        let _paste = paste; // keeps the paste channel open while the caller holds no sender
        let token = tokio::time::timeout(
            std::time::Duration::from_secs(600),
            sign_in.complete_at(pasted, endpoints),
        )
        .await
        .map_err(|_| "Codex sign-in timed out. Try again.")??;
        save_at(store, &token)
    }
}

#[cfg(test)]
mod tests;

#[cfg(feature = "fuzzing")]
pub(super) fn fuzz_callback(bytes: &[u8]) {
    protocol::fuzz_callback(bytes);
}
