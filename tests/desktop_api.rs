//! The library API used by the desktop app. One test per binary: it sets
//! process-wide environment variables, which the in-process engine reads.
#[allow(dead_code)] // Only the temp-dir helper is used here.
mod support;
use hibiscus::desktop::{self, Engine, Start};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

fn sse(v: Value) -> String {
    format!("data: {v}\n\n")
}

/// Answers every model request with a reply; a prompt containing SLOW gets a
/// stream that never finishes, so it can be aborted.
fn serve(listener: TcpListener, stop: Arc<AtomicBool>) {
    listener.set_nonblocking(true).unwrap();
    while !stop.load(Ordering::Relaxed) {
        let Ok((socket, _)) = listener.accept() else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        socket.set_nonblocking(false).unwrap();
        let stop = stop.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut socket = socket;
            let mut route = String::new();
            reader.read_line(&mut route).unwrap();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
                    length = n.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            if route.starts_with("GET ") {
                let body = r#"{"data":[]}"#;
                let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                return;
            }
            let delta = sse(json!({"type":"response.output_text.delta","delta":"DESKTOP_"}));
            if String::from_utf8_lossy(&body).contains("SLOW") {
                let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n{delta}");
                while !stop.load(Ordering::Relaxed) && socket.write_all(b": wait\n\n").is_ok() {
                    thread::sleep(Duration::from_millis(50));
                }
                return;
            }
            let done = sse(
                json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"DESKTOP_REPLY"}]}],"usage":{"input_tokens":20,"output_tokens":5}}}),
            );
            let reply = format!(
                "{}{done}",
                sse(json!({"type":"response.output_text.delta","delta":"DESKTOP_REPLY"}))
            );
            let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
        });
    }
}

/// Events until `agent_settled`, as one string.
fn until_settled(engine: &mut Engine) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = String::new();
    while Instant::now() < deadline {
        if let Some(event) = engine.next_event(Duration::from_millis(100)).unwrap() {
            seen.push_str(&event.to_string());
            seen.push('\n');
            if event["type"] == "agent_settled" {
                return seen;
            }
        }
    }
    panic!("run did not settle: {seen}");
}

fn prompt(engine: &mut Engine, text: &str) -> String {
    engine
        .send(json!({"type":"prompt","message":text}))
        .unwrap();
    until_settled(engine)
}

#[test]
fn desktop_api_runs_engines_per_workspace_saves_keys_and_aborts() {
    let root = support::unique_temp_dir("hibiscus-desktop");
    let home = root.join("app");
    let (a, b) = (root.join("a"), root.join("b"));
    for dir in [&home, &a, &b] {
        fs::create_dir(dir).unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    fs::write(
        home.join("config.json"),
        json!({"model":"test/model","providers":[
            {"id":"test","baseUrl":base,"api":"responses","apiKeyEnv":"HIBISCUS_TEST_DESKTOP_KEY","models":[{"id":"model","contextWindow":128000,"maxTokens":4096}]},
            {"id":"rooxcode","baseUrl":base,"api":"chat_completions","authentication":"rooxcode"}
        ]})
        .to_string(),
    )
    .unwrap();
    std::env::set_var("HIBISCUS_HOME", &home);
    std::env::set_var("HIBISCUS_TEST_DESKTOP_KEY", "SYNTHETIC-KEY");
    for name in ["HIBISCUS_MODEL", "OPENROUTER_API_KEY", "OPENCODE_API_KEY"] {
        std::env::remove_var(name);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let server = thread::spawn({
        let stop = stop.clone();
        move || serve(listener, stop)
    });

    // Two engines in one process, each in its own workspace.
    let mut first = Engine::start(Start::New, &a).unwrap();
    let mut second = Engine::start(Start::New, &b).unwrap();
    assert!(prompt(&mut first, "hello from a").contains("DESKTOP_REPLY"));
    assert!(prompt(&mut second, "hello from b").contains("DESKTOP_REPLY"));
    let titles = |dir: &Path| {
        desktop::sessions(dir)
            .unwrap()
            .into_iter()
            .map(|s| s.title)
            .collect::<Vec<_>>()
    };
    assert_eq!(titles(&a), ["hello from a"]);
    assert_eq!(titles(&b), ["hello from b"]);

    // Abort settles a run that is still streaming.
    first
        .send(json!({"type":"prompt","message":"SLOW please"}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let event = first.next_event(Duration::from_millis(100)).unwrap();
        if event.is_some_and(|e| e["type"] == "message_update") {
            break;
        }
        assert!(Instant::now() < deadline, "slow stream never started");
    }
    first.send(json!({"type":"abort"})).unwrap();
    until_settled(&mut first);

    // A resumed engine sees the earlier conversation.
    first.shutdown();
    let mut resumed = Engine::start(Start::Latest, &a).unwrap();
    let id = resumed.send(json!({"type":"get_messages"})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let messages = loop {
        if let Some(e) = resumed.next_event(Duration::from_millis(100)).unwrap() {
            if e["type"] == "response" && e["id"] == id.as_str() {
                break e;
            }
        }
        assert!(Instant::now() < deadline, "no get_messages response");
    };
    assert!(messages.to_string().contains("hello from a"));
    assert!(resumed.send(json!({"message":"no type"})).is_err());
    resumed.shutdown();
    second.shutdown();

    // API keys: validated, stored privately, unknown providers refused.
    assert!(desktop::save_api_key("test", "").is_err());
    assert!(desktop::save_api_key("test", "bad\nkey").is_err());
    assert!(desktop::save_api_key("nope", "KEY").is_err());
    assert!(!home.join("auth.json").exists());
    desktop::save_api_key("test", "SAVED-KEY").unwrap();
    let auth: Value = serde_json::from_slice(&fs::read(home.join("auth.json")).unwrap()).unwrap();
    assert_eq!(auth["test"], "SAVED-KEY");
    let mode = fs::metadata(home.join("auth.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(
        desktop::key_page("openrouter"),
        Some("openrouter.ai/settings/keys")
    );
    assert_eq!(desktop::key_page("test"), None);
    let providers = desktop::providers().unwrap();
    let test = providers.iter().find(|p| p.id == "test").unwrap();
    assert!(test.api_key && test.signed_in && test.key_page.is_none());
    let openrouter = providers.iter().find(|p| p.id == "openrouter").unwrap();
    assert!(openrouter.api_key && !openrouter.signed_in);
    assert!(providers.iter().any(|p| p.id == "rooxcode" && !p.api_key));

    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    fs::remove_dir_all(&root).unwrap();
}
