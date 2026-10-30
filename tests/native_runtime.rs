mod support;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use support::{unique_temp_dir, Pty};
struct Fixture {
    root: PathBuf,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn new(api: &str, replies: Vec<(u16, String)>) -> Self {
        let root = unique_temp_dir("hibiscus-native");
        let home = root.join("app");
        fs::create_dir(&home).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        fs::write(home.join("config.json"),serde_json::to_vec(&json!({"model":"test/model","providers":[{"id":"test","baseUrl":format!("http://{address}/v1"),"api":api,"apiKeyEnv":"HIBISCUS_TEST_NATIVE_KEY","models":[{"id":"model","contextWindow":128000,"maxTokens":4096,"images":true}]}]})).unwrap()).unwrap();
        let requests = Arc::new(Mutex::new(vec![]));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let server = thread::spawn(move || {
            let mut replies = VecDeque::from(replies);
            let deadline = Instant::now() + Duration::from_secs(45);
            while !stopped.load(Ordering::Relaxed) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false).unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        socket
                            .set_write_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let mut reader = BufReader::new(socket.try_clone().unwrap());
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        let route = line.clone();
                        let mut length = 0;
                        loop {
                            line.clear();
                            reader.read_line(&mut line).unwrap();
                            if line == "\r\n" {
                                break;
                            }
                            if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
                                length = n.trim().parse::<usize>().unwrap();
                            }
                        }
                        assert!(length < 64 * 1024 * 1024);
                        let mut bytes = vec![0; length];
                        reader.read_exact(&mut bytes).unwrap();
                        let mut body: Value = serde_json::from_slice(&bytes).unwrap();
                        body["fixture_route"] = json!(route);
                        captured.lock().unwrap().push(body);
                        let (status, body) = replies
                            .pop_front()
                            .unwrap_or((500, "unexpected request".into()));
                        let actual = if status == 298 || status == 299 {
                            200
                        } else {
                            status
                        };
                        let reason = if actual == 200 { "OK" } else { "Error" };
                        let length = if status == 299 {
                            1024 * 1024
                        } else {
                            body.len()
                        };
                        let header=format!("HTTP/1.1 {actual} {reason}\r\nContent-Type: text/event-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n");
                        if status == 299 && socket.write_all(header.as_bytes()).is_ok() {
                            let _ = socket.write_all(body.as_bytes());
                            for _ in 0..100 {
                                if stopped.load(Ordering::Relaxed)
                                    || socket.write_all(b": waiting\n\n").is_err()
                                {
                                    break;
                                }
                                thread::sleep(Duration::from_millis(50));
                            }
                            continue;
                        }
                        if status == 298 && socket.write_all(header.as_bytes()).is_ok() {
                            let split = body.find("\n\n").unwrap() + 2;
                            let _ = socket.write_all(&body.as_bytes()[..split]);
                            thread::sleep(Duration::from_millis(500));
                            let _ = socket.write_all(&body.as_bytes()[split..]);
                            continue;
                        }
                        if socket.write_all(header.as_bytes()).is_ok() {
                            for part in body.as_bytes().chunks(7) {
                                if socket.write_all(part).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self {
            root,
            requests,
            stop,
            server: Some(server),
        }
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
        c.current_dir(&self.root)
            .env("HIBISCUS_HOME", self.root.join("app"))
            .env("HOME", &self.root)
            .env("HIBISCUS_TEST_NATIVE_KEY", "SYNTHETIC-KEY")
            .env("HIBISCUS_NO_UPDATE_CHECK", "1")
            .env_remove("HIBISCUS_MODEL")
            .env("HIBISCUS_PI", "/not-installed/pi")
            .env("HIBISCUS_NODE", "/not-installed/node");
        c
    }
    fn run(&self, input: &str, args: &[&str]) -> std::process::Output {
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if child.try_wait().unwrap().is_some() {
                return child.wait_with_output().unwrap();
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("native CLI timed out");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn captured(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(s) = self.server.take() {
            let result = s.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn sse(v: Value) -> String {
    format!("data: {v}\n\n")
}
fn response(text: &str) -> (u16, String) {
    (
        200,
        format!(
            "{}{}",
            sse(json!({"type":"response.output_text.delta","delta":text})),
            sse(
                json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":20,"output_tokens":5}}})
            )
        ),
    )
}
fn calls(calls: Vec<(&str, Value)>) -> (u16, String) {
    (
        200,
        sse(
            json!({"type":"response.completed","response":{"status":"completed","output":calls.into_iter().enumerate().map(|(i,(name,args))|json!({"type":"function_call","call_id":format!("tool-{i}"),"name":name,"arguments":args.to_string()})).collect::<Vec<_>>(),"usage":{"input_tokens":20,"output_tokens":5}}}),
        ),
    )
}
fn good(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn native_responses_streams_without_pi_node_or_curl() {
    let f = Fixture::new("responses", vec![response("NATIVE_REPLY")]);
    let mut c = f.command();
    c.env("PATH", "/not-installed").arg("hello");
    let o = c.output().unwrap();
    good(&o);
    assert!(String::from_utf8_lossy(&o.stdout).contains("NATIVE_REPLY"));
    assert!(f.captured()[0]["fixture_route"]
        .as_str()
        .unwrap()
        .contains("/v1/responses"));
    assert!(f.root.join("app/sessions").is_dir());
}
#[test]
fn native_tools_complete_real_file_shell_and_goal_work() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![
                ("write", json!({"path":"demo.txt","content":"hello\n"})),
                ("read", json!({"path":"demo.txt"})),
                (
                    "edit",
                    json!({"path":"demo.txt","oldText":"hello","newText":"world"}),
                ),
                ("bash", json!({"command":"printf shell-result"})),
                (
                    "goal",
                    json!({"action":"set","steps":["Verify native tools"]}),
                ),
                ("goal", json!({"action":"complete","id":1})),
            ]),
            response("TOOLS_FINISHED"),
        ],
    );
    let o = f.run("run tools\n/quit\n", &[]);
    good(&o);
    assert_eq!(
        fs::read_to_string(f.root.join("demo.txt")).unwrap(),
        "world\n"
    );
    assert!(String::from_utf8_lossy(&o.stdout).contains("TOOLS_FINISHED"));
    let r = f.captured();
    assert_eq!(r.len(), 2);
    let results = r[1]["input"].as_array().unwrap();
    assert!(results.iter().any(|v| v["output"]
        .as_str()
        .is_some_and(|s| s.contains("shell-result"))));
    let saved = fs::read_dir(f.root.join("app/sessions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let data = fs::read_to_string(saved).unwrap();
    assert!(data.contains("hibiscusGoal"));
    assert!(data.contains("@@ -1,1 +1,1 @@"));
}
#[test]
fn noninteractive_shell_runs_without_a_permission_prompt() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![(
                "bash",
                json!({"command":"printf native-shell > shell-result"}),
            )]),
            response("SHELL_FINISHED"),
        ],
    );
    let output = f.run("", &["run the shell command"]);
    good(&output);
    assert_eq!(
        fs::read_to_string(f.root.join("shell-result")).unwrap(),
        "native-shell"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Approval"));
    assert!(!f.root.join("app/approval.json").exists());
}

#[test]
fn incomplete_stream_never_executes_a_tool() {
    let f = Fixture::new(
        "responses",
        vec![(
            200,
            sse(
                json!({"type":"response.output_item.added","item":{"type":"function_call","call_id":"x","name":"write","arguments":"{\"path\":\"bad\",\"content\":\"bad\"}"}}),
            ),
        )],
    );
    let o = f.run("", &["hello"]);
    assert!(!o.status.success());
    assert!(!f.root.join("bad").exists());
    assert!(String::from_utf8_lossy(&o.stderr).contains("completion event"));
}
#[test]
fn unknown_tool_and_ambiguous_edit_return_errors_without_mutation() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![
                ("invented", json!({})),
                (
                    "edit",
                    json!({"path":"same.txt","oldText":"same","newText":"new"}),
                ),
            ]),
            response("ERRORS_HANDLED"),
        ],
    );
    fs::write(f.root.join("same.txt"), "same same").unwrap();
    good(&f.run("", &["hello"]));
    assert_eq!(
        fs::read_to_string(f.root.join("same.txt")).unwrap(),
        "same same"
    );
    assert!(f.captured()[1]["input"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["output"]
            .as_str()
            .is_some_and(|s| s.contains("Unknown tool"))));
}
#[test]
fn session_resume_keeps_history_and_provider_tool_ids() {
    let f = Fixture::new(
        "responses",
        vec![response("FIRST_TURN"), response("SECOND_TURN")],
    );
    good(&f.run("", &["first"]));
    good(&f.run("second\n/quit\n", &["--continue"]));
    let r = f.captured();
    assert_eq!(r.len(), 2);
    assert!(r[1]["input"].to_string().contains("FIRST_TURN"));
    let files = fs::read_dir(f.root.join("app/sessions")).unwrap().count();
    assert_eq!(files, 1);
}
#[test]
fn manual_compaction_archives_history_and_continues() {
    let f = Fixture::new(
        "responses",
        vec![
            response("ORIGINAL_TURN"),
            response("COMPACTED_SUMMARY"),
            response("AFTER_COMPACT"),
        ],
    );
    let o = f.run("hello\n/compact\ncontinue\n/quit\n", &[]);
    good(&o);
    let requests = f.captured();
    assert_eq!(requests.len(), 3);
    assert!(requests[2]["input"]
        .to_string()
        .contains("COMPACTED_SUMMARY"));
    assert!(!requests[2]["input"].to_string().contains("ORIGINAL_TURN"));
    assert_eq!(
        fs::read_dir(f.root.join("app/archives")).unwrap().count(),
        1
    );
}
#[test]
fn provider_errors_do_not_echo_sensitive_bodies() {
    let f = Fixture::new(
        "responses",
        vec![(401, "PRIVATE_PROVIDER_TOKEN https://secret.invalid".into())],
    );
    let o = f.run("", &["hello"]);
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("401"));
    assert!(!e.contains("PRIVATE_PROVIDER_TOKEN"));
    assert!(!e.contains("secret.invalid"));
}
#[test]
fn chat_completions_assembles_fragmented_tool_arguments() {
    let data = format!(
        "{}{}{}data: [DONE]\n\n",
        sse(
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call1","function":{"name":"write","arguments":"{\"path\":\"chat.txt\","}}]}}]})
        ),
        sse(
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"content\":\"native\"}"}}]}}]})
        ),
        sse(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}))
    );
    let final_text = format!(
        "{}{}data: [DONE]\n\n",
        sse(json!({"choices":[{"delta":{"content":"CHAT_FINISHED"}}]})),
        sse(
            json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":3}})
        )
    );
    let f = Fixture::new("chat_completions", vec![(200, data), (200, final_text)]);
    good(&f.run("", &["hello"]));
    assert_eq!(
        fs::read_to_string(f.root.join("chat.txt")).unwrap(),
        "native"
    );
    assert_eq!(
        f.captured()[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool")
            .unwrap()["tool_call_id"],
        "call1"
    );
}
#[test]
fn anthropic_streams_tool_use_and_replays_results() {
    let tool = format!(
        "{}{}{}{}{}",
        sse(json!({"type":"message_start","message":{"usage":{"input_tokens":5}}})),
        sse(
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"a1","name":"write","input":{}}})
        ),
        sse(
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"claude.txt\",\"content\":\"native\"}"}})
        ),
        sse(
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":4}})
        ),
        sse(json!({"type":"message_stop"}))
    );
    let text = format!(
        "{}{}{}{}",
        sse(
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})
        ),
        sse(
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"CLAUDE_FINISHED"}})
        ),
        sse(
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}})
        ),
        sse(json!({"type":"message_stop"}))
    );
    let f = Fixture::new("anthropic", vec![(200, tool), (200, text)]);
    good(&f.run("", &["hello"]));
    assert_eq!(
        fs::read_to_string(f.root.join("claude.txt")).unwrap(),
        "native"
    );
    assert!(f.captured()[1]["messages"]
        .to_string()
        .contains("tool_result"));
}
#[test]
fn native_api_key_login_and_logout_are_masked_and_local() {
    let f = Fixture::new("responses", vec![]);
    let mut command = f.command();
    command.env_remove("HIBISCUS_TEST_NATIVE_KEY");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("no active model");
    tty.wait_text("Enter send");
    tty.send(b"/login\r");
    tty.wait_text("Sign in to Hibiscus");
    tty.send(b"\x1b[B\r");
    tty.wait_text("API Key · provider");
    tty.send(b"\r");
    tty.wait_text("Provider API key");
    tty.send(b"FAKE-PRIVATE-CREDENTIAL\r");
    tty.wait_text("sign-in completed");
    tty.wait_text("test/model");
    tty.wait_text("Enter send");
    let auth = f.root.join("app/auth.json");
    assert!(fs::read_to_string(&auth)
        .unwrap()
        .contains("FAKE-PRIVATE-CREDENTIAL"));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
        0o600
    );
    tty.send(b"/logout\r");
    tty.wait_text("Stored credentials");
    tty.send(b"\r");
    tty.wait_text("Remove stored credential");
    tty.send(b"\x1b[B\r");
    tty.wait_text("Logout complete");
    tty.wait_text("no active model");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("FAKE-PRIVATE-CREDENTIAL"));
    let views = support::screen::snapshots(&shown);
    assert!(views.last().unwrap().contains("no active model"));
    assert!(!views.last().unwrap().contains("test/model"));
    assert_eq!(fs::read_to_string(&auth).unwrap(), "{}");
}

#[test]
fn first_launch_without_credentials_shows_welcome_and_empty_logout_keeps_no_active_model() {
    let f = Fixture::new("responses", vec![]);
    let config_path = f.root.join("app/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config.as_object_mut().unwrap().remove("model");
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut command = f.command();
    command.env_remove("HIBISCUS_TEST_NATIVE_KEY");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter to send · Esc to stop");
    tty.wait_text("no active model");
    tty.resize(42, 24);
    tty.send(b"a");
    tty.wait_text("/login");
    tty.send(b"\x7f/logout\r");
    tty.wait_text("No stored credentials");
    tty.wait_text("no active model");
    tty.send(b"/new\r");
    tty.wait_text("Enter to send · Esc to stop");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let views = support::screen::snapshots(&shown);
    assert!(views
        .iter()
        .any(|v| v.contains("Enter to send · Esc to stop")));
    assert!(!views.iter().any(|v| v.contains("test/model")));
    assert!(f.captured().is_empty());
    assert!(!f.root.join("app/auth.json").exists());
}

#[test]
fn automatic_model_selection_skips_providers_without_credentials() {
    let f = Fixture::new("responses", vec![response("SIGNED_IN_DEFAULT")]);
    let config_path = f.root.join("app/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config.as_object_mut().unwrap().remove("model");
    config["providers"].as_array_mut().unwrap().insert(
        0,
        json!({
            "id":"logged-out", "baseUrl":"https://unused.invalid/v1", "api":"responses",
            "models":[{"id":"WRONG_DEFAULT"}]
        }),
    );
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    good(&f.run("", &["hello"]));
    assert_eq!(f.captured()[0]["model"], "model");
}

#[test]
fn empty_logout_keeps_environment_authenticated_model_available() {
    let f = Fixture::new("responses", vec![]);
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("test/model");
    tty.send(b"/logout\r");
    tty.wait_text("No stored credentials");
    tty.wait_text("test/model");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(support::screen::snapshots(&shown)
        .last()
        .unwrap()
        .contains("test/model"));
    assert!(f.captured().is_empty());
}
#[test]
fn fullscreen_model_picker_stays_searchable_without_chat_confirmation() {
    let f = Fixture::new("responses", vec![]);
    let mut command = f.command();
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"/models\r");
    tty.wait_text("Models · all providers");
    tty.send(b"absent");
    tty.wait_text("No matching models");
    tty.send(b"\x1b");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("Already using test/model"));
    assert!(!shown.contains("Selected test/model"));
}

#[test]
fn tui_cli_tui_switch_keeps_one_session_and_never_replays_prompts() {
    let f = Fixture::new(
        "responses",
        vec![response("BEFORE_SWITCH"), response("AFTER_SWITCH")],
    );
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("Enter send");
    tty.send(b"first message\r");
    tty.wait_text("BEFORE_SWITCH");
    tty.wait_text("Enter send");
    tty.send(b"/cli\r");
    tty.wait_text(" · CLI");
    tty.wait_text("/tui");
    tty.wait_text("BEFORE_SWITCH");
    tty.wait_text("❯ ");
    tty.send(b"second message\r");
    tty.wait_text("AFTER_SWITCH");
    tty.wait_text("❯ ");
    tty.send(b"/tui\r");
    tty.wait_text("BEFORE_SWITCH");
    tty.wait_text("AFTER_SWITCH");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(shown.contains("\x1b[?1049l"));
    assert_eq!(shown.matches("\x1b[?1049h").count(), 2);
    assert_eq!(f.captured().len(), 2);
    let files = fs::read_dir(f.root.join("app/sessions"))
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1);
    let history = fs::read_to_string(files.into_iter().next().unwrap().unwrap().path()).unwrap();
    assert!(!history.contains("/cli") && !history.contains("/tui"));
    assert_eq!(history.matches("first message").count(), 1);
    assert_eq!(history.matches("second message").count(), 1);
}

#[test]
fn explicit_cli_starts_inline_with_tui_hint_and_no_alternate_screen() {
    let f = Fixture::new("responses", vec![]);
    let mut command = f.command();
    command.arg("--cli").env_remove("HIBISCUS_TEST_NATIVE_KEY");
    let mut tty = Pty::spawn_with_color(&mut command, true);
    tty.wait_text(" · CLI");
    tty.wait_text("/tui");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("\x1b[?1049h"));
    assert!(shown.contains("\x1b[48;2;18;18;18m"));
    assert!(!shown.contains("\x1b[48;2;48;10;36m"));
    assert!(!shown.contains("\x1b]11;"));
    assert!(!shown.contains(['╭', '╮', '╰', '╯', '│', '█', '░']));
    assert!(shown.contains(&format!("{} · ", env!("CARGO_PKG_VERSION"))));
    assert!(shown.contains("no active model    /login"));
    assert!(shown.contains("/login"));
    assert!(f.captured().is_empty());
}

#[test]
fn cli_login_returns_inline_and_hides_login_tip_after_key_is_saved() {
    let f = Fixture::new("responses", vec![]);
    let mut command = f.command();
    command.arg("--cli").env_remove("HIBISCUS_TEST_NATIVE_KEY");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text(" · CLI");
    tty.send(b"/login\r");
    tty.wait_text("Sign in to Hibiscus");
    tty.send(b"\x1b[B\r");
    tty.wait_text("API Key · provider");
    tty.send(b"\r");
    tty.wait_text("Provider API key");
    tty.send(b"SYNTHETIC-CLI-SECRET\r");
    tty.wait_text(" · CLI");
    tty.wait_text("test/model");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let returned_cli = shown.rsplit("\x1b[?1049l").next().unwrap();
    assert!(returned_cli.contains("/tui"));
    assert!(!returned_cli.contains("/login"));
    assert!(!shown.contains("SYNTHETIC-CLI-SECRET"));
    assert!(f.captured().is_empty());
}

#[test]
fn dumb_cli_keeps_plain_input_without_editor_protocol_sequences() {
    let f = Fixture::new("responses", vec![]);
    let mut process = f.command();
    process.arg("--cli").env("TERM", "dumb");
    let mut tty = Pty::spawn(&mut process);
    tty.wait_text("/tui");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains('\x1b'));
    assert!(f.captured().is_empty());
}

#[test]
fn cli_multiline_keys_edit_one_draft_and_submit_one_prompt() {
    for color in [false, true] {
        let f = Fixture::new("responses", vec![response("MULTILINE_DONE")]);
        let mut process = f.command();
        process.arg("--cli");
        let mut tty = Pty::spawn_with_color(&mut process, color);
        tty.wait_text("\x1b[?2004h");
        tty.send(b"alpha\x1b\rsecond\nthird");
        tty.wait_text("third");
        assert!(f.captured().is_empty());
        tty.send(b"\x1b[A\x1b[F!\x1b[B\x1b[F\r");
        tty.wait_text("MULTILINE_DONE");
        tty.wait_text("\x1b[?2004h");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        let requests = f.captured();
        assert_eq!(requests.len(), 1);
        let users = requests[0]["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "user")
            .collect::<Vec<_>>();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0]["content"][0]["text"], "alpha\nsecond!\nthird");
        let views = support::screen::snapshots(&format!("{shown}\x1b[?2026l"));
        assert!(views
            .last()
            .unwrap()
            .contains("❯ alpha\n  second!\n  third"));
        assert!(!shown.contains("\x1b[?1049h"));
        if !color {
            assert!(!shown.contains("\x1b[38;") && !shown.contains("\x1b[48;"));
        }
    }
}

#[test]
fn cli_bracketed_paste_keeps_unicode_whitespace_and_embedded_commands_until_enter() {
    let f = Fixture::new("responses", vec![response("PASTE_DONE")]);
    let mut process = f.command();
    process.arg("--cli");
    let mut tty = Pty::spawn_with_size(&mut process, true, 24, 14);
    tty.wait_text("\x1b[?2004h");
    let raw = "\r\n  café 世界\r\n/quit\r\n\tkeep this line\r\n\r\n\r\n";
    tty.send(format!("\x1b[200~{raw}\x1b[201~").as_bytes());
    tty.wait_text("rows above");
    assert!(f.captured().is_empty());
    tty.send(b"\r");
    tty.wait_text("PASTE_DONE");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/quit\r");
    tty.finish();
    let requests = f.captured();
    assert_eq!(requests.len(), 1);
    let user = requests[0]["input"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .unwrap();
    assert_eq!(
        user["content"][0]["text"],
        "\n  café 世界\n/quit\n    keep this line\n\n\n"
    );
}

#[test]
fn cli_rejected_paste_keeps_the_existing_draft_and_never_submits() {
    let f = Fixture::new("responses", vec![response("KEPT_DRAFT")]);
    let mut process = f.command();
    process.arg("--cli");
    let mut tty = Pty::spawn(&mut process);
    tty.wait_text("\x1b[?2004h");
    tty.send(b"keep\x1b[200~bad\xff\n/quit\x1b[201~");
    tty.wait_text("Paste rejected");
    assert!(f.captured().is_empty());
    tty.send(b"\r");
    tty.wait_text("KEPT_DRAFT");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/quit\r");
    tty.finish();
    let requests = f.captured();
    assert_eq!(requests.len(), 1);
    let user = requests[0]["input"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .unwrap();
    assert_eq!(user["content"][0]["text"], "keep");
}

#[test]
fn saved_output_commands_are_local_complete_resumable_and_session_scoped() {
    for color in [false, true] {
        let command = format!(
            "printf '%s\\n' {}",
            (0..30)
                .map(|i| format!("'SAVED_LINE_{i:02}_payload'"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        assert!(command.len() > 400);
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![
                    ("write", json!({"path":"saved.txt","content":"old\n"})),
                    (
                        "edit",
                        json!({"path":"saved.txt","oldText":"old","newText":"new"}),
                    ),
                    ("bash", json!({"command":command})),
                ]),
                response("OUTPUT_READY"),
            ],
        );
        let mut process = f.command();
        process.arg("--cli");
        let mut tty = Pty::spawn_with_color(&mut process, color);
        tty.wait_text("/tui");
        tty.send(b"run the saved-output fixture\r");
        tty.wait_text("OUTPUT_READY");
        tty.wait_text("❯");
        tty.send(b"/output list\r");
        tty.wait_text("Recent tool results");
        tty.wait_text("Use /output <number>");
        tty.wait_text("❯");
        tty.send(b"/output\r");
        tty.wait_text("Saved output #3");
        tty.wait_text("      SAVED_LINE_15_payload");
        tty.wait_text("❯");
        tty.send(b"/output 2\r");
        tty.wait_text("Saved output #2");
        tty.wait_text("+new");
        tty.wait_text("❯");
        tty.send(b"/output 0\r");
        tty.wait_text("positive number");
        tty.wait_text("❯");
        tty.send(b"/output 999\r");
        tty.wait_text("No saved tool output matches");
        tty.wait_text("❯");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        assert!(shown.contains("Saved output: /output list"));
        let views = support::screen::snapshots(&format!("{shown}\x1b[?2026l"));
        let visible = views.last().unwrap().split_whitespace().collect::<String>();
        assert!(visible.contains("SAVED_LINE_29_payload'")); // command survived wrapping and the former 400-character cap
        assert!(!shown.contains("\n      exit 0"));
        assert_eq!(f.captured().len(), 2);
        let session = fs::read_dir(f.root.join("app/sessions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let history = fs::read_to_string(session).unwrap();
        assert!(!history.contains("/output") && !history.contains("Waiting for response"));

        let mut process = f.command();
        process.args(["--cli", "--continue"]);
        let mut tty = Pty::spawn_with_color(&mut process, color);
        tty.wait_text("OUTPUT_READY");
        tty.wait_text("❯");
        tty.send(b"/output 3\r");
        tty.wait_text("Saved output #3");
        tty.wait_text("      SAVED_LINE_15_payload");
        tty.wait_text("❯");
        tty.send(b"/new\r");
        tty.wait_text("❯");
        tty.send(b"/output\r");
        tty.wait_text("No saved tool output matches");
        tty.wait_text("❯");
        tty.send(b"/quit\r");
        tty.finish();
        assert_eq!(f.captured().len(), 2);
        assert_eq!(
            fs::read_to_string(f.root.join("saved.txt")).unwrap(),
            "new\n"
        );
    }
}

#[test]
fn cli_waiting_row_is_static_and_clears_when_a_run_is_cancelled() {
    let f = Fixture::new("responses", vec![(299, ": stream open\n\n".into())]);
    let mut process = f.command();
    process.arg("--cli");
    let mut tty = Pty::spawn_with_color(&mut process, true);
    tty.wait_text("/tui");
    tty.send(b"wait for the provider\r");
    tty.wait_text("Waiting for response · Esc to stop");
    let deadline = Instant::now() + Duration::from_secs(3);
    while f.captured().is_empty() && Instant::now() < deadline {
        tty.pump();
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(f.captured().len(), 1);
    tty.send(b"\x1b");
    tty.wait_text("Stopped");
    tty.wait_text("❯");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert_eq!(shown.matches("Waiting for response").count(), 1);
    let tail = shown.split("Waiting for response").nth(1).unwrap();
    assert!(tail.find("\r\x1b[2K").unwrap() < tail.find("Stopped").unwrap());
}

#[test]
fn cli_tool_rows_show_real_success_failure_and_commands_without_ansi_when_disabled() {
    for color in [true, false] {
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![
                    ("write", json!({"path":"one.txt","content":"one\n"})),
                    ("read", json!({"path":"one.txt"})),
                    ("read", json!({"path":"one.txt"})),
                    (
                        "bash",
                        json!({"command":"printf 'ACTUAL_FAILURE\\n============\\n\\n\\nFAILURE_CONTEXT\\n' >&2; exit 3"}),
                    ),
                    ("bash", json!({"command":"printf 'ACTUAL_SUCCESS\\n'"})),
                ]),
                response("The failed command returned 3; the next command succeeded. ROWS_DONE"),
            ],
        );
        let mut command = f.command();
        command.arg("--cli");
        let mut tty = Pty::spawn_with_color(&mut command, color);
        tty.wait_text(" · CLI");
        tty.send(b"run the local tools\r");
        tty.wait_text("ROWS_DONE");
        tty.wait_text("❯");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        assert!(shown.contains("Read  one.txt, one.txt"));
        assert!(shown.contains("printf 'ACTUAL_FAILURE"));
        assert!(shown.contains("      ACTUAL_FAILURE\r\n"));
        assert!(shown.contains("      ACTUAL_SUCCESS\r\n"));
        assert!(shown.contains("      FAILURE_CONTEXT\r\n"));
        assert!(!shown.contains("      ============\r\n"));
        assert!(shown.contains("Shell exited with exit status: 3"));
        assert!(shown.contains("exit 0 · ") && shown.contains("exit 3 · "));
        assert!(!shown.contains("done ·") && !shown.contains("↳"));
        assert_eq!(
            shown.matches("hibi\n").count()
                + shown.matches("hibi\r\n").count()
                + shown.matches("hibi\x1b[0m").count(),
            1
        );
        if color {
            assert!(shown.contains("\x1b[48;2;24;57;37m"));
            assert!(shown.contains("\x1b[48;2;72;28;34m"));
        } else {
            assert!(!shown
                .replace("\x1b[?2004h", "")
                .replace("\x1b[?2004l", "")
                .contains('\x1b'));
        }
        let request = f.captured();
        assert_eq!(request.len(), 2);
        let outputs = request[1]["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .collect::<Vec<_>>();
        assert_eq!(outputs.len(), 5);
        assert!(outputs[3]["output"]
            .as_str()
            .unwrap()
            .contains("ACTUAL_FAILURE"));
        assert!(outputs[3]["output"]
            .as_str()
            .unwrap()
            .contains("============\n\n\nFAILURE_CONTEXT"));
        let session = fs::read_dir(f.root.join("app/sessions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let records = fs::read_to_string(session)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let failure = records
            .iter()
            .find(|record| record["message"]["isError"] == true)
            .unwrap();
        assert_eq!(failure["message"]["details"]["exitCode"], 3);
    }
}

#[test]
fn cli_streamed_and_edit_diffs_use_red_green_and_respect_no_color() {
    let text = "Here is the patch:\n```diff\ndiff --git a/demo.txt b/demo.txt\n--- a/demo.txt\n+++ b/demo.txt\n@@ -1 +1 @@\n-old\n+new\n```\nPATCH_DONE";
    let mut body = text.as_bytes().chunks(3).map(|part| sse(json!({"type":"response.output_text.delta","delta":std::str::from_utf8(part).unwrap()}))).collect::<String>();
    body.push_str(&sse(json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}]}})));
    for color in [true, false] {
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![
                    ("write", json!({"path":"demo.txt","content":"old\n"})),
                    (
                        "edit",
                        json!({"path":"demo.txt","oldText":"old","newText":"new"}),
                    ),
                ]),
                (200, body.clone()),
            ],
        );
        let mut command = f.command();
        command.arg("--cli");
        let mut tty = Pty::spawn_with_color(&mut command, color);
        tty.wait_text(" · CLI");
        tty.send(b"apply the patch\r");
        tty.wait_text("PATCH_DONE");
        tty.send(b"/quit\r");
        let shown = tty.finish();
        if color {
            assert!(shown.contains("\x1b[48;2;18;18;18m"));
            assert!(shown.contains("\x1b[38;2;110;210;135m+new"));
            assert!(shown.contains("\x1b[38;2;155;155;155m   1 \x1b[38;2;110;210;135m+new"));
            assert!(shown.contains("\x1b[38;2;245;110;115m-old"));
            assert!(shown.contains("\x1b[38;2;155;155;155m   1 \x1b[38;2;245;110;115m-old"));
        } else {
            assert!(!shown
                .replace("\x1b[?2004h", "")
                .replace("\x1b[?2004l", "")
                .contains('\x1b'));
        }
        assert_eq!(
            fs::read_to_string(f.root.join("demo.txt")).unwrap(),
            "new\n"
        );
        let session = fs::read_dir(f.root.join("app/sessions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(!fs::read_to_string(session).unwrap().contains("\\u001b"));
    }
}

#[test]
fn welcome_disappears_after_chat_and_is_never_saved_as_history() {
    let f = Fixture::new("responses", vec![response("WELCOME_CHAT_COMPLETE")]);
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("Enter to send · Esc to stop");
    tty.send(b"hello\r");
    tty.wait_text("WELCOME_CHAT_COMPLETE");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let views = support::screen::snapshots(&shown);
    assert!(!views
        .last()
        .unwrap()
        .contains("Enter to send · Esc to stop"));
    for file in fs::read_dir(f.root.join("app/sessions")).unwrap() {
        let history = fs::read_to_string(file.unwrap().path()).unwrap();
        assert!(history.contains("WELCOME_CHAT_COMPLETE"));
        assert!(!history.contains("Enter to send · Esc to stop"));
        assert!(!history.contains("Sign in /login"));
    }
}

#[test]
fn browser_login_menu_cancels_locally_without_starting_auth() {
    let f = Fixture::new("responses", vec![]);
    let mut command = f.command();
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"/login\r");
    tty.wait_text("Sign in to Hibiscus");
    tty.send(b"\r");
    tty.wait_text("Codex · ChatGPT account");
    tty.send(b"\x1b");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let _ = tty.finish();
    assert!(!f.root.join("app/codex-auth.json").exists());
    assert!(f.captured().is_empty());
}

#[test]
fn escape_cancels_an_http_stream_and_chat_remains_usable() {
    let slow = sse(json!({"type":"response.output_text.delta","delta":"WAITING_FOR_CANCEL"}));
    let f = Fixture::new("responses", vec![(299, slow), response("AFTER_CANCEL")]);
    let mut c = f.command();
    let mut tty = Pty::spawn(&mut c);
    tty.wait_text("Enter send");
    tty.send(b"hello\r");
    tty.wait_text("WAITING_FOR_CANCEL");
    let start = Instant::now();
    tty.send(b"\x1b");
    tty.wait_text("Stopped.");
    assert!(start.elapsed() < Duration::from_secs(3));
    tty.wait_text("Enter send");
    tty.send(b"after\r");
    tty.wait_text("AFTER_CANCEL");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(f.captured().len(), 2);
}
#[test]
fn escape_kills_a_running_shell_before_resuming_chat() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![(
                "bash",
                json!({"command":"printf '%s' $$ > shell-pid; sleep 30; printf bad > late-file"}),
            )]),
            response("AFTER_SHELL_CANCEL"),
        ],
    );
    let mut c = f.command();
    let mut tty = Pty::spawn(&mut c);
    tty.wait_text("Enter send");
    tty.send(b"run shell\r");
    let deadline = Instant::now() + Duration::from_secs(5);
    let pid: i32 = loop {
        // Keep draining the PTY while waiting for a tool side effect. A full
        // terminal buffer can block prompt submission before the shell starts.
        tty.pump();
        if let Ok(text) = fs::read_to_string(f.root.join("shell-pid")) {
            if let Ok(pid) = text.parse() {
                break pid;
            }
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    };
    tty.send(b"\x1b");
    tty.wait_text("Stopped.");
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert!(!f.root.join("late-file").exists());
    tty.wait_text("Enter send");
    tty.send(b"after\r");
    tty.wait_text("AFTER_SHELL_CANCEL");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert!(f.captured()[1]["input"].to_string().contains("cancelled"));
}
#[test]
fn queued_followup_is_delivered_once_before_settlement() {
    let (_, slow) = response("FIRST_STREAM");
    let f = Fixture::new("responses", vec![(298, slow), response("FOLLOWUP_DONE")]);
    let mut c = f.command();
    let mut tty = Pty::spawn(&mut c);
    tty.wait_text("Enter send");
    tty.send(b"first\r");
    tty.wait_text("FIRST_STREAM");
    tty.send(b"follow up\x11");
    tty.wait_text("FOLLOWUP_DONE");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    let r = f.captured();
    assert_eq!(r.len(), 2);
    assert_eq!(
        r[1]["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["role"] == "user" && v["content"].to_string().contains("follow up"))
            .count(),
        1
    );
}
#[test]
fn shell_runs_in_both_views_and_ignores_obsolete_settings() {
    for mode in ["--tui", "--cli"] {
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![
                    ("bash", json!({"command":"printf first > first-file"})),
                    ("bash", json!({"command":"printf second > second-file"})),
                ]),
                response("SHELLS_DONE"),
            ],
        );
        let settings = f.root.join("app/approval.json");
        fs::write(&settings, "obsolete settings are not parsed").unwrap();
        let mut command = f.command();
        command.arg(mode);
        let mut tty = Pty::spawn(&mut command);
        tty.wait_text(if mode == "--tui" {
            "Enter send"
        } else {
            " · CLI"
        });
        tty.send(b"run both shell commands\r");
        tty.wait_text("SHELLS_DONE");
        tty.wait_text(if mode == "--tui" {
            "Enter send"
        } else {
            "❯ "
        });
        tty.send(b"/quit\r");
        let shown = tty.finish();
        assert!(!shown.contains("Approval") && !shown.contains("Allow once"));
        assert_eq!(
            fs::read_to_string(f.root.join("first-file")).unwrap(),
            "first"
        );
        assert_eq!(
            fs::read_to_string(f.root.join("second-file")).unwrap(),
            "second"
        );
        assert_eq!(
            fs::read_to_string(settings).unwrap(),
            "obsolete settings are not parsed"
        );
        assert_eq!(f.captured().len(), 2);
    }
}

#[test]
fn automatic_compaction_preserves_the_new_prompt_and_archives_prior_history() {
    let (_, old) = response("ORIGINAL_AUTO");
    let old = old.replace("\"input_tokens\":20", "\"input_tokens\":90");
    let f = Fixture::new(
        "responses",
        vec![
            (200, old),
            response("AUTO_SUMMARY"),
            response("AUTO_CONTINUED"),
        ],
    );
    let path = f.root.join("app/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["providers"][0]["models"][0]["contextWindow"] = json!(100);
    config["providers"][0]["models"][0]["maxTokens"] = json!(8);
    fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    good(&f.run("first\nEXACT_NEW_REQUEST\n/quit\n", &[]));
    let r = f.captured();
    assert_eq!(r.len(), 3);
    assert!(!r[1]["input"].to_string().contains("EXACT_NEW_REQUEST"));
    assert!(r[2]["input"].to_string().contains("AUTO_SUMMARY"));
    assert!(r[2]["input"].to_string().contains("EXACT_NEW_REQUEST"));
}
#[test]
fn interrupted_tool_calls_are_marked_unknown_without_reexecution() {
    let f = Fixture::new("responses", vec![response("RESUMED_UNKNOWN")]);
    let cwd = f.root.to_str().unwrap();
    let sessions = f.root.join("app/sessions");
    fs::create_dir(&sessions).unwrap();
    let path = sessions.join("interrupted.jsonl");
    let header = json!({"type":"session","format":"hibiscus","version":1,"id":"interrupted","cwd":cwd,"model":["test","model"],"thinking":"off","steps":[],"usage":{"input":0,"output":0,"cacheRead":0},"context":null});
    let assistant = json!({"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"interrupted-call","name":"write","arguments":{"path":"must-not-exist","content":"do not replay"}}],"stopReason":"stop"}});
    fs::write(&path, format!("{header}\n{assistant}\n")).unwrap();
    good(&f.run("continue\n/quit\n", &["--continue"]));
    assert!(!f.root.join("must-not-exist").exists());
    let request = &f.captured()[0];
    assert!(request["input"].to_string().contains("outcome is unknown"));
    assert!(request["input"].to_string().contains("interrupted-call"));
}
#[test]
fn project_instructions_and_skill_catalog_enter_native_prompt() {
    let f = Fixture::new("responses", vec![response("POLICY_READ")]);
    fs::write(f.root.join("AGENTS.md"), "PROJECT_RULE_FOR_TEST").unwrap();
    fs::create_dir_all(f.root.join(".agents/skills/example")).unwrap();
    fs::write(
        f.root.join(".agents/skills/example/SKILL.md"),
        "---\nname: example\n---\nDo a focused check.\n",
    )
    .unwrap();
    good(&f.run("", &["hello"]));
    let r = f.captured();
    let system = r[0]["instructions"].as_str().unwrap();
    assert!(system.contains("PROJECT_RULE_FOR_TEST"));
    assert!(system.contains("Available project skill: example"));
    assert!(system.contains("Writing guidance"));
}
#[test]
fn legacy_configuration_is_rejected_with_migration_guidance() {
    let f = Fixture::new("responses", vec![]);
    fs::write(
        f.root.join("app/config.json"),
        "{\"codexContextWindow\":1000000}",
    )
    .unwrap();
    let o = f.run("", &["hello"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("Legacy config.json"));
    assert!(f.captured().is_empty());
}
#[test]
fn malformed_tool_arguments_fail_before_any_execution() {
    let bad = sse(
        json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","call_id":"bad","name":"write","arguments":"{\"path\":\"no-write\","}],"usage":{"input_tokens":2,"output_tokens":2}}}),
    );
    let f = Fixture::new("responses", vec![(200, bad)]);
    let o = f.run("", &["hello"]);
    assert!(!o.status.success());
    assert!(!f.root.join("no-write").exists());
}

#[test]
fn failed_run_returns_undelivered_queued_input_for_review() {
    let failed = format!(
        "{}{}",
        sse(json!({"type":"response.output_text.delta","delta":"WAIT_FOR_FAILURE"})),
        sse(json!({"type":"response.failed","response":{"status":"failed"}}))
    );
    let f = Fixture::new("responses", vec![(298, failed)]);
    let mut c = f.command();
    let mut tty = Pty::spawn(&mut c);
    tty.wait_text("Enter send");
    tty.send(b"initial\r");
    tty.wait_text("WAIT_FOR_FAILURE");
    tty.send(b"native queued text\x11");
    tty.wait_text("rejected queued");
    tty.wait_text("native queued text");
    tty.send(b"\x03/quit\r");
    tty.finish();
    assert_eq!(f.captured().len(), 1);
}

#[test]
fn authoritative_completion_does_not_wait_for_http_socket_close() {
    let (_, body) = response("AUTHORITATIVE_DONE");
    let f = Fixture::new("responses", vec![(299, body)]);
    let start = Instant::now();
    let o = f.run("", &["hello"]);
    good(&o);
    assert!(String::from_utf8_lossy(&o.stdout).contains("AUTHORITATIVE_DONE"));
    assert!(start.elapsed() < Duration::from_secs(3));
}
