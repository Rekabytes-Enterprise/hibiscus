//! OpenAI's dynamic registration and ChatGPT plan grant for local apps.
mod identity;

use super::oauth::{
    protocol::{now, random, Pending},
    store::Store,
};
use crate::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc::Receiver;

pub(crate) const ID: &str = "openai-chatgpt";
pub(super) const RESOURCE: &str = "https://api.openai.com/v1";
const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
const ENTRYPOINT: &str = "dynamic_agent_client";
const DIRECT: &str = "chatgpt.tokens.use.direct";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const SIGN_IN: &str = "Sign in with ChatGPT in Settings → Providers or /login.";
const CONSENT: &str =
    "ChatGPT plan usage is disabled for Hibiscus. Sign in again and approve plan usage.";

#[derive(Clone, Deserialize, Serialize)]
struct Registration {
    client_id: String,
    subject: String,
    grant: Option<Grant>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Grant {
    access: String,
    scope: String,
    refresh: String,
    expires: u64,
    refresh_after: u64,
}

#[derive(Deserialize)]
struct Receipt {
    token_type: String,
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: u64,
    scope: Option<String>,
    #[serde(default)]
    earliest_refresh_at: u64,
}

impl Receipt {
    fn grant(&self, previous_scope: Option<&str>) -> Result<Option<Grant>> {
        let scope = self.scope.as_deref().or(previous_scope).unwrap_or_default();
        if !scope.split_whitespace().any(|s| s == DIRECT) {
            return Ok(None);
        }
        let refresh = self
            .refresh_token
            .as_deref()
            .filter(|s| valid(s, 32768))
            .ok_or("ChatGPT did not return a valid refresh token.")?;
        if !self.token_type.eq_ignore_ascii_case("bearer")
            || !valid(&self.access_token, 32768)
            || !(1..=86400).contains(&self.expires_in)
        {
            return Err("ChatGPT returned invalid access-token metadata.".into());
        }
        Ok(Some(Grant {
            access: self.access_token.clone(),
            scope: scope.to_owned(),
            refresh: refresh.into(),
            expires: now().saturating_add(self.expires_in),
            refresh_after: self.earliest_refresh_at,
        }))
    }
}

fn valid(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && value.bytes().all(|b| b.is_ascii_graphic())
}

fn client_id(value: &str) -> bool {
    value != ENTRYPOINT
        && !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}

fn store() -> Result<Store> {
    Store::named("chatgpt-plan-auth.json")
}

fn host_id(store: &Store) -> Result<String> {
    let _lock = store.try_lock()?;
    if let Some(id) = store.read_json::<String>()? {
        if valid_host_id(&id) {
            return Ok(id);
        }
        // v0.4.11 generated a raw base64url string from 32 random bytes.
        // OpenAI rejected that format before it could issue a registration.
        if id.len() != 43
            || !URL_SAFE_NO_PAD
                .decode(&id)
                .is_ok_and(|bytes| bytes.len() == 32)
        {
            return Err("Invalid saved ChatGPT host identifier.".into());
        }
    }
    let id = format!("urn:uuid:{}", super::uuid_v4()?);
    store.write_json(&id)?;
    Ok(id)
}

fn valid_host_id(id: &str) -> bool {
    id.strip_prefix("urn:uuid:").is_some_and(|uuid| {
        let bytes = uuid.as_bytes();
        bytes.len() == 36
            && bytes[14] == b'4'
            && matches!(bytes[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
            && bytes.iter().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    *byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
    })
}

fn registered_client(previous: Option<&str>, returned: Option<&str>) -> Result<String> {
    match (previous, returned) {
        (Some(expected), Some(actual)) if expected != actual => {
            Err("ChatGPT returned a different app registration.".into())
        }
        (Some(expected), _) => Ok(expected.into()),
        (None, Some(actual)) if client_id(actual) => Ok(actual.into()),
        _ => Err("ChatGPT did not issue a Hibiscus app registration.".into()),
    }
}

pub(crate) struct CompletedSignIn {
    registration: Registration,
    previous_client: Option<String>,
}

pub(crate) struct BrowserSignIn {
    pending: Pending,
    previous: Option<Registration>,
    nonce: String,
    url: reqwest::Url,
}

impl BrowserSignIn {
    pub(crate) fn start() -> Result<Self> {
        let host = host_id(&Store::named("chatgpt-plan-host.json")?)?;
        let previous = store()?.read_json::<Registration>()?;
        Self::prepare(&host, previous)
    }

    fn prepare(host: &str, previous: Option<Registration>) -> Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let redirect = format!("http://{}/auth/callback", listener.local_addr()?);
        let pending = Pending::new(listener, redirect, "ChatGPT")?;
        let nonce = random()?;
        let mut url = reqwest::Url::parse(AUTHORIZE)?;
        let client = previous
            .as_ref()
            .map_or(ENTRYPOINT, |r| r.client_id.as_str());
        if client != ENTRYPOINT && !client_id(client) {
            return Err("Invalid saved ChatGPT registration. Sign out and try again.".into());
        }
        url.query_pairs_mut().extend_pairs([
            ("client_id", client),
            ("ext_agent_host_id", host),
            ("response_type", "code"),
            ("redirect_uri", &pending.redirect),
            ("scope", SCOPES),
            ("resource", RESOURCE),
            ("state", &pending.state),
            ("nonce", &nonce),
            ("code_challenge_method", "S256"),
            ("code_challenge", &pending.challenge()),
        ]);
        if previous.is_none() {
            url.query_pairs_mut()
                .append_pair("agent_name_hint", "Hibiscus");
        } else {
            url.query_pairs_mut().append_pair("prompt", "consent");
        }
        Ok(Self {
            pending,
            previous,
            nonce,
            url,
        })
    }

    pub(crate) fn url(&self) -> &str {
        self.url.as_str()
    }

    pub(crate) async fn complete(self, manual: Receiver<String>) -> Result<CompletedSignIn> {
        self.complete_at(
            manual,
            TOKEN,
            &format!("{ISSUER}/.well-known/openid-configuration"),
        )
        .await
    }

    async fn complete_at(
        self,
        manual: Receiver<String>,
        endpoint: &str,
        discovery: &str,
    ) -> Result<CompletedSignIn> {
        let (code, client) = self.pending.wait_registration(manual).await?;
        let client_id = registered_client(
            self.previous.as_ref().map(|r| r.client_id.as_str()),
            client.as_deref(),
        )?;
        let http = http()?;
        let receipt = token(
            &http,
            endpoint,
            &[
                ("grant_type", "authorization_code"),
                ("client_id", &client_id),
                ("code", &code),
                ("code_verifier", &self.pending.verifier),
                ("redirect_uri", &self.pending.redirect),
                ("resource", RESOURCE),
            ],
        )
        .await?;
        let id = receipt
            .id_token
            .as_deref()
            .ok_or("ChatGPT did not return an identity token.")?;
        let subject = identity::verify(&http, discovery, id, &client_id, Some(&self.nonce)).await?;
        if self.previous.as_ref().is_some_and(|r| r.subject != subject) {
            return Err(
                "ChatGPT returned a different account. Sign out before connecting another account."
                    .into(),
            );
        }
        Ok(CompletedSignIn {
            registration: Registration {
                client_id,
                subject,
                grant: receipt.grant(None)?,
            },
            previous_client: self.previous.map(|registration| registration.client_id),
        })
    }
}

pub(crate) fn save(completed: &CompletedSignIn) -> Result<()> {
    let store = store()?;
    let _lock = store.try_lock()?;
    let current = store.read_json::<Registration>()?;
    if current.as_ref().map(|registration| &registration.client_id)
        != completed.previous_client.as_ref()
    {
        return Err(
            "The ChatGPT login changed while the browser was open. Start sign-in again.".into(),
        );
    }
    store.write_json(&completed.registration)?;
    if completed.registration.grant.is_none() {
        return Err(CONSENT.into());
    }
    Ok(())
}

pub(crate) fn has_registration() -> bool {
    store()
        .and_then(|store| store.read_json::<Registration>())
        .ok()
        .flatten()
        .is_some()
}

pub(super) fn access() -> Result<()> {
    let account = store()?.read_json::<Registration>()?.ok_or(SIGN_IN)?;
    account.grant.ok_or_else(|| CONSENT.into()).map(|_| ())
}

pub(super) async fn credential() -> Result<String> {
    credential_at(
        &store()?,
        TOKEN,
        &format!("{ISSUER}/.well-known/openid-configuration"),
    )
    .await
}

async fn credential_at(store: &Store, endpoint: &str, discovery: &str) -> Result<String> {
    let _lock = store.lock().await?;
    let mut account = store.read_json::<Registration>()?.ok_or(SIGN_IN)?;
    let grant = account.grant.as_ref().ok_or(CONSENT)?;
    if !client_id(&account.client_id)
        || !valid(&grant.access, 32768)
        || !valid(&grant.refresh, 32768)
    {
        return Err("Invalid saved ChatGPT plan credential. Sign in again.".into());
    }
    let now = now();
    if grant.expires > now && (grant.expires.saturating_sub(now) > 60 || grant.refresh_after > now)
    {
        return Ok(grant.access.clone());
    }
    if grant.refresh_after > now {
        return Err("ChatGPT token renewal is temporarily unavailable. Try again later.".into());
    }
    let http = http()?;
    let receipt = token(
        &http,
        endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &account.client_id),
            ("refresh_token", &grant.refresh),
            ("resource", RESOURCE),
        ],
    )
    .await?;
    if let Some(id) = &receipt.id_token {
        if identity::verify(&http, discovery, id, &account.client_id, None).await?
            != account.subject
        {
            return Err("Refreshed ChatGPT token did not match the saved account.".into());
        }
    }
    account.grant = receipt.grant(Some(&grant.scope))?;
    store.write_json(&account)?;
    Ok(account.grant.ok_or(CONSENT)?.access)
}

pub(crate) fn remove() -> Result<()> {
    let store = store()?;
    let _lock = store.try_lock()?;
    match std::fs::remove_file(store.path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?)
}

async fn token(http: &reqwest::Client, endpoint: &str, form: &[(&str, &str)]) -> Result<Receipt> {
    let response = http
        .post(endpoint)
        .form(form)
        .send()
        .await
        .map_err(|_| "Could not reach ChatGPT to finish signing in or refresh the token.")?;
    if !response.status().is_success() {
        return Err(format!(
            "ChatGPT token request returned HTTP {}. Try signing in again.",
            response.status().as_u16()
        )
        .into());
    }
    json(response).await
}

async fn json<T: serde::de::DeserializeOwned>(mut response: reqwest::Response) -> Result<T> {
    if !response.status().is_success() {
        return Err("ChatGPT identity service returned an error.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "ChatGPT response was interrupted.")?
    {
        if bytes.len() + chunk.len() > 128 * 1024 {
            return Err("ChatGPT authentication response exceeds the supported size.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid ChatGPT authentication response.".into())
}

#[cfg(test)]
mod tests;

pub(super) fn validate_provider(provider: &super::config::Provider) -> Result<()> {
    use super::config::{Api, Authentication};
    if (provider.id == ID || provider.authentication == Authentication::ChatGptPlan)
        && (provider.id != ID
            || provider.authentication != Authentication::ChatGptPlan
            || provider.base_url != RESOURCE
            || provider.api != Api::Responses
            || provider.api_key_env.is_some()
            || provider.discover_models)
    {
        return Err("openai-chatgpt requires ChatGPT plan authentication and the fixed OpenAI Responses endpoint.".into());
    }
    Ok(())
}

pub(super) async fn response_error(
    response: reqwest::Response,
) -> Box<dyn std::error::Error + Send + Sync> {
    let status = response.status();
    let mut response = response;
    let mut bytes = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        if bytes.len() + chunk.len() > 32 * 1024 {
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_default();
    request_error(status, body["error"]["code"].as_str()).into()
}

pub(super) fn request_error(status: reqwest::StatusCode, code: Option<&str>) -> String {
    match code {
        Some("subscription_sharing_usage_limit_exceeded") =>
            "Hibiscus reached a ChatGPT plan usage limit. Review your plan and Hibiscus’s limit in ChatGPT Settings → Usage.".into(),
        Some("subscription_sharing_usage_unavailable") =>
            "ChatGPT could not check plan usage right now. Try again later.".into(),
        Some("subscription_sharing_user_not_eligible") =>
            "ChatGPT plan usage is unavailable for this account or workspace.".into(),
        _ => match status.as_u16() {
            401 => "ChatGPT did not accept the saved login. Sign in again to continue.".into(),
            429 => "ChatGPT temporarily limited this request. Check ChatGPT Settings → Usage, then retry manually.".into(),
            _ => format!("ChatGPT plan request returned HTTP {}. Check model access and ChatGPT Settings → Usage.", status.as_u16()),
        },
    }
}
