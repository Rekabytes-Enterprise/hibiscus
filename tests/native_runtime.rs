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
    let o = f.run("/approval off\nrun tools\n/quit\n", &[]);
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
    assert!(data.contains("@@ line 1 @@"));
}
#[test]
fn noninteractive_shell_approval_denies_before_execution() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![(
                "bash",
                json!({"command":"printf bad > should-not-exist"}),
            )]),
            response("DENIAL_HANDLED"),
        ],
    );
    good(&f.run("", &["try shell"]));
    assert!(!f.root.join("should-not-exist").exists());
    assert!(f.captured()[1]["input"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["output"].as_str().is_some_and(|s| s.contains("denied"))));
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
    tty.wait_text("Enter send");
    tty.send(b"/login\r");
    tty.wait_text("Sign in to Hibiscus");
    tty.send(b"\x1b[B\r");
    tty.wait_text("API Key · provider");
    tty.send(b"\r");
    tty.wait_text("Provider API key");
    tty.send(b"FAKE-PRIVATE-CREDENTIAL\r");
    tty.wait_text("sign-in completed");
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
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("FAKE-PRIVATE-CREDENTIAL"));
    assert_eq!(fs::read_to_string(&auth).unwrap(), "{}");
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
    tty.send(b"/approval off\r");
    tty.wait_text("Approvals disabled");
    tty.wait_text("Enter send");
    tty.send(b"run shell\r");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !f.root.join("shell-pid").exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let pid: i32 = fs::read_to_string(f.root.join("shell-pid"))
        .unwrap()
        .parse()
        .unwrap();
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
fn always_allow_is_scoped_to_the_exact_command_in_one_run() {
    let command = "printf allowed > allowed-file";
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![("bash", json!({"command":command}))]),
            calls(vec![("bash", json!({"command":command}))]),
            response("GRANTED_DONE"),
        ],
    );
    let mut c = f.command();
    let mut tty = Pty::spawn(&mut c);
    tty.wait_text("Enter send");
    tty.send(b"run\r");
    tty.wait_text("Approval needed: bash");
    tty.send(b"\x1b[B\x1b[B\r");
    tty.wait_text("GRANTED_DONE");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(
        fs::read_to_string(f.root.join("allowed-file")).unwrap(),
        "allowed"
    );
    assert_eq!(f.captured().len(), 3);
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
