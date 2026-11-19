//! "Sign in with ElevenLabs" for the canvas, the way Claude Code connects:
//! OAuth 2.1 with PKCE against ElevenLabs' hosted MCP server, as a public
//! client identified by a Client ID Metadata Document. Hibiscus's document is
//! served by GitHub Pages from the repo's gh-pages branch. The token is bound to
//! the MCP server (`resource`) and kept in `connectors.json`.
use super::oauth::protocol::{now, Pending};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Hibiscus's client ID: the address of its metadata document.
pub(crate) const CLIENT_ID: &str =
    "https://rekabytes-enterprise.github.io/hibiscus/oauth/elevenlabs.json";
const AUTHORIZE: &str = "https://elevenlabs.io/app/oauth/authorize";
const TOKEN: &str = "https://api.us.elevenlabs.io/v1/oauth/token";
const REVOKE: &str = "https://api.us.elevenlabs.io/v1/oauth/revoke";
/// The MCP server the token is for.
pub(crate) const RESOURCE: &str = "https://api.elevenlabs.io/v1/mcp";
/// Everything the canvas uses: speech, voices, the creative flows that hosted
/// speech runs as, image and video generation, and speech history. Not the
/// ElevenLabs Agents (convai) or Ads scopes, which Hibiscus has no use for.
pub(crate) const SCOPE: &str = "text_to_speech voice_generation user_read flows image_video_generation speech_history_read pronunciation_dictionaries_read";
const STORE: &str = "elevenlabs-oauth";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Token {
    access: String,
    refresh: Option<String>,
    /// Unix seconds.
    expires: u64,
}

fn load() -> Result<Option<Token>> {
    match super::auth::connector_key(STORE)? {
        Some(json) => {
            Ok(Some(serde_json::from_str(&json).map_err(|_| {
                "Saved ElevenLabs sign-in is damaged. Sign in again."
            })?))
        }
        None => Ok(None),
    }
}

fn save(token: Option<&Token>) -> Result<()> {
    super::auth::save_connector_key(STORE, token.map(serde_json::to_string).transpose()?)
}

pub(crate) fn signed_in() -> bool {
    matches!(load(), Ok(Some(_)))
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?)
}

#[derive(Deserialize)]
struct Receipt {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

/// Sends a token request; ElevenLabs answers errors as `{"error":…,"error_description":…}`.
async fn tokens(form: &[(&str, &str)]) -> Result<Token> {
    let response = client()?.post(TOKEN).form(form).send().await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v["error_description"]
                    .as_str()
                    .or(v["error"].as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        return Err(format!("ElevenLabs sign-in failed: {detail}").into());
    }
    let receipt: Receipt =
        serde_json::from_str(&body).map_err(|_| "ElevenLabs sent no usable token")?;
    if receipt.access_token.is_empty() || receipt.access_token.len() > 16384 {
        return Err("ElevenLabs sent no usable token".into());
    }
    Ok(Token {
        access: receipt.access_token,
        refresh: receipt.refresh_token,
        // Without a lifetime, treat the token as good for an hour and refresh after.
        expires: now().saturating_add(receipt.expires_in.unwrap_or(3600).min(86400 * 30)),
    })
}

/// A valid access token, refreshed first when it is about to expire.
pub(crate) async fn access_token() -> Result<String> {
    let token = load()?.ok_or("Sign in to ElevenLabs in Settings first.")?;
    if token.expires > now() + 60 {
        return Ok(token.access);
    }
    let refresh = token
        .refresh
        .clone()
        .ok_or("Your ElevenLabs sign-in expired. Sign in again in Settings.")?;
    let mut fresh = tokens(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh),
        ("client_id", CLIENT_ID),
        ("resource", RESOURCE),
    ])
    .await
    .map_err(|e| format!("{e}. Sign in again in Settings."))?;
    // Servers may keep the refresh token instead of rotating it.
    fresh.refresh = fresh.refresh.or(token.refresh);
    save(Some(&fresh))?;
    Ok(fresh.access)
}

/// Forgets the sign-in, revoking the refresh token at ElevenLabs when it can.
pub(crate) async fn sign_out() -> Result<()> {
    if let Ok(Some(token)) = load() {
        let revoke = token.refresh.as_deref().unwrap_or(&token.access).to_owned();
        if let Ok(client) = client() {
            let _ = client
                .post(REVOKE)
                .form(&[("token", revoke.as_str()), ("client_id", CLIENT_ID)])
                .send()
                .await;
        }
    }
    save(None)
}

/// The authorization URL for a pending sign-in.
fn authorize_url(pending: &Pending) -> Result<String> {
    let mut url = reqwest::Url::parse(AUTHORIZE)?;
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", pending.redirect.as_str()),
        ("scope", SCOPE),
        ("state", pending.state.as_str()),
        ("code_challenge", &pending.challenge()),
        ("code_challenge_method", "S256"),
        ("resource", RESOURCE),
    ]);
    Ok(url.into())
}

/// A browser sign-in in progress. Open `url`, then await `finish`; dropping
/// the future cancels it and frees the port.
pub struct ElevenLabsSignIn {
    pending: Pending,
    url: String,
    paste: tokio::sync::mpsc::Sender<String>,
    pasted: tokio::sync::mpsc::Receiver<String>,
}

impl ElevenLabsSignIn {
    /// Listens on a free loopback port: the client's document allows
    /// `http://127.0.0.1/callback` on any port, as Claude Code's does.
    pub fn start() -> Result<Self> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let pending = Pending::new(
            listener,
            format!("http://127.0.0.1:{port}/callback"),
            "ElevenLabs",
        )?;
        let url = authorize_url(&pending)?;
        let (paste, pasted) = tokio::sync::mpsc::channel(1);
        Ok(Self {
            pending,
            url,
            paste,
            pasted,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Takes a redirect URL pasted by the user when the browser can't reach the app.
    pub fn paste(&self) -> tokio::sync::mpsc::Sender<String> {
        self.paste.clone()
    }

    /// Waits up to ten minutes for the browser, then stores the token.
    pub async fn finish(self) -> Result<()> {
        let Self {
            pending,
            paste,
            pasted,
            ..
        } = self;
        let _paste = paste;
        let code = tokio::time::timeout(Duration::from_secs(600), pending.wait(pasted))
            .await
            .map_err(|_| "ElevenLabs sign-in timed out. Try again.")??;
        let token = tokens(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &pending.redirect),
            ("client_id", CLIENT_ID),
            ("code_verifier", &pending.verifier),
            ("resource", RESOURCE),
        ])
        .await?;
        save(Some(&token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_asks_for_the_mcp_server_with_pkce() {
        let sign_in = ElevenLabsSignIn::start().unwrap();
        let url = reqwest::Url::parse(sign_in.url()).unwrap();
        assert_eq!(url.as_str().split('?').next(), Some(AUTHORIZE));
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], CLIENT_ID);
        assert_eq!(query["resource"], RESOURCE);
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["code_challenge"], sign_in.pending.challenge());
        assert_eq!(query["state"], sign_in.pending.state);
        assert!(query["redirect_uri"].starts_with("http://127.0.0.1:"));
        assert!(query["redirect_uri"].ends_with("/callback"));
    }
}
