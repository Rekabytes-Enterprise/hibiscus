use super::*;
fn extension(f: &Fixture, script: &str) -> PathBuf {
    let directory = f.root.join("app/extensions/demo");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("main.sh"), script).unwrap();
    fs::write(directory.join("extension.json"),serde_json::to_vec(&json!({"apiVersion":1,"id":"demo","description":"Executable extension fixture","command":["/bin/sh","{extensionDir}/main.sh"],"timeoutSeconds":5,"tools":[{"name":"greet","description":"Test a custom tool","parameters":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"],"additionalProperties":false}}],"commands":[{"name":"greet","description":"Run a local fixture command"}]})).unwrap()).unwrap();
    directory
}
const PROGRAM: &str = r#"IFS= read -r request
printf '%s' "$request" > extension-request.json
id=${request#*\"id\":\"}
id=${id%%\"*}
printf '{"apiVersion":1,"id":"%s","text":"EXTENSION_OUTPUT","data":{"checked":true}}\n' "$id"
"#;
fn provider_tool(api: &str, args: Value) -> (u16, String) {
    match api {
        "responses" => calls(vec![("demo__greet", args)]),
        "chat_completions" => (
            200,
            format!(
                "{}{}data: [DONE]\n\n",
                sse(
                    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"tool-0","function":{"name":"demo__greet","arguments":args.to_string()}}]},"finish_reason":null}]})
                ),
                sse(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}))
            ),
        ),
        "anthropic" => (
            200,
            format!(
                "{}{}{}{}{}",
                sse(json!({"type":"message_start","message":{"usage":{"input_tokens":1}}})),
                sse(
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"tool-0","name":"demo__greet","input":{}}})
                ),
                sse(
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":args.to_string()}})
                ),
                sse(
                    json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":1}})
                ),
                sse(json!({"type":"message_stop"}))
            ),
        ),
        _ => unreachable!(),
    }
}
fn provider_done(api: &str) -> (u16, String) {
    match api {
        "responses" => response("EXTENSION_TURN_DONE"),
        "chat_completions" => (
            200,
            format!(
                "{}{}data: [DONE]\n\n",
                sse(
                    json!({"choices":[{"delta":{"content":"EXTENSION_TURN_DONE"},"finish_reason":null}]})
                ),
                sse(json!({"choices":[{"delta":{},"finish_reason":"stop"}]}))
            ),
        ),
        "anthropic" => (
            200,
            format!(
                "{}{}{}{}{}",
                sse(json!({"type":"message_start","message":{"usage":{"input_tokens":1}}})),
                sse(
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})
                ),
                sse(
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"EXTENSION_TURN_DONE"}})
                ),
                sse(
                    json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}})
                ),
                sse(json!({"type":"message_stop"}))
            ),
        ),
        _ => unreachable!(),
    }
}
#[test]
fn extension_tools_are_advertised_executed_and_persisted_for_every_provider() {
    for api in ["responses", "chat_completions", "anthropic"] {
        let f = Fixture::new(
            api,
            vec![
                provider_tool(api, json!({"name":"Raspberry"})),
                provider_done(api),
            ],
        );
        extension(&f, PROGRAM);
        let output = f.run("", &["use the extension"]);
        good(&output);
        assert!(String::from_utf8_lossy(&output.stdout).contains("EXTENSION_TURN_DONE"));
        let requests = f.captured();
        assert_eq!(requests.len(), 2);
        let tool = requests[0]["tools"].as_array().unwrap().iter().find(|t| {
            if api == "chat_completions" {
                t["function"]["name"] == "demo__greet"
            } else {
                t["name"] == "demo__greet"
            }
        });
        assert!(tool.is_some());
        for name in [
            "create_extension",
            "list_extensions",
            "reload_extensions",
            "update_extension",
            "extension_history",
            "extension_diff",
            "rollback_extension",
        ] {
            assert!(
                requests[0]["tools"].as_array().unwrap().iter().any(|tool| {
                    if api == "chat_completions" {
                        tool["function"]["name"] == name
                    } else {
                        tool["name"] == name
                    }
                }),
                "missing {name} for {api}"
            );
        }
        assert!(requests[1].to_string().contains("EXTENSION_OUTPUT"));
        let invocation: Value =
            serde_json::from_slice(&fs::read(f.root.join("extension-request.json")).unwrap())
                .unwrap();
        assert_eq!(invocation["id"], "tool-0");
        assert_eq!(invocation["kind"], "tool");
        assert_eq!(invocation["name"], "greet");
        assert_eq!(invocation["arguments"]["name"], "Raspberry");
        assert_eq!(invocation["cwd"], f.root.to_string_lossy().as_ref());
        assert!(!invocation.to_string().contains("SYNTHETIC-KEY"));
        let session = fs::read_dir(f.root.join("app/sessions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let history = fs::read_to_string(session).unwrap();
        assert!(history.contains("demo__greet"));
        assert!(history.contains("EXTENSION_OUTPUT"));
    }
}
#[test]
fn invalid_extension_arguments_never_start_the_program() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![("demo__greet", json!({"name":7}))]),
            response("INVALID_TOOL_RECOVERED"),
        ],
    );
    extension(&f, PROGRAM);
    good(&f.run("", &["call the extension"]));
    assert!(!f.root.join("extension-request.json").exists());
    assert!(f.captured()[1].to_string().contains("parameters schema"));
}
#[test]
fn incomplete_provider_stream_cannot_execute_extension_tools() {
    let reply = (
        200,
        sse(
            json!({"type":"response.output_item.done","item":{"type":"function_call","call_id":"tool-0","name":"demo__greet","arguments":"{\"name\":\"test\"}"}}),
        ),
    );
    let f = Fixture::new("responses", vec![reply]);
    extension(&f, PROGRAM);
    assert!(!f.run("", &["call the extension"]).status.success());
    assert!(!f.root.join("extension-request.json").exists());
    assert_eq!(f.captured().len(), 1);
}
#[test]
fn extension_failures_are_tool_results_and_leave_the_agent_usable() {
    for script in [
        "printf 'broken JSON'",
        "printf failure >&2; exit 7",
        r#"IFS= read -r request
id=${request#*\"id\":\"}
id=${id%%\"*}
printf '{"apiVersion":1,"id":"%s","text":"LOGICAL_FAILURE","isError":true}\n' "$id"
"#,
    ] {
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![("demo__greet", json!({"name":"test"}))]),
                response("RECOVERED"),
            ],
        );
        extension(&f, script);
        good(&f.run("", &["use the extension"]));
        let session = fs::read_dir(f.root.join("app/sessions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let history = fs::read_to_string(session).unwrap();
        assert!(history.contains("\"isError\":true"));
        assert!(history.contains("RECOVERED"));
    }
}
#[test]
fn extension_commands_remain_local_and_reload_updates_tui_completion() {
    for cli in [false, true] {
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![("reload_extensions", json!({}))]),
                response("RELOAD_DONE"),
            ],
        );
        let directory = extension(&f, PROGRAM);
        let mut command = f.command();
        if cli {
            command.arg("--cli");
        }
        let mut tty = Pty::spawn(&mut command);
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        assert!(!f.root.join("extension-request.json").exists());
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(directory.join("extension.json")).unwrap()).unwrap();
        manifest["commands"][0]["name"] = json!("run");
        manifest["commands"][0]["slashCommand"] = json!("/review");
        fs::write(directory.join("extension.json"), manifest.to_string()).unwrap();
        tty.send(b"reload the extension catalog\r");
        tty.wait_text("RELOAD_DONE");
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        if cli {
            tty.send(b"/review literal ; $input\r");
        } else {
            tty.send(b"/rev\t literal ; $input\r");
        }
        tty.wait_text("EXTENSION_OUTPUT");
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        tty.send(b"/quit\r");
        let shown = tty.finish();
        assert_eq!(f.captured().len(), 2);
        let request: Value =
            serde_json::from_slice(&fs::read(f.root.join("extension-request.json")).unwrap())
                .unwrap();
        assert_eq!(request["kind"], "command");
        assert_eq!(request["name"], "run");
        assert_eq!(request["arguments"]["input"], "literal ; $input");
        assert!(!shown.contains("\nyou › /review"));
    }
}
fn authoring_args() -> Value {
    json!({"manifest":{"apiVersion":1,"id":"made","description":"Agent-authored extension","command":["/bin/sh","{extensionDir}/main.sh"],"tools":[{"name":"greet","description":"Read a greeting","parameters":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"],"additionalProperties":false}}],"commands":[{"name":"greet","description":"A greeting command","slashCommand":"/hello"}]},"files":[{"path":"main.sh","content":PROGRAM}]})
}
#[test]
fn agent_creates_and_uses_new_extension_on_the_next_provider_step() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![("create_extension", authoring_args())]),
            calls(vec![("made__greet", json!({"name":"Raspberry"}))]),
            response("AUTHORING_DONE"),
        ],
    );
    good(&f.run("", &["Create a greeting extension and use it"]));
    let requests = f.captured();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "made__greet"));
    assert!(requests[1]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "made__greet"));
    assert!(requests[1]
        .to_string()
        .contains("Its tools and slash commands are registered"));
    assert!(requests[2].to_string().contains("EXTENSION_OUTPUT"));
    assert_eq!(
        fs::read_to_string(f.root.join("app/extensions/made/main.sh")).unwrap(),
        PROGRAM
    );
    let invocation: Value =
        serde_json::from_slice(&fs::read(f.root.join("extension-request.json")).unwrap()).unwrap();
    assert_eq!(invocation["kind"], "tool");
    assert_eq!(invocation["arguments"]["name"], "Raspberry");
}
#[test]
fn agent_created_slash_commands_are_available_in_the_same_cli_and_tui_session() {
    for cli in [false, true] {
        let f = Fixture::new(
            "responses",
            vec![
                calls(vec![("create_extension", authoring_args())]),
                response("CREATED_COMMAND"),
            ],
        );
        let mut command = f.command();
        if cli {
            command.arg("--cli");
        }
        let mut tty = Pty::spawn(&mut command);
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f.root.join("app/extensions").is_dir() {
            assert!(
                Instant::now() < deadline,
                "startup did not create extensions directory"
            );
            tty.pump();
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            fs::read_dir(f.root.join("app/extensions")).unwrap().count(),
            0
        );
        tty.send(b"create the greeting extension\r");
        tty.wait_text("CREATED_COMMAND");
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        if cli {
            tty.send(b"/hello Raspberry\r");
        } else {
            tty.send(b"/hell\t Raspberry\r");
        }
        tty.wait_text("EXTENSION_OUTPUT");
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        tty.send(b"/quit\r");
        let _ = tty.finish();
        assert_eq!(f.captured().len(), 2);
        let invocation: Value =
            serde_json::from_slice(&fs::read(f.root.join("extension-request.json")).unwrap())
                .unwrap();
        assert_eq!(invocation["kind"], "command");
        assert_eq!(invocation["arguments"]["input"], "Raspberry");
    }
}
#[test]
fn agent_can_inspect_extension_errors_without_hiding_native_tools() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![("list_extensions", json!({}))]),
            response("INSPECTED"),
        ],
    );
    let directory = extension(&f, PROGRAM);
    fs::write(directory.join("extension.json"), "{}").unwrap();
    good(&f.run("", &["inspect installed extensions"]));
    let requests = f.captured();
    assert!(requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "read"));
    assert!(requests[1].to_string().contains("Invalid extension.json"));
    assert!(!f.root.join("extension-request.json").exists());
}
#[test]
fn escaped_extension_commands_kill_descendants_and_chat_recovers_in_both_views() {
    for cli in [false, true] {
        let f = Fixture::new("responses", vec![response("AFTER_EXTENSION_CANCEL")]);
        extension(&f,"IFS= read -r request\nprintf ready > extension-ready\n(sleep 0.6; printf late > extension-late)&\nwait\n");
        let mut command = f.command();
        if cli {
            command.arg("--cli");
        }
        let mut tty = Pty::spawn(&mut command);
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        tty.send(b"/demo:greet\r");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f.root.join("extension-ready").exists() {
            assert!(Instant::now() < deadline);
            tty.pump();
            thread::sleep(Duration::from_millis(5));
        }
        tty.send(b"\x1b");
        tty.wait_text("cancelled");
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        tty.send(b"after cancel\r");
        tty.wait_text("AFTER_EXTENSION_CANCEL");
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        let deadline = Instant::now() + Duration::from_millis(700);
        while Instant::now() < deadline {
            tty.pump();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!f.root.join("extension-late").exists());
        tty.send(b"/quit\r");
        let _ = tty.finish();
        assert_eq!(f.captured().len(), 1);
    }
}

#[test]
fn invalid_extensions_and_unknown_commands_recover_without_entering_model_history() {
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![("reload_extensions", json!({}))]),
            response("RELOADED_AFTER_ERROR"),
            response("MODEL_STILL_USABLE"),
        ],
    );
    let directory = extension(&f, PROGRAM);
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(directory.join("extension.json")).unwrap()).unwrap();
    manifest["apiVersion"] = json!(2);
    fs::write(directory.join("extension.json"), manifest.to_string()).unwrap();
    let mut command = f.command();
    command.arg("--cli");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Unsupported extension apiVersion");
    tty.wait_text("\x1b[?2004h");
    assert!(!f.root.join("extension-request.json").exists());
    tty.send(b"/extensions\r");
    tty.wait_text("Unknown command");
    tty.wait_text("\x1b[?2004h");
    for name in ["/demo:greet\r", "/Demo:greet\r", "/demo:greet:extra\r"] {
        tty.send(name.as_bytes());
        tty.wait_text("Unknown extension command");
        tty.wait_text("\x1b[?2004h");
    }
    assert!(f.captured().is_empty());
    manifest["apiVersion"] = json!(1);
    fs::write(directory.join("extension.json"), manifest.to_string()).unwrap();
    tty.send(b"reload the extension\r");
    tty.wait_text("RELOADED_AFTER_ERROR");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/demo:greet\r");
    tty.wait_text("EXTENSION_OUTPUT");
    tty.wait_text("\x1b[?2004h");
    assert_eq!(f.captured().len(), 2);
    tty.send(b"hello\r");
    tty.wait_text("MODEL_STILL_USABLE");
    tty.wait_text("\x1b[?2004h");
    tty.send(b"/quit\r");
    let _ = tty.finish();
    assert_eq!(f.captured().len(), 3);
    let session = fs::read_dir(f.root.join("app/sessions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let history = fs::read_to_string(session).unwrap();
    let messages = history
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|row| row["type"] == "message")
        .map(|row| row["message"].clone())
        .collect::<Vec<_>>();
    let user_text = messages
        .iter()
        .filter(|message| message["role"] == "user")
        .map(|message| message["content"][0]["text"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(user_text, ["reload the extension", "hello"]);
    assert!(!history.contains("EXTENSION_OUTPUT"));
}

#[test]
fn agent_creation_checks_disk_collisions_after_manifest_edits_before_writing() {
    let mut manifest = authoring_args()["manifest"].clone();
    manifest["id"] = json!("demo");
    // The loaded catalog initially has /demo:greet. A native edit introduces
    // /hello before creation, without an explicit reload between the two tools.
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![
                (
                    "write",
                    json!({"path":"app/extensions/demo/extension.json","content":manifest.to_string()}),
                ),
                ("create_extension", authoring_args()),
            ]),
            response("COLLISION_RECOVERED"),
        ],
    );
    let directory = extension(&f, PROGRAM);
    good(&f.run("", &["update the command then create another extension"]));
    assert!(!f.root.join("app/extensions/made").exists());
    assert!(f.captured()[1]
        .to_string()
        .contains("collides with a native or registered command"));
    let current: Value =
        serde_json::from_slice(&fs::read(directory.join("extension.json")).unwrap()).unwrap();
    assert_eq!(current["commands"][0]["slashCommand"], "/hello");
}

fn wait_version_state(tty: &mut Pty, f: &Fixture, previous: Option<&str>) -> String {
    let path = f.root.join("app/extensions/.versions/demo/active.json");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        tty.pump();
        thread::sleep(Duration::from_millis(10));
        if let Ok(bytes) = fs::read(&path) {
            let state: Value = serde_json::from_slice(&bytes).unwrap();
            if let Some(version) = state["active"].as_str().filter(|s| Some(*s) != previous) {
                return version.into();
            }
        }
        assert!(Instant::now() < deadline, "extension activation timed out");
    }
}
#[test]
fn versioned_updates_and_rollback_refresh_tools_and_commands_in_both_views() {
    for cli in [false, true] {
        let f = Fixture::new("responses", vec![]);
        let source = extension(&f, PROGRAM);
        // Native startup imports the legacy extension without asking a provider.
        let mut init = f.command();
        let mut tty = Pty::spawn_with_size(&mut init, false, 110, 32);
        tty.wait_text("Enter send");
        let original = wait_version_state(&mut tty, &f, None);
        tty.send(b"/quit\r");
        tty.finish();
        assert!(f.captured().is_empty());
        // Configure deterministic model replies after obtaining the actual version.
        let updated = PROGRAM.replace("EXTENSION_OUTPUT", "UPDATED_OUTPUT");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(source.join("extension.json")).unwrap()).unwrap();
        manifest["commands"][0]["slashCommand"] = json!("/review");
        let f2 = Fixture::new(
            "responses",
            vec![
                calls(vec![
                    (
                        "update_extension",
                        json!({"id":"demo","expectedVersion":original,"manifest":manifest,"files":[{"path":"main.sh","content":updated}]}),
                    ),
                    // The rest of this provider response must keep the old registry.
                    ("demo__greet", json!({"name":"before"})),
                ]),
                calls(vec![
                    ("demo__greet", json!({"name":"after"})),
                    ("extension_history", json!({"id":"demo"})),
                ]),
                response("VERSION_UPDATE_DONE"),
            ],
        );
        // Use f2's provider with f's independently retained extension state.
        fs::copy(
            f2.root.join("app/config.json"),
            f.root.join("app/config.json"),
        )
        .unwrap();
        let mut command = f.command();
        if cli {
            command.arg("--cli");
        }
        let mut tty = Pty::spawn_with_size(&mut command, false, 110, 32);
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        tty.send(b"update and verify the extension\r");
        tty.wait_text("VERSION_UPDATE_DONE");
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        let next = wait_version_state(&mut tty, &f, Some(&original));
        tty.send(if cli { b"/review\r" } else { b"/rev\t\r" });
        tty.wait_text("UPDATED_OUTPUT");
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        tty.send(b"/quit\r");
        tty.finish();
        let requests = f2.captured();
        assert_eq!(requests.len(), 3);
        assert!(requests[1].to_string().contains("EXTENSION_OUTPUT"));
        assert!(requests[2].to_string().contains("UPDATED_OUTPUT"));
        assert!(requests[2].to_string().contains("activeVersion"));
        assert_eq!(fs::read_to_string(source.join("main.sh")).unwrap(), PROGRAM);
        let f3 = Fixture::new(
            "responses",
            vec![
                calls(vec![
                    (
                        "extension_diff",
                        json!({"id":"demo","from":original,"to":next}),
                    ),
                    (
                        "rollback_extension",
                        json!({"id":"demo","expectedVersion":next,"version":original}),
                    ),
                ]),
                response("ROLLBACK_DONE"),
            ],
        );
        fs::copy(
            f3.root.join("app/config.json"),
            f.root.join("app/config.json"),
        )
        .unwrap();
        let mut command = f.command();
        if cli {
            command.arg("--cli");
        }
        let mut tty = Pty::spawn_with_size(&mut command, false, 110, 32);
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        tty.send(b"inspect the diff and restore the original\r");
        tty.wait_text("ROLLBACK_DONE");
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        tty.send(b"/demo:greet\r");
        tty.wait_text("EXTENSION_OUTPUT");
        tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
        tty.send(b"/quit\r");
        tty.finish();
        let requests = f3.captured();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].to_string().contains("changedFiles"));
        assert!(requests[1].to_string().contains("restoredFrom"));
        let record_root = f.root.join("app/extensions/.versions/demo/versions");
        assert_eq!(fs::read_dir(record_root).unwrap().count(), 3);
    }
}
