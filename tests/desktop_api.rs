//! The library API used by the desktop app. One test per binary: it sets
//! process-wide environment variables, which the in-process engine reads.
//! Runs on Windows too, so it doesn't use the Unix PTY harness in `support`.
use hibiscus::desktop::{self, Engine, SessionKind, Start};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("{prefix}-{}-{tick}", std::process::id()));
    fs::create_dir(&root).unwrap();
    root.canonicalize().unwrap()
}

fn sse(v: Value) -> String {
    format!("data: {v}\n\n")
}

/// Answers every model request with a reply; a prompt containing SLOW gets a
/// stream that never finishes, so it can be aborted.
fn serve(listener: TcpListener, stop: Arc<AtomicBool>, catalog: PathBuf) {
    listener.set_nonblocking(true).unwrap();
    while !stop.load(Ordering::Relaxed) {
        let Ok((socket, _)) = listener.accept() else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        socket.set_nonblocking(false).unwrap();
        let stop = stop.clone();
        let catalog = catalog.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut socket = socket;
            let mut route = String::new();
            reader.read_line(&mut route).unwrap();
            let mut length = 0;
            let mut authorization = None;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    } else if name.eq_ignore_ascii_case("authorization") {
                        authorization = Some(value.trim().to_owned());
                    }
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            if route.starts_with("GET /local/v1/models ") {
                assert!(authorization.is_none());
                let fixture: Value = serde_json::from_slice(&fs::read(catalog).unwrap()).unwrap();
                let status = fixture["status"].as_u64().unwrap();
                let body = fixture["body"].to_string();
                write!(socket, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                return;
            }
            if route.starts_with("POST /local/v1/chat/completions ") {
                assert!(authorization.is_none());
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["model"], "second");
                assert_eq!(body["reasoning_effort"], "low");
                assert!(body.get("max_tokens").is_none());
                let reply = sse(
                    json!({"choices":[{"delta":{"content":"LOCAL_REPLY"},"finish_reason":"stop"}]}),
                );
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
                return;
            }
            if route.starts_with("GET ") {
                let body = r#"{"data":[]}"#;
                let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                return;
            }
            let request: Value = serde_json::from_slice(&body).unwrap();
            if let Some(input) = request["input"].as_array() {
                let latest = input.last().unwrap().to_string();
                if latest.contains("QUEUE_PROMOTION_ACTIVE") {
                    let ready = catalog.with_file_name("queue-release");
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while !ready.exists() {
                        assert!(Instant::now() < deadline, "queue fixture was not released");
                        thread::sleep(Duration::from_millis(5));
                    }
                    tool_reply(
                        &mut socket,
                        "queue-read",
                        "read",
                        json!({"path":"input.txt"}),
                    );
                    return;
                }
                if latest.contains("MIXED_CHECKOUT_CALL") {
                    let reply = sse(json!({
                        "type":"response.completed",
                        "response":{"status":"completed","output":[
                            {"type":"function_call","call_id":"mixed-bind","name":"working_repository","arguments":json!({"path":"project"}).to_string()},
                            {"type":"function_call","call_id":"premature-write","name":"write","arguments":json!({"path":"premature.txt","content":"do not write"}).to_string()}
                        ]}
                    }));
                    write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
                    return;
                }
                if latest.contains("TARGET_CHECKOUT") {
                    let tools = request["tools"].as_array().unwrap();
                    assert!(tools
                        .iter()
                        .any(|tool| tool["name"] == "working_repository"));
                    tool_reply(
                        &mut socket,
                        "bind",
                        "working_repository",
                        json!({"path":"project"}),
                    );
                    return;
                }
                if input.last().is_some_and(|last| last["call_id"] == "bind") {
                    assert!(latest.contains("Working repository is"), "{latest}");
                    assert_project_instructions(&request);
                    tool_reply(
                        &mut socket,
                        "edit-project",
                        "edit",
                        json!({"path":"file.txt","oldText":"original","newText":"scoped edit"}),
                    );
                    return;
                }
                if latest.contains("READ_PROJECT_INSTRUCTIONS") {
                    assert_project_instructions(&request);
                    tool_reply(
                        &mut socket,
                        "read-project",
                        "read",
                        json!({"path":"AGENTS.md"}),
                    );
                    return;
                }
                if latest.contains("TRY_SWITCH_CHECKOUT") {
                    tool_reply(
                        &mut socket,
                        "switch",
                        "working_repository",
                        json!({"path":"sibling"}),
                    );
                    return;
                }
                let canvas = latest.contains("ASK_IN_CANVAS");
                if canvas || latest.contains("ASK_IN_CHAT") {
                    let tools = request["tools"].as_array().unwrap();
                    let has = |name: &str| tools.iter().any(|tool| tool["name"] == name);
                    assert!(has("ask_user"));
                    for name in ["storyboard", "video_preview", "text_to_speech"] {
                        assert_eq!(has(name), canvas, "unexpected visibility for {name}");
                    }
                    let instructions = request["instructions"].as_str().unwrap();
                    assert_eq!(instructions.contains("This is a Canvas session"), canvas);
                    let arguments = json!({"questions":[{"question":"Which output?","options":["JSON","Text"]}]});
                    let reply = sse(json!({
                        "type":"response.completed",
                        "response":{"status":"completed","output":[
                            {"type":"function_call","call_id":"clarify","name":"ask_user","arguments":arguments.to_string()}
                        ]}
                    }));
                    write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
                    return;
                }
                // A completed question should wait for the user before the next generation.
                assert!(!input
                    .last()
                    .is_some_and(|last| last["call_id"] == "clarify"));
            }
            let delta = sse(json!({"type":"response.output_text.delta","delta":"DESKTOP_"}));
            if String::from_utf8_lossy(&body).contains("SLOW") {
                let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n{delta}");
                while !stop.load(Ordering::Relaxed) && socket.write_all(b": wait\n\n").is_ok() {
                    thread::sleep(Duration::from_millis(50));
                }
                return;
            }
            let summary = request["instructions"]
                .as_str()
                .is_some_and(|value| value.starts_with("Summarize this conversation"));
            let text = if summary {
                r#"{"summary":"MODEL_SWITCH_SUMMARY"}"#
            } else {
                "DESKTOP_REPLY"
            };
            let latest = request["input"].as_array().and_then(|input| input.last());
            let input_tokens = if latest
                .is_some_and(|last| last.to_string().contains("MODEL_SWITCH_OLD_RECEIPT"))
            {
                500_000
            } else {
                20
            };
            let done = sse(
                json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":input_tokens,"output_tokens":5}}}),
            );
            let reply = format!(
                "{}{done}",
                sse(json!({"type":"response.output_text.delta","delta":text}))
            );
            let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
        });
    }
}

fn assert_project_instructions(request: &Value) {
    let instructions = request["instructions"].as_str().unwrap();
    assert!(instructions.contains("OPEN_FOLDER_RULE"));
    assert!(instructions.contains("ACTIVE_PROJECT_RULE"));
    assert!(!instructions.contains("SIBLING_PROJECT_RULE"));
}

fn tool_reply(out: &mut impl Write, id: &str, name: &str, args: Value) {
    let reply = sse(json!({
        "type":"response.completed",
        "response":{"status":"completed","output":[
            {"type":"function_call","call_id":id,"name":name,"arguments":args.to_string()}
        ]}
    }));
    write!(out, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
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

fn ask(engine: &mut Engine, command: Value) -> Value {
    let id = engine.send(command).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(event) = engine.next_event(Duration::from_millis(100)).unwrap() {
            if event["type"] == "response" && event["id"] == id.as_str() {
                assert_eq!(event["success"], true, "{event}");
                return event["data"].clone();
            }
        }
        assert!(Instant::now() < deadline, "no response for {id}");
    }
}

fn verify_context_switching(root: &Path, home: &Path) {
    let folder = root.join("model-switch");
    fs::create_dir(&folder).unwrap();
    let mut engine = Engine::start(Start::New, &folder).unwrap();
    let _ = prompt(
        &mut engine,
        &format!(
            "MODEL_SWITCH_OLD_RECEIPT {}",
            "retained context ".repeat(2_100)
        ),
    );
    let stats = |engine: &mut Engine| ask(engine, json!({"type":"get_session_stats"}));
    let select = |engine: &mut Engine, model: &str| {
        ask(
            engine,
            json!({"type":"set_model","provider":"test","modelId":model}),
        );
    };
    let previous = stats(&mut engine);
    assert_eq!(previous["contextUsage"]["tokens"], 500_005);
    assert_eq!(previous["contextUsage"]["contextWindow"], 1_000_000);
    assert_eq!(previous["contextUsage"]["estimated"], false);
    for (model, window) in [("mid", 400_000), ("small", 256_000)] {
        select(&mut engine, model);
        let next = stats(&mut engine);
        assert_eq!(next["tokens"], previous["tokens"]);
        assert_eq!(next["contextUsage"]["contextWindow"], window);
        assert_eq!(next["contextUsage"]["estimated"], true);
        let estimate = next["contextUsage"]["tokens"].as_u64().unwrap();
        assert!((8_192..256_000).contains(&estimate), "{next}");
    }
    let fit = prompt(&mut engine, "This turn still fits the 256K model.");
    assert!(!fit.contains("compaction_start"), "{fit}");
    assert_eq!(stats(&mut engine)["contextUsage"]["estimated"], false);
    let archives = home.join("archives");
    let before = fs::read_dir(&archives).map_or(0, |entries| entries.count());
    select(&mut engine, "tiny");
    let after_switch = stats(&mut engine);
    assert_eq!(after_switch["contextUsage"]["contextWindow"], 8_192);
    assert_eq!(after_switch["contextUsage"]["estimated"], true);
    assert!(after_switch["contextUsage"]["percent"].as_f64().unwrap() > 100.0);
    assert_eq!(
        fs::read_dir(&archives).map_or(0, |entries| entries.count()),
        before
    );
    let overflow = prompt(&mut engine, "Continue with the tiny model.");
    assert!(overflow.contains("compaction_start"), "{overflow}");
    assert!(overflow.contains("compaction_end"), "{overflow}");
    assert!(overflow.contains("DESKTOP_REPLY"), "{overflow}");
    assert!(ask(&mut engine, json!({"type":"get_messages"}))
        .to_string()
        .contains("MODEL_SWITCH_SUMMARY"));
    assert_eq!(fs::read_dir(&archives).unwrap().count(), before + 1);
    engine.shutdown();
}

fn verify_queued_message_promotion(root: &Path) {
    fn queue_state(engine: &mut Engine, entries: usize, steering: bool) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(event) = engine.next_event(Duration::from_millis(100)).unwrap() {
                if event["type"] == "queue_update" {
                    let prompts = event["prompts"].as_array().unwrap();
                    if prompts.len() == entries && (prompts[0]["steer"] == true) == steering {
                        return prompts.clone();
                    }
                }
            }
            assert!(Instant::now() < deadline, "missing queue state");
        }
    }

    let folder = root.join("queued-send-now");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("input.txt"), "fixture").unwrap();
    let mut engine = Engine::start(Start::New, &folder).unwrap();
    engine
        .send(json!({"type":"prompt","message":"QUEUE_PROMOTION_ACTIVE"}))
        .unwrap();
    engine
        .send(json!({"type":"prompt","message":"Same words","streamingBehavior":"followUp"}))
        .unwrap();
    let first = queue_state(&mut engine, 1, false);
    engine
        .send(json!({
            "type":"prompt","message":"Same words","streamingBehavior":"followUp",
            "images":[{"mimeType":"image/png","data":"aW1hZ2U="}]
        }))
        .unwrap();
    let waiting = queue_state(&mut engine, 2, false);
    assert_eq!(waiting[0]["id"], first[0]["id"]);
    assert_ne!(waiting[0]["id"], waiting[1]["id"]);
    let selected = &waiting[1]["id"];
    engine
        .send(json!({"type":"steer_queued_prompt","queueId":selected}))
        .unwrap();
    let promoted = queue_state(&mut engine, 2, true);
    assert_eq!(promoted[0]["id"], *selected);
    assert_eq!(promoted[0]["images"], 1);
    assert_eq!(promoted[1], waiting[0]);

    // Clicking twice while the message waits for the next step is idempotent.
    ask(
        &mut engine,
        json!({"type":"steer_queued_prompt","queueId":selected}),
    );
    fs::write(root.join("queue-release"), "").unwrap();
    let settled = until_settled(&mut engine);
    assert!(!settled.contains("stopReason\":\"error"), "{settled}");
    let history = ask(&mut engine, json!({"type":"get_messages"}));
    let user: Vec<_> = history["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .collect();
    assert_eq!(user.len(), 3);
    assert_eq!(user[1]["content"][0]["text"], "Same words");
    assert_eq!(user[1]["content"][1]["type"], "image");
    assert_eq!(user[1]["content"][1]["data"], "aW1hZ2U=");
    assert_eq!(
        user[2]["content"],
        json!([{"type":"text","text":"Same words"}])
    );

    let request = engine
        .send(json!({"type":"steer_queued_prompt","queueId":selected}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(event) = engine.next_event(Duration::from_millis(100)).unwrap() {
            if event["type"] == "response" && event["id"] == request.as_str() {
                assert_eq!(event["success"], false);
                assert!(event["error"]
                    .as_str()
                    .unwrap()
                    .contains("already left the queue"));
                break;
            }
        }
        assert!(Instant::now() < deadline, "missing late promotion response");
    }
    engine.shutdown();
}

#[test]
fn desktop_api_runs_engines_per_workspace_saves_keys_and_aborts() {
    let root = unique_temp_dir("hibiscus-desktop");
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
            {"id":"test","baseUrl":base,"api":"responses","apiKeyEnv":"HIBISCUS_TEST_DESKTOP_KEY","models":[
                {"id":"model","contextWindow":1000000,"maxTokens":4096,"images":true},
                {"id":"mid","contextWindow":400000,"maxTokens":4096},
                {"id":"small","contextWindow":256000,"maxTokens":4096},
                {"id":"tiny","contextWindow":8192,"maxTokens":512}
            ]},
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
    let catalog = root.join("local-catalog.json");
    let server = thread::spawn({
        let stop = stop.clone();
        let catalog = catalog.clone();
        move || serve(listener, stop, catalog)
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

    // Sidebar switches can open chats whose provider or model was removed.
    let mut recovering = Engine::start(Start::New, &a).unwrap();
    for (name, selection) in [
        ("renamed-provider", ["MyProvider", "old-model"]),
        ("removed-model", ["test", "old-model"]),
    ] {
        let saved = home.join("sessions").join(format!("{name}.jsonl"));
        let header = json!({"type":"session","format":"hibiscus","version":1,
            "kind":"chat","id":name,"cwd":a,"model":selection});
        let message = json!({"type":"message","message":{"role":"user","content":[
            {"type":"text","text":"History from the old provider"}
        ]}});
        let original = format!("{header}\n{message}\n");
        fs::write(&saved, &original).unwrap();

        ask(
            &mut recovering,
            json!({"type":"switch_session","sessionPath":saved}),
        );
        let state = ask(&mut recovering, json!({"type":"get_state"}));
        assert!(state["model"].is_null());
        assert_eq!(state["modelSelectionUnavailable"], true);
        let messages = ask(&mut recovering, json!({"type":"get_messages"}));
        assert_eq!(
            messages["messages"][0]["content"][0]["text"],
            "History from the old provider"
        );
        assert_eq!(fs::read_to_string(&saved).unwrap(), original);

        let state = ask(
            &mut recovering,
            json!({"type":"set_model","provider":"test","modelId":"model"}),
        );
        assert_eq!(state["model"]["provider"], "test");
        assert_eq!(state["modelSelectionUnavailable"], false);
        assert!(prompt(&mut recovering, "Continue this chat.").contains("DESKTOP_REPLY"));
        let persisted = fs::read_to_string(saved).unwrap();
        let header: Value = serde_json::from_str(persisted.lines().next().unwrap()).unwrap();
        assert_eq!(header["model"], json!(["test", "model"]));
        assert!(persisted.contains("History from the old provider"));
    }
    recovering.shutdown();

    let mut kinds = Engine::start(Start::Canvas, &a).unwrap();
    let state = ask(&mut kinds, json!({"type":"get_state"}));
    assert_eq!(state["sessionKind"], "canvas");
    let canvas = PathBuf::from(state["sessionFile"].as_str().unwrap());
    assert!(
        canvas.is_file(),
        "Canvas drafts can save a board before the first prompt"
    );
    assert!(desktop::sessions(&a)
        .unwrap()
        .iter()
        .any(|s| s.path == canvas && s.kind == SessionKind::Canvas));
    let events = prompt(&mut kinds, "ASK_IN_CANVAS");
    assert!(events.contains("Which output?"), "{events}");
    assert!(!events.contains("DESKTOP_REPLY"), "{events}");
    desktop::save_canvas(&canvas, r#"{"version":1,"cards":[{}]}"#).unwrap();

    let legacy = |path: &Path| {
        let stored = fs::read_to_string(path).unwrap();
        let (header, rest) = stored.split_once('\n').unwrap();
        let mut header: Value = serde_json::from_str(header).unwrap();
        header.as_object_mut().unwrap().remove("kind");
        fs::write(path, format!("{header}\n{rest}")).unwrap();
    };

    ask(&mut kinds, json!({"type":"new_session"}));
    assert_eq!(
        ask(&mut kinds, json!({"type":"get_state"}))["sessionKind"],
        "chat"
    );
    let events = prompt(&mut kinds, "ASK_IN_CHAT");
    assert!(events.contains("Which output?"), "{events}");
    assert!(!events.contains("DESKTOP_REPLY"), "{events}");
    let pending = ask(&mut kinds, json!({"type":"get_messages"}));
    assert!(pending["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["role"] == "toolResult"
            && item["details"]["questions"][0]["question"] == "Which output?"));
    let chat = PathBuf::from(
        ask(&mut kinds, json!({"type":"get_state"}))["sessionFile"]
            .as_str()
            .unwrap(),
    );
    assert!(desktop::load_canvas(&chat).unwrap().is_none());
    assert!(prompt(&mut kinds, "Text, please").contains("DESKTOP_REPLY"));
    legacy(&canvas);
    legacy(&chat);
    ask(
        &mut kinds,
        json!({"type":"switch_session","sessionPath":canvas}),
    );
    assert_eq!(
        ask(&mut kinds, json!({"type":"get_state"}))["sessionKind"],
        "canvas"
    );
    assert!(desktop::load_canvas(&canvas).unwrap().is_some());
    ask(
        &mut kinds,
        json!({"type":"set_session_name","name":"Continued Canvas"}),
    );
    desktop::save_canvas(&canvas, "").unwrap();
    ask(
        &mut kinds,
        json!({"type":"switch_session","sessionPath":chat}),
    );
    assert_eq!(
        ask(&mut kinds, json!({"type":"get_state"}))["sessionKind"],
        "chat"
    );
    ask(
        &mut kinds,
        json!({"type":"switch_session","sessionPath":canvas}),
    );
    assert_eq!(
        ask(&mut kinds, json!({"type":"get_state"}))["sessionKind"],
        "canvas"
    );
    ask(&mut kinds, json!({"type":"new_session","kind":"canvas"}));
    let state = ask(&mut kinds, json!({"type":"get_state"}));
    assert_eq!(state["sessionKind"], "canvas");
    assert_ne!(state["sessionFile"], canvas.to_str().unwrap());
    kinds.shutdown();

    // The parent owns the conversation while the selected checkout owns its instructions and files.
    fs::write(a.join("AGENTS.md"), "OPEN_FOLDER_RULE").unwrap();
    let project = a.join("project");
    let sibling = a.join("sibling");
    for (repo, rule) in [
        (&project, "ACTIVE_PROJECT_RULE"),
        (&sibling, "SIBLING_PROJECT_RULE"),
    ] {
        fs::create_dir(repo).unwrap();
        assert!(std::process::Command::new("git")
            .current_dir(repo)
            .args(["init", "--quiet"])
            .status()
            .unwrap()
            .success());
        fs::write(repo.join("AGENTS.md"), rule).unwrap();
        fs::write(repo.join("file.txt"), "original").unwrap();
    }
    let mut scoped = Engine::start(Start::New, &a).unwrap();
    let events = prompt(&mut scoped, "MIXED_CHECKOUT_CALL");
    assert!(events.contains("No calls ran"), "{events}");
    assert!(!a.join("premature.txt").exists());
    assert!(!project.join("premature.txt").exists());
    assert_eq!(
        ask(&mut scoped, json!({"type":"get_state"}))["workingDirectory"],
        json!(a)
    );
    let events = prompt(&mut scoped, "TARGET_CHECKOUT project. Make the edit there.");
    assert!(events.contains("working_repository_changed"), "{events}");
    assert!(events.contains("checkpoint"), "{events}");
    assert_eq!(
        fs::read_to_string(project.join("file.txt")).unwrap(),
        "scoped edit"
    );
    assert_eq!(
        fs::read_to_string(sibling.join("file.txt")).unwrap(),
        "original"
    );
    assert!(!a.join("file.txt").exists());
    let state = ask(&mut scoped, json!({"type":"get_state"}));
    assert_eq!(state["workingDirectory"], json!(project));
    let saved = PathBuf::from(state["sessionFile"].as_str().unwrap());
    let header: Value =
        serde_json::from_str(fs::read_to_string(&saved).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(header["cwd"], json!(a));
    assert_eq!(header["workingRepository"], json!(project));
    scoped.shutdown();

    let mut scoped = Engine::start(Start::Selected(saved), &a).unwrap();
    let events = prompt(&mut scoped, "READ_PROJECT_INSTRUCTIONS read the AGENTS.md");
    assert!(events.contains("ACTIVE_PROJECT_RULE"), "{events}");
    assert!(!events.contains("SIBLING_PROJECT_RULE"), "{events}");
    let events = prompt(&mut scoped, "TRY_SWITCH_CHECKOUT");
    assert!(events.contains("Ask the user to switch"), "{events}");
    assert_eq!(
        ask(&mut scoped, json!({"type":"get_state"}))["workingDirectory"],
        json!(project)
    );
    assert_eq!(
        ask(
            &mut scoped,
            json!({"type":"set_working_repository","path":sibling})
        )["workingDirectory"],
        json!(sibling)
    );
    fs::rename(&sibling, a.join("moved-sibling")).unwrap();
    let events = prompt(&mut scoped, "Read files after the checkout moved.");
    assert!(
        events.contains("working repository moved or is unavailable"),
        "{events}"
    );
    assert!(!events.contains("DESKTOP_REPLY"), "{events}");
    scoped.shutdown();

    verify_context_switching(&root, &home);
    verify_queued_message_promotion(&root);

    // API keys: validated, stored privately, unknown providers refused.
    assert!(desktop::save_api_key("test", "").is_err());
    assert!(desktop::save_api_key("test", "bad\nkey").is_err());
    assert!(desktop::save_api_key("nope", "KEY").is_err());
    assert!(!home.join("auth.json").exists());
    desktop::save_api_key("test", "SAVED-KEY").unwrap();
    let auth: Value = serde_json::from_slice(&fs::read(home.join("auth.json")).unwrap()).unwrap();
    assert_eq!(auth["test"], "SAVED-KEY");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(home.join("auth.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
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

    let local_url = base.replace("/v1", "/local/v1");
    let entry = |id| json!({"id":id,"context_length":32768,"supported_parameters":["tools","reasoning"],"architecture":{"input_modalities":["text"]}});
    let publish_catalog = |status, entries| {
        fs::write(
            &catalog,
            json!({"status":status,"body":{"data":entries}}).to_string(),
        )
        .unwrap();
    };
    publish_catalog(200, vec![entry("first")]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let models = desktop::detect_local_models(&local_url, "").await.unwrap();
        let preview = serde_json::to_value(models).unwrap();
        assert_eq!(
            preview[0],
            json!({"id":"first","contextWindow":32768,"reasoning":true,"images":false})
        );
        assert!(desktop::detect_local_models("https://example.org/v1", "")
            .await
            .is_err());
        desktop::add_local_provider(desktop::LocalProvider {
            id: "ornith".into(),
            base_url: local_url,
            api: "chat_completions".into(),
            models: vec![],
            context_window: None,
            discover_models: true,
            key: String::new(),
        })
        .await
        .unwrap();
    });
    let saved: Value =
        serde_json::from_slice(&fs::read(home.join("config.json")).unwrap()).unwrap();
    let local = saved["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "ornith")
        .unwrap();
    assert_eq!(local["discoverModels"], true);
    assert_eq!(local["models"][0]["id"], "first");
    let local = desktop::providers()
        .unwrap()
        .into_iter()
        .find(|p| p.id == "ornith")
        .unwrap();
    assert!(local.local && local.signed_in && !local.api_key);
    let mut detected = Engine::start(Start::New, &a).unwrap();
    let list = |engine: &mut Engine| ask(engine, json!({"type":"get_available_models"}));
    publish_catalog(200, vec![entry("first"), entry("second")]);
    let latest = list(&mut detected);
    let new = latest["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["provider"] == "ornith" && m["id"] == "second")
        .unwrap();
    assert_eq!(new["contextWindow"], 32768);
    assert_eq!(new["thinkingLevels"], json!(["off", "on"]));
    publish_catalog(503, vec![]);
    for _ in 0..2 {
        let outage = list(&mut detected);
        assert_eq!(outage["models"], latest["models"]);
        assert!(outage["catalogError"]
            .as_str()
            .unwrap()
            .contains("ornith: model discovery returned HTTP 503"));
    }
    ask(
        &mut detected,
        json!({"type":"set_model","provider":"ornith","modelId":"second"}),
    );
    ask(
        &mut detected,
        json!({"type":"set_thinking_level","level":"on"}),
    );
    assert!(prompt(&mut detected, "Use the selected local model.").contains("LOCAL_REPLY"));
    detected.shutdown();

    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    fs::remove_dir_all(&root).unwrap();
}
