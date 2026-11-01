use super::Token;
use crate::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc::Receiver,
};

// Public Codex OAuth client identifier, not a secret or a Pi client identity.
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const REDIRECT: &str = "http://localhost:1455/auth/callback";
pub(super) struct Endpoints {
    authorize: String,
    token: String,
}
impl Endpoints {
    pub(super) fn production() -> Self {
        Self {
            authorize: "https://auth.openai.com/oauth/authorize".into(),
            token: "https://auth.openai.com/oauth/token".into(),
        }
    }
    #[cfg(test)]
    pub(super) fn local(base: &str) -> Self {
        Self {
            authorize: format!("{base}/authorize"),
            token: format!("{base}/token"),
        }
    }
}
pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn random() -> Result<String> {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| "Cannot obtain secure randomness for Codex login")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
pub(super) struct Pending {
    listener: std::net::TcpListener,
    state: String,
    verifier: String,
    redirect: String,
}
enum Callback {
    Code(String),
    Denied,
}
impl Pending {
    pub(super) fn bind() -> Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:1455")
            .map_err(|_| "Codex login needs loopback port 1455. Close other pending Codex sign-ins and retry /login.")?;
        Self::new(listener, REDIRECT.into())
    }
    fn new(listener: std::net::TcpListener, redirect: String) -> Result<Self> {
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            state: random()?,
            verifier: random()?,
            redirect,
        })
    }
    #[cfg(test)]
    pub(super) fn local() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect = format!("http://{}/auth/callback", listener.local_addr().unwrap());
        Self::new(listener, redirect).unwrap()
    }
    pub(super) fn url(&self, endpoints: &Endpoints) -> Result<reqwest::Url> {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()));
        let mut url = reqwest::Url::parse(&endpoints.authorize)?;
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", &self.redirect),
            ("scope", "openid profile email offline_access"),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &self.state),
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", "hibiscus"),
        ]);
        Ok(url)
    }
    fn code(&self, input: &str) -> Result<String> {
        match self.callback(input)? {
            Callback::Code(code) => Ok(code),
            Callback::Denied => Err("Codex sign-in was denied. Retry /login when ready.".into()),
        }
    }
    fn callback(&self, input: &str) -> Result<Callback> {
        if input.len() > 16384 || input.chars().any(char::is_control) {
            return Err("Invalid Codex redirect URL".into());
        }
        let url = reqwest::Url::parse(input.trim())
            .map_err(|_| "Paste the full Codex redirect URL, including its state")?;
        let expected = reqwest::Url::parse(&self.redirect)?;
        if url.scheme() != expected.scheme()
            || url.host_str() != expected.host_str()
            || url.port_or_known_default() != expected.port_or_known_default()
            || url.path() != expected.path()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err("Codex redirect does not match this login attempt".into());
        }
        let mut params = HashMap::new();
        for (key, value) in url.query_pairs() {
            if params
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                return Err("Duplicate Codex redirect parameter".into());
            }
        }
        if params.get("state") != Some(&self.state) {
            return Err("Codex login state mismatch. Retry /login.".into());
        }
        if params.contains_key("error") {
            return Ok(Callback::Denied);
        }
        params
            .remove("code")
            .filter(|s| !s.is_empty() && s.len() <= 8192 && !s.chars().any(char::is_control))
            .map(Callback::Code)
            .ok_or_else(|| "Missing Codex authorization code".into())
    }
    pub(super) async fn complete(
        self,
        mut manual: Receiver<String>,
        endpoints: &Endpoints,
    ) -> Result<Token> {
        let listener = tokio::net::TcpListener::from_std(self.listener.try_clone()?)?;
        let code = loop {
            tokio::select! {
                answer = manual.recv() => break self.code(&answer.ok_or("Codex login input closed")?)?,
                accepted = listener.accept() => {
                    let (mut socket, peer) = accepted?;
                    if !peer.ip().is_loopback() { continue; }
                    let target = tokio::time::timeout(Duration::from_secs(2), async {
                        let mut header = vec![];
                        loop {
                            let byte = socket.read_u8().await?;
                            header.push(byte);
                            if header.ends_with(b"\r\n\r\n") || header.len() >= 16384 { break; }
                        }
                        Ok::<_,std::io::Error>(header)
                    }).await;
                    let code = target.ok().and_then(std::result::Result::ok).and_then(|bytes| {
                        let line = std::str::from_utf8(&bytes).ok()?.lines().next()?;
                        let mut parts = line.split_whitespace();
                        if parts.next()? != "GET" { return None; }
                        let target = parts.next()?;
                        if !target.starts_with("/auth/callback?") { return None; }
                        self.callback(&format!("{}{}",self.redirect.trim_end_matches("/auth/callback"),target)).ok()
                    });
                    let (status, text) = if code.is_some() { ("200 OK", "Return to Hibiscus to finish sign-in. You can close this tab.") }
                        else { ("400 Bad Request", "This callback does not match the pending sign-in. Return to Hibiscus.") };
                    let reply = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{text}",text.len());
                    let _ = tokio::time::timeout(Duration::from_secs(1),socket.write_all(reply.as_bytes())).await;
                    match code {
                        Some(Callback::Code(code)) => break code,
                        Some(Callback::Denied) => return Err("Codex sign-in was denied. Retry /login when ready.".into()),
                        None => {},
                    }
                }
            }
        };
        exchange(endpoints, &code, &self.verifier, &self.redirect).await
    }
}

#[derive(Deserialize)]
struct Receipt {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}
async fn tokens(endpoints: &Endpoints, form: &[(&str, &str)]) -> Result<Token> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?;
    let mut response = client
        .post(&endpoints.token)
        .form(form)
        .send()
        .await
        .map_err(|_| "Codex token request failed. Check connectivity and retry /login.")?;
    if !response.status().is_success() {
        return Err(format!("Codex token request returned HTTP {}. Retry /login. Existing credentials were preserved.",response.status().as_u16()).into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Codex token response was interrupted")?
    {
        if bytes.len() + chunk.len() > 128 * 1024 {
            return Err("Codex token response exceeds supported bounds".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|_| "Codex token response is missing valid credentials")?;
    if receipt.expires_in == 0 || receipt.expires_in > 86400 * 30 {
        return Err("Invalid Codex access-token lifetime".into());
    }
    let account_id = account_id(&receipt.access_token)?;
    let token = Token {
        access: receipt.access_token,
        refresh: receipt.refresh_token,
        expires: now().saturating_add(receipt.expires_in),
        account_id,
    };
    token.validate()?;
    Ok(token)
}
async fn exchange(
    endpoints: &Endpoints,
    code: &str,
    verifier: &str,
    redirect: &str,
) -> Result<Token> {
    tokens(
        endpoints,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect),
        ],
    )
    .await
}
pub(super) async fn refresh(endpoints: &Endpoints, token: &Token) -> Result<Token> {
    let refreshed = tokens(
        endpoints,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", &token.refresh),
        ],
    )
    .await?;
    if refreshed.account_id != token.account_id {
        return Err("Codex refresh changed account identity. Use /login to sign in again.".into());
    }
    Ok(refreshed)
}
fn account_id(access: &str) -> Result<String> {
    // Routing metadata from the TLS-protected token endpoint, not a locally
    // verified identity assertion. This does not authenticate arbitrary JWTs.
    let mut parts = access.split('.');
    let _header = parts.next();
    let payload = parts
        .next()
        .ok_or("Codex access token has no account routing metadata")?;
    if parts.next().is_none() || parts.next().is_some() || payload.len() > 65536 {
        return Err("Invalid Codex access-token format".into());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "Invalid Codex access-token encoding")?;
    let claims: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid Codex account routing metadata")?;
    claims["https://api.openai.com/auth"]["chatgpt_account_id"]
        .as_str()
        .filter(|s| {
            !s.is_empty() && s.len() <= 512 && s.is_ascii() && !s.chars().any(char::is_control)
        })
        .map(str::to_owned)
        .ok_or_else(|| "Codex access token is missing its account ID".into())
}

#[cfg(test)]
pub(super) mod tests;
