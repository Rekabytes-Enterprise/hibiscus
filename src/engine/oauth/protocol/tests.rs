use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

pub(in crate::engine) struct Server {
    pub base: String,
    pub requests: Arc<Mutex<Vec<HashMap<String, String>>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    pub fn new(replies: Vec<(u16, String)>) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(vec![]));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let own = base.clone();
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
                let mut authorization = None;
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse::<usize>().unwrap();
                    }
                    if let Some(v) = line.to_lowercase().strip_prefix("authorization:") {
                        authorization = Some(v.trim().to_owned());
                    }
                }
                assert!(len < 65536);
                let mut bytes = vec![0; len];
                if reader.read_exact(&mut bytes).is_err() {
                    continue;
                }
                let raw = String::from_utf8(bytes).unwrap();
                let mut form: HashMap<String, String> =
                    reqwest::Url::parse(&format!("http://test/?{raw}"))
                        .unwrap()
                        .query_pairs()
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect();
                form.insert("route".into(), route);
                form.insert("body".into(), raw);
                if let Some(value) = authorization {
                    form.insert("authorization".into(), value);
                }
                captured.lock().unwrap().push(form);
                let (status, body) = replies
                    .pop_front()
                    .unwrap_or((500, "unexpected request".into()));
                // Replies can point back at this server, as discovery documents do.
                let body = body.replace("{base}", &own);
                let _=write!(stream,"HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
            }
        });
        Self {
            base,
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
pub(in crate::engine) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn redirected(pending: &Pending, state: &str) -> String {
    format!("{}?state={state}&code=FAKE-CODE", pending.redirect)
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
