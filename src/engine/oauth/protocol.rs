use crate::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::TryRng;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc::Receiver,
};

pub(in crate::engine) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub(in crate::engine) fn random() -> Result<String> {
    let mut bytes = [0u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| "Cannot obtain secure randomness for sign-in")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
/// A browser sign-in waiting for its loopback callback.
/// The label names the service in errors.
pub(in crate::engine) struct Pending {
    listener: std::net::TcpListener,
    pub(in crate::engine) state: String,
    pub(in crate::engine) verifier: String,
    pub(in crate::engine) redirect: String,
    label: &'static str,
}
enum Callback {
    Code(String, Option<String>),
    Denied,
}
impl Pending {
    pub(in crate::engine) fn new(
        listener: std::net::TcpListener,
        redirect: String,
        label: &'static str,
    ) -> Result<Self> {
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            state: random()?,
            verifier: random()?,
            redirect,
            label,
        })
    }
    #[cfg(test)]
    pub(super) fn local() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect = format!("http://{}/auth/callback", listener.local_addr().unwrap());
        Self::new(listener, redirect, "ChatGPT").unwrap()
    }
    pub(in crate::engine) fn challenge(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()))
    }
    #[cfg(test)]
    fn code(&self, input: &str) -> Result<String> {
        match self.callback(input)? {
            Callback::Code(code, _) => Ok(code),
            Callback::Denied => Err(self.denied()),
        }
    }
    fn denied(&self) -> Box<dyn std::error::Error + Send + Sync> {
        format!(
            "{} sign-in was denied. Retry /login when ready.",
            self.label
        )
        .into()
    }
    fn callback(&self, input: &str) -> Result<Callback> {
        callback(input, &self.redirect, &self.state, self.label)
    }
    /// The authorization code from the browser callback or a pasted redirect URL.
    pub(in crate::engine) async fn wait(&self, manual: Receiver<String>) -> Result<String> {
        self.wait_registration(manual).await.map(|(code, _)| code)
    }
    pub(in crate::engine) async fn wait_registration(
        &self,
        mut manual: Receiver<String>,
    ) -> Result<(String, Option<String>)> {
        let listener = self.listener.try_clone()?;
        // tokio needs a nonblocking socket, and on Windows a duplicated
        // socket doesn't keep the original's nonblocking mode.
        listener.set_nonblocking(true)?;
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let path = reqwest::Url::parse(&self.redirect)?.path().to_owned();
        let origin = self.redirect.trim_end_matches(path.as_str()).to_owned();
        let prefix = format!("{path}?");
        Ok(loop {
            tokio::select! {
                answer = manual.recv() => {
                    match self.callback(&answer.ok_or("Sign-in input closed")?)? {
                        Callback::Code(code, client) => break (code, client),
                        Callback::Denied => return Err(self.denied()),
                    }
                },
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
                        if !target.starts_with(&prefix) { return None; }
                        self.callback(&format!("{origin}{target}")).ok()
                    });
                    let (status, text) = if code.is_some() { ("200 OK", "Return to Hibiscus to finish sign-in. You can close this tab.") }
                        else { ("400 Bad Request", "This callback does not match the pending sign-in. Return to Hibiscus.") };
                    let reply = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{text}",text.len());
                    let _ = tokio::time::timeout(Duration::from_secs(1),socket.write_all(reply.as_bytes())).await;
                    match code {
                        Some(Callback::Code(code, client)) => break (code, client),
                        Some(Callback::Denied) => return Err(self.denied()),
                        None => {},
                    }
                }
            }
        })
    }
}

#[cfg(test)]
pub(in crate::engine) mod tests;

fn callback(input: &str, redirect: &str, state: &str, label: &str) -> Result<Callback> {
    if input.len() > 16384 || input.chars().any(char::is_control) {
        return Err(format!("Invalid {label} redirect URL").into());
    }
    let url = reqwest::Url::parse(input.trim())
        .map_err(|_| format!("Paste the full {label} redirect URL, including its state"))?;
    let expected = reqwest::Url::parse(redirect)?;
    if url.scheme() != expected.scheme()
        || url.host_str() != expected.host_str()
        || url.port_or_known_default() != expected.port_or_known_default()
        || url.path() != expected.path()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(format!("{label} redirect does not match this login attempt").into());
    }
    let mut params = HashMap::new();
    for (key, value) in url.query_pairs() {
        if params
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(format!("Duplicate {label} redirect parameter").into());
        }
    }
    if params.get("state").map(String::as_str) != Some(state) {
        return Err(format!("{label} login state mismatch. Retry /login.").into());
    }
    if params.contains_key("error") {
        return Ok(Callback::Denied);
    }
    params
        .remove("code")
        .filter(|s| !s.is_empty() && s.len() <= 8192 && !s.chars().any(char::is_control))
        .map(|code| Callback::Code(code, params.remove("client_id")))
        .ok_or_else(|| format!("Missing {label} authorization code").into())
}

#[cfg(feature = "fuzzing")]
pub(super) fn fuzz_callback(bytes: &[u8]) {
    const REDIRECT: &str = "http://localhost:1455/auth/callback";
    if let Ok(input) = std::str::from_utf8(bytes) {
        let _ = callback(input, REDIRECT, "fuzz-state", "ChatGPT");
        if matches!(
            callback(input, REDIRECT, "fuzz-state", "ChatGPT"),
            Ok(Callback::Code(..))
        ) {
            assert!(callback(input, REDIRECT, "different-state", "ChatGPT").is_err());
        }
    }
}
