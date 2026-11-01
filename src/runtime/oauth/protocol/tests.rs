use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

pub(in crate::runtime::oauth) fn access(account: &str) -> String {
    let claims = serde_json::json!({"https://api.openai.com/auth":{"chatgpt_account_id":account}});
    format!(
        "e30.{}.synthetic",
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}
pub(in crate::runtime::oauth) struct Server {
    pub endpoint: Endpoints,
    pub requests: Arc<Mutex<Vec<HashMap<String, String>>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    pub fn new(replies: Vec<(u16, String)>) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = Endpoints::local(&format!("http://{}", listener.local_addr().unwrap()));
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(vec![]));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            let mut replies = std::collections::VecDeque::from(replies);
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !stopped.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let route = line.clone();
                let mut len = 0;
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse::<usize>().unwrap();
                    }
                }
                assert!(len < 65536);
                let mut bytes = vec![0; len];
                if reader.read_exact(&mut bytes).is_err() {
                    continue;
                }
                let mut form: HashMap<String, String> = reqwest::Url::parse(&format!(
                    "http://test/?{}",
                    String::from_utf8(bytes).unwrap()
                ))
                .unwrap()
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
                form.insert("route".into(), route);
                captured.lock().unwrap().push(form);
                let (status, body) = replies
                    .pop_front()
                    .unwrap_or((500, "unexpected request".into()));
                let _=write!(stream,"HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}
pub(in crate::runtime::oauth) fn receipt(account: &str, refresh: &str) -> String {
    serde_json::json!({"access_token":access(account),"refresh_token":refresh,"expires_in":3600})
        .to_string()
}
pub(in crate::runtime::oauth) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn redirected(pending: &Pending, state: &str) -> String {
    format!("{}?state={state}&code=FAKE-CODE", pending.redirect)
}

#[test]
fn authorization_has_public_codex_client_fresh_state_and_s256_pkce() {
    let pending = Pending::local();
    let url = pending.url(&Endpoints::production()).unwrap();
    assert_eq!(url.host_str(), Some("auth.openai.com"));
    let q: HashMap<_, _> = url.query_pairs().collect();
    assert_eq!(q["client_id"], CLIENT_ID);
    assert_eq!(q["originator"], "hibiscus");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(
        q["code_challenge"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.as_bytes()))
    );
    assert!(pending.verifier.len() >= 43);
    let other = Pending::local();
    assert_ne!(pending.state, other.state);
    assert_ne!(pending.verifier, other.verifier);
}
#[test]
fn redirect_requires_exact_target_and_state_and_never_accepts_bare_code() {
    let pending = Pending::local();
    let valid = redirected(&pending, &pending.state);
    assert_eq!(pending.code(&valid).unwrap(), "FAKE-CODE");
    for bad in [
        "FAKE-CODE".into(),
        redirected(&pending, "wrong"),
        format!("{}&state={}", valid, pending.state),
        valid.replace("127.0.0.1", "evil.example"),
        valid.replace("/auth/callback", "/other"),
        format!("{valid}#fragment"),
        format!("{}?code=missing-state", pending.redirect),
        format!(
            "{}?state={}&error=access_denied&error_description=PRIVATE",
            pending.redirect, pending.state
        ),
    ] {
        let error = pending.code(&bad).err().unwrap().to_string();
        assert!(!error.contains("PRIVATE"));
    }
}
#[test]
fn native_loopback_callback_exchanges_code_only_after_matching_state() {
    let server = Server::new(vec![(200, receipt("account-a", "refresh-a"))]);
    let pending = Pending::local();
    let invalid = redirected(&pending, "wrong");
    let valid = redirected(&pending, &pending.state);
    let expected_verifier = pending.verifier.clone();
    let expected_redirect = pending.redirect.clone();
    runtime().block_on(async {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        let completion = pending.complete(rx, &server.endpoint);
        let browser = async {
            let client = reqwest::Client::new();
            assert_eq!(client.get(invalid).send().await.unwrap().status(), 400);
            assert!(server.requests.lock().unwrap().is_empty());
            assert_eq!(client.get(valid).send().await.unwrap().status(), 200);
        };
        let (token, ()) = tokio::join!(completion, browser);
        let token = token.unwrap();
        assert_eq!(token.account_id, "account-a");
        assert_eq!(token.refresh, "refresh-a");
    });
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let q = &requests[0];
    assert_eq!(q["grant_type"], "authorization_code");
    assert_eq!(q["client_id"], CLIENT_ID);
    assert_eq!(q["code"], "FAKE-CODE");
    assert_eq!(q["code_verifier"], expected_verifier);
    assert_eq!(q["redirect_uri"], expected_redirect);
}
#[test]
fn masked_manual_redirect_uses_same_native_exchange() {
    let server = Server::new(vec![(200, receipt("account-a", "refresh-a"))]);
    let pending = Pending::local();
    let redirect = redirected(&pending, &pending.state);
    let token = runtime().block_on(async {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.send(redirect).await.unwrap();
        pending.complete(rx, &server.endpoint).await.unwrap()
    });
    assert_eq!(token.account_id, "account-a");
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}
#[test]
fn denied_browser_callback_finishes_without_a_token_exchange() {
    let server = Server::new(vec![]);
    let pending = Pending::local();
    let redirect = format!(
        "{}?state={}&error=access_denied&error_description=PRIVATE",
        pending.redirect, pending.state
    );
    runtime().block_on(async {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        let completion = pending.complete(rx, &server.endpoint);
        let browser = async {
            reqwest::get(redirect).await.unwrap();
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(completion, browser)
        })
        .await
        .unwrap();
        let error = result.err().unwrap().to_string();
        assert!(error.contains("denied"));
        assert!(!error.contains("PRIVATE"));
    });
    assert!(server.requests.lock().unwrap().is_empty());
}
#[test]
fn token_errors_and_account_routing_failures_do_not_expose_response_contents() {
    for (status,body) in [(401,"PRIVATE-TOKEN-ERROR".into()), (200,"{\"access_token\":\"PRIVATE-TOKEN\"}".into()),
        (200,serde_json::json!({"access_token":"PRIVATE-TOKEN","refresh_token":"SECRET","expires_in":3600}).to_string())] {
        let server=Server::new(vec![(status,body)]);
        let error=runtime().block_on(exchange(&server.endpoint,"code","verifier",REDIRECT)).err().unwrap().to_string();
        assert!(!error.contains("PRIVATE"));
        assert!(!error.contains("SECRET"));
    }
}
