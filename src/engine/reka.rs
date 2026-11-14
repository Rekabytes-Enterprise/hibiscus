//! Reka Bytes sign-in for Rooxcode (#185). The browser part is the Codex flow
//! (oauth/protocol.rs): PKCE, `state`, a loopback callback or a pasted URL.
//! The gateway turns the code into its own device session, kept in
//! `rooxcode-auth.json` (0600) and shared by the desktop app and the terminal.
//! The tokens only ever go to the Rooxcode gateway.
use super::oauth::{
    protocol::{Pending, now, random},
    store::Store,
};
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

/// The error for a Rooxcode prompt without an account. The desktop app and
/// the terminal look for it to offer sign-in.
pub const SIGN_IN: &str =
    "Sign in with Reka Bytes to use Rooxcode models. Use /login → Browser Login → Reka Bytes.";
/// Loopback ports registered for the `hibiscus` client. The issuer matches
/// redirect URIs exactly, so a free one of these is used.
const PORTS: [u16; 3] = [14565, 14566, 14567];
const FILE: &str = "rooxcode-auth.json";

/// The gateway's origin: the Rooxcode API without `/v1`.
fn gateway() -> &'static str {
    super::rooxcode::URL.trim_end_matches("/v1")
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Account {
    access: String,
    refresh: String,
    expires: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    /// The pixel character picked on the Rooxcode account page; None is the default.
    #[serde(default)]
    pub avatar: Option<String>,
}

#[derive(Deserialize)]
struct Session {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}
impl Session {
    fn account(self, previous: Option<&Account>) -> Result<Account> {
        let token = |t: &str| {
            t.len() == 43
                && t.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        };
        if !token(&self.access_token)
            || !token(&self.refresh_token)
            || !(1..=86400).contains(&self.expires_in)
        {
            return Err("Rooxcode returned an invalid session. Sign in again with /login.".into());
        }
        Ok(Account {
            access: self.access_token,
            refresh: self.refresh_token,
            expires: now().saturating_add(self.expires_in),
            name: previous.and_then(|a| a.name.clone()),
            email: previous.and_then(|a| a.email.clone()),
            avatar: previous.and_then(|a| a.avatar.clone()),
        })
    }
}

fn store() -> Result<Store> {
    Store::named(FILE)
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?)
}

async fn json_body(mut response: reqwest::Response) -> Result<Value> {
    let bytes = super::rooxcode::bounded_body(&mut response, 64 * 1024).await?;
    Ok(serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// The signed-in account, if any. A broken file counts as signed out.
pub(crate) fn account() -> Option<Account> {
    store().ok()?.read_json().ok().flatten()
}

/// Refuses a Rooxcode prompt before it is sent when nobody is signed in.
pub(super) fn signed_in() -> Result<()> {
    account().map(|_| ()).ok_or_else(|| SIGN_IN.into())
}

/// A current access token for a Rooxcode request, refreshed when it expires
/// within a minute, or when the gateway just rejected `rejected` as expired.
pub(super) async fn access(rejected: Option<&str>) -> Result<String> {
    access_at(&store()?, gateway(), rejected).await
}

async fn access_at(store: &Store, gateway: &str, rejected: Option<&str>) -> Result<String> {
    // Refresh tokens rotate and a reused one revokes the device, so the lock
    // covers the desktop app and the terminal refreshing at once.
    let _lock = store.lock().await?;
    let account: Account = store.read_json()?.ok_or(SIGN_IN)?;
    let stale = rejected == Some(account.access.as_str());
    if !stale && account.expires > now().saturating_add(60) {
        return Ok(account.access);
    }
    let response = client()?
        .post(format!("{gateway}/api/hibiscus/auth/refresh"))
        .json(&json!({"refresh_token": account.refresh}))
        .send()
        .await
        .map_err(|_| "Rooxcode session refresh failed. Check connectivity; nothing was sent.")?;
    match response.status().as_u16() {
        200 => {}
        401 => {
            remove(store)?;
            return Err(SIGN_IN.into());
        }
        status => {
            return Err(format!(
                "Rooxcode session refresh returned HTTP {status}. Nothing was sent; try again."
            )
            .into());
        }
    }
    let session: Session = serde_json::from_value(json_body(response).await?)
        .map_err(|_| "Rooxcode returned an invalid session. Sign in again with /login.")?;
    let refreshed = session.account(Some(&account))?;
    store.write_json(&refreshed)?;
    Ok(refreshed.access)
}

/// Forgets a session the gateway no longer knows (revoked or expired).
pub(super) fn forget() {
    if let Ok(store) = store() {
        let _ = remove(&store);
    }
}

fn remove(store: &Store) -> Result<()> {
    match std::fs::remove_file(store.path()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Signs out: the local session goes at once, then the gateway revokes the
/// device if it can be reached within a few seconds.
pub(crate) fn sign_out() -> Result<()> {
    let store = store()?;
    let account: Option<Account> = {
        let _lock = store.try_lock()?;
        let account = store.read_json().ok().flatten();
        remove(&store)?;
        account
    };
    if let Some(account) = account {
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(revoke(gateway(), &account.refresh))
        })
        .join();
    }
    Ok(())
}

async fn revoke(gateway: &str, refresh: &str) -> Result<()> {
    client()?
        .post(format!("{gateway}/api/hibiscus/auth/sign-out"))
        .timeout(Duration::from_secs(5))
        .json(&json!({"refresh_token": refresh}))
        .send()
        .await?;
    Ok(())
}

/// A name for this device on the website's device list.
fn device(kind: &str) -> String {
    let mut buffer = [0u8; 256];
    let host = if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } == 0 {
        let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
        String::from_utf8_lossy(&buffer[..end]).into_owned()
    } else {
        String::new()
    };
    let host: String = host.chars().filter(|c| !c.is_control()).take(60).collect();
    if host.trim().is_empty() {
        format!("Hibiscus {kind}")
    } else {
        format!("Hibiscus {kind} on {}", host.trim())
    }
}

/// One Reka Bytes browser sign-in, without any UI.
pub(crate) struct BrowserSignIn {
    pending: Pending,
    nonce: String,
    url: String,
    gateway: String,
    device: String,
}
impl BrowserSignIn {
    /// Asks the gateway for the issuer and client, listens on a registered
    /// loopback port and builds the authorization URL. `kind` is "desktop" or
    /// "terminal", for the device name.
    pub(crate) async fn start(kind: &str) -> Result<Self> {
        store()?;
        Self::start_at(gateway(), kind, bind).await
    }
    async fn start_at(
        gateway: &str,
        kind: &str,
        bind: impl FnOnce() -> Result<Pending>,
    ) -> Result<Self> {
        let unreachable =
            "Can't reach Rooxcode to start sign-in. Check connectivity and try again.";
        let config = json_body(
            client()?
                .get(format!("{gateway}/api/hibiscus/auth"))
                .send()
                .await
                .map_err(|_| unreachable)?,
        )
        .await?;
        let (Some(issuer), Some(client_id), true) = (
            config["issuer"].as_str(),
            config["client_id"].as_str(),
            config["enabled"] == true,
        ) else {
            return Err("Reka Bytes sign-in isn't available on Rooxcode right now.".into());
        };
        let discovery = json_body(
            client()?
                .get(format!(
                    "{}/.well-known/openid-configuration",
                    issuer.trim_end_matches('/')
                ))
                .send()
                .await
                .map_err(|_| "Can't reach Reka Bytes sign-in. Check connectivity and try again.")?,
        )
        .await?;
        let mut url = discovery["authorization_endpoint"]
            .as_str()
            .and_then(|u| reqwest::Url::parse(u).ok())
            .filter(|u| u.scheme() == "https" || cfg!(test))
            .ok_or("Reka Bytes sign-in returned an invalid configuration.")?;
        let pending = bind()?;
        let nonce = random()?;
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", &pending.redirect),
            ("scope", "openid profile email"),
            ("state", &pending.state),
            ("nonce", &nonce),
            ("code_challenge", &pending.challenge()),
            ("code_challenge_method", "S256"),
        ]);
        Ok(Self {
            pending,
            nonce,
            url: url.to_string(),
            gateway: gateway.to_owned(),
            device: device(kind),
        })
    }
    pub(crate) fn url(&self) -> &str {
        &self.url
    }
    /// Waits for the browser callback or a pasted redirect URL, then asks the
    /// gateway for a device session. Nothing is saved here.
    pub(crate) async fn complete(
        self,
        pasted: tokio::sync::mpsc::Receiver<String>,
    ) -> Result<Account> {
        let code = self.pending.wait(pasted).await?;
        let response = client()?
            .post(format!("{}/api/hibiscus/auth/session", self.gateway))
            .json(&json!({
                "code": code,
                "code_verifier": self.pending.verifier,
                "nonce": self.nonce,
                "redirect_uri": self.pending.redirect,
                "device_name": self.device,
            }))
            .send()
            .await
            .map_err(|_| "Can't reach Rooxcode to finish sign-in. Try again.")?;
        if !response.status().is_success() {
            return Err("Reka Bytes sign-in could not be completed. Start sign-in again.".into());
        }
        let session: Session = serde_json::from_value(json_body(response).await?)
            .map_err(|_| "Rooxcode returned an invalid session. Start sign-in again.")?;
        let mut account = session.account(None)?;
        // "Signed in as" is a nicety: sign-in still works without it.
        if let Some(me) = me(&self.gateway, &account.access).await {
            me.apply(&mut account);
        }
        Ok(account)
    }
}

/// The gateway's `/me`: the profile, plus usage for the Account page.
struct Me {
    name: Option<String>,
    email: Option<String>,
    avatar: Option<String>,
    usage: Usage,
}

/// One budget window: how much is used, never the amounts behind it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Window {
    /// 0 to 100.
    pub percent: u8,
    /// Unix seconds when the oldest counted usage leaves the window.
    pub drops_at: Option<i64>,
    /// Seconds until a request fits again, when the window is full.
    pub retry_after: Option<u64>,
}

/// Account usage from `/me`. A window whose limit is off is None.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Usage {
    pub five_hours: Option<Window>,
    pub week: Option<Window>,
    pub credits: Option<f64>,
    pub device: Option<String>,
}

fn window(v: &Value) -> Option<Window> {
    let limit = v["limit"].as_i64().filter(|l| *l > 0)?;
    let used = v["used"].as_i64().unwrap_or(0).max(0);
    Some(Window {
        percent: (used.saturating_mul(100) / limit).min(100) as u8,
        drops_at: v["drops_at"].as_i64(),
        retry_after: v["retry_after"].as_u64(),
    })
}
impl Me {
    fn apply(self, account: &mut Account) {
        account.name = self.name;
        account.email = self.email;
        account.avatar = self.avatar;
    }
}

/// Pixel characters the Rooxcode account page offers. Anything else is the default.
const AVATARS: [&str; 6] = ["slime", "cat", "bunny", "frog", "ghost", "bloom"];

async fn me(gateway: &str, access: &str) -> Option<Me> {
    let response = client()
        .ok()?
        .get(format!("{gateway}/api/hibiscus/auth/me"))
        .bearer_auth(access)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let me = json_body(response).await.ok()?;
    let text = |v: &Value| {
        v.as_str()
            .filter(|s| s.len() <= 320 && !s.chars().any(char::is_control))
            .map(str::to_owned)
    };
    Some(Me {
        name: text(&me["name"]),
        email: text(&me["email"]),
        avatar: me["avatar"]
            .as_str()
            .filter(|a| AVATARS.contains(a))
            .map(str::to_owned),
        usage: Usage {
            five_hours: window(&me["allowance"]["five_hours"]),
            week: window(&me["allowance"]["week"]),
            credits: me["wallet"]["credits"].as_f64().filter(|c| c.is_finite()),
            device: text(&me["device_name"]),
        },
    })
}

/// The signed-in account and its usage, fresh from the gateway, for the Account page.
/// The saved profile is updated on the way.
pub(crate) async fn usage() -> Result<(Account, Usage)> {
    let store = store()?;
    let access = access_at(&store, gateway(), None).await?;
    let me = me(gateway(), &access).await.ok_or(
        "Can't load your Rooxcode account right now. Check your connection and try again.",
    )?;
    let usage = me.usage.clone();
    let _lock = store.lock().await?;
    let mut account: Account = store.read_json()?.ok_or(SIGN_IN)?;
    me.apply(&mut account);
    store.write_json(&account)?;
    Ok((account, usage))
}

/// Reloads the name and avatar from the gateway, so changes made on the
/// account page show up. Keeps the saved profile when the gateway can't be reached.
pub(crate) async fn refresh_profile() -> Option<Account> {
    let store = store().ok()?;
    let access = access_at(&store, gateway(), None).await.ok()?;
    if let Some(me) = me(gateway(), &access).await {
        let _lock = store.lock().await.ok()?;
        // Re-read under the lock: a refresh may have rotated the tokens meanwhile.
        let mut account: Account = store.read_json().ok().flatten()?;
        me.apply(&mut account);
        let _ = store.write_json(&account);
    }
    account()
}

fn bind() -> Result<Pending> {
    for port in PORTS {
        if let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) {
            return Pending::new(
                listener,
                format!("http://127.0.0.1:{port}/oauth/callback"),
                "Reka Bytes",
            );
        }
    }
    Err("Reka Bytes sign-in needs a free port (14565 to 14567). Close other sign-ins and try again.".into())
}

/// Stores a completed sign-in. Cancelled or failed attempts never get here.
pub(crate) fn save(account: &Account) -> Result<()> {
    let store = store()?;
    let _lock = store.try_lock()?;
    store.write_json(account)
}

/// Reka Bytes sign-in for the desktop app, shaped like `CodexSignIn`: a paste
/// channel, a 10-minute limit and saving.
pub struct RekaSignIn {
    sign_in: BrowserSignIn,
    paste: tokio::sync::mpsc::Sender<String>,
    pasted: tokio::sync::mpsc::Receiver<String>,
}
impl RekaSignIn {
    /// Starts sign-in. Open `url` in the browser, then await `finish`.
    pub async fn start() -> Result<Self> {
        let sign_in = BrowserSignIn::start("desktop").await?;
        let (paste, pasted) = tokio::sync::mpsc::channel(1);
        Ok(Self {
            sign_in,
            paste,
            pasted,
        })
    }
    pub fn url(&self) -> &str {
        self.sign_in.url()
    }
    /// Sends a redirect URL pasted by the user, for when the browser can't reach the app.
    pub fn paste(&self) -> tokio::sync::mpsc::Sender<String> {
        self.paste.clone()
    }
    /// Waits for the browser (or a pasted URL), then saves the session.
    /// Dropping the future cancels and frees the port.
    pub async fn finish(self) -> Result<()> {
        let Self {
            sign_in,
            paste,
            pasted,
        } = self;
        let _paste = paste;
        let account = tokio::time::timeout(Duration::from_secs(600), sign_in.complete(pasted))
            .await
            .map_err(|_| "Reka Bytes sign-in timed out. Try again.")??;
        save(&account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::oauth::protocol::tests::{Server, runtime};

    #[test]
    fn usage_windows_show_percent_only() {
        let full = window(
            &json!({"used":9_000_000,"limit":8_000_000,"retry_after":120,"drops_at":1_800_000_000}),
        );
        assert_eq!(
            full,
            Some(Window {
                percent: 100,
                drops_at: Some(1_800_000_000),
                retry_after: Some(120)
            })
        );
        let some =
            window(&json!({"used":2_000_000,"limit":8_000_000,"retry_after":null,"drops_at":null}));
        assert_eq!(some.map(|w| w.percent), Some(25));
        assert_eq!(
            window(&json!({"used":5,"limit":0})),
            None,
            "a limit that is off"
        );
        assert_eq!(window(&Value::Null), None);
    }

    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("hibiscus-reka-{}", super::super::config::unique()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn store(&self) -> Store {
            Store::at(self.0.join(FILE))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn token(c: char) -> String {
        std::iter::repeat_n(c, 43).collect()
    }
    fn session(c: char) -> (u16, String) {
        (200, json!({"access_token":token(c),"refresh_token":token(c.to_ascii_uppercase()),"token_type":"Bearer","expires_in":1800}).to_string())
    }
    fn account(c: char, expires: u64) -> Account {
        Account {
            access: token(c),
            refresh: token(c.to_ascii_uppercase()),
            expires,
            name: None,
            email: Some("me@example.com".into()),
            avatar: None,
        }
    }

    #[test]
    fn refresh_rotates_once_keeps_the_account_and_a_rejected_session_signs_out() {
        let temp = Temp::new();
        let store = temp.store();
        let server = Server::new(vec![
            session('b'),
            (
                401,
                r#"{"error":{"code":"rooxcode_sign_in_required"}}"#.into(),
            ),
        ]);
        runtime().block_on(async {
            assert_eq!(
                access_at(&store, &server.base, None)
                    .await
                    .unwrap_err()
                    .to_string(),
                SIGN_IN
            );
            store.write_json(&account('a', now() + 3600)).unwrap();
            // Fresh: no request.
            assert_eq!(
                access_at(&store, &server.base, None).await.unwrap(),
                token('a')
            );
            // Rejected as expired: refreshed once, even by two callers at once.
            let rejected = token('a');
            let (one, two) = tokio::join!(
                access_at(&store, &server.base, Some(&rejected)),
                access_at(&store, &server.base, Some(&rejected))
            );
            assert_eq!((one.unwrap(), two.unwrap()), (token('b'), token('b')));
            let saved: Account = store.read_json().unwrap().unwrap();
            assert_eq!(saved.email.as_deref(), Some("me@example.com"));
            // A refresh the gateway refuses means the device was revoked.
            store.write_json(&account('c', 1)).unwrap();
            assert_eq!(
                access_at(&store, &server.base, None)
                    .await
                    .unwrap_err()
                    .to_string(),
                SIGN_IN
            );
        });
        assert!(!store.path().exists());
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0]["route"].starts_with("POST /api/hibiscus/auth/refresh"));
        assert!(requests[0]["body"].contains(&token('A')));
        assert!(requests.iter().all(|r| !r.contains_key("authorization")));
    }

    #[test]
    fn sign_in_checks_state_and_sends_the_code_with_its_verifier() {
        let config = (
            200,
            json!({"enabled":true,"required":true,"issuer":"{base}/issuer","client_id":"hibiscus"})
                .to_string(),
        );
        let discovery = (
            200,
            json!({"authorization_endpoint":"{base}/authorize"}).to_string(),
        );
        let server = Server::new(vec![
            config.clone(),
            discovery.clone(),
            config,
            discovery,
            session('a'),
            (
                200,
                json!({"name":"Me","email":"me@example.com","avatar":"frog"}).to_string(),
            ),
        ]);
        let local = || {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let redirect = format!("http://{}/oauth/callback", listener.local_addr().unwrap());
            Pending::new(listener, redirect, "Reka Bytes")
        };
        let query = |sign_in: &BrowserSignIn, key: &str| {
            reqwest::Url::parse(sign_in.url())
                .unwrap()
                .query_pairs()
                .find(|(k, _)| k == key)
                .unwrap()
                .1
                .into_owned()
        };
        let account = runtime().block_on(async {
            let wrong = BrowserSignIn::start_at(&server.base, "terminal", local)
                .await
                .unwrap();
            assert!(
                wrong
                    .url()
                    .starts_with(&format!("{}/authorize?", server.base))
            );
            assert_eq!(query(&wrong, "client_id"), "hibiscus");
            assert_eq!(query(&wrong, "code_challenge_method"), "S256");
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.send(format!(
                "{}?state=other&code=FAKE-CODE",
                query(&wrong, "redirect_uri")
            ))
            .await
            .unwrap();
            assert!(wrong.complete(rx).await.is_err());

            let right = BrowserSignIn::start_at(&server.base, "terminal", local)
                .await
                .unwrap();
            let verifier = right.pending.verifier.clone();
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.send(format!(
                "{}?state={}&code=FAKE-CODE",
                query(&right, "redirect_uri"),
                query(&right, "state")
            ))
            .await
            .unwrap();
            let account = right.complete(rx).await.unwrap();
            (account, verifier)
        });
        let (account, verifier) = account;
        assert_eq!(account.access, token('a'));
        assert_eq!(account.email.as_deref(), Some("me@example.com"));
        assert_eq!(account.avatar.as_deref(), Some("frog"));
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 6, "a wrong state sends nothing");
        let exchange = &requests[4];
        assert!(exchange["route"].starts_with("POST /api/hibiscus/auth/session"));
        for part in ["FAKE-CODE", verifier.as_str(), "Hibiscus terminal"] {
            assert!(exchange["body"].contains(part));
        }
        assert_eq!(
            requests[5]["authorization"],
            format!("bearer {}", token('a')).to_lowercase()
        );
    }

    #[test]
    fn credentials_go_only_to_the_rooxcode_origin() {
        let mut provider: super::super::config::Provider = serde_json::from_value(json!({"id":"rooxcode","authentication":"rooxcode","baseUrl":super::super::rooxcode::URL,"api":"chat_completions","models":[]})).unwrap();
        let id = super::super::rooxcode::SessionId::new().unwrap();
        let body = json!({"model":"m"});
        let request =
            super::super::rooxcode::chat_request(&provider, &body, &id, Some(&token('a')))
                .unwrap()
                .build()
                .unwrap();
        assert_eq!(
            request.headers()["authorization"],
            format!("Bearer {}", token('a'))
        );
        provider.base_url = "https://example.com/v1".into();
        let request =
            super::super::rooxcode::chat_request(&provider, &body, &id, Some(&token('a')))
                .unwrap()
                .build()
                .unwrap();
        assert!(!request.headers().contains_key("authorization"));
    }
}
