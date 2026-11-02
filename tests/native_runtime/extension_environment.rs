//! Actual subprocess checks; never mutate the parallel test runner's environment.
use super::*;
use std::path::Path;

const TOKEN: &str = "synthetic-extension-opt-in";
const PROBE: &str = r#"set -eu
for name in OPENAI_API_KEY ANTHROPIC_API_KEY HIBISCUS_TEST_NATIVE_KEY AWS_SECRET_ACCESS_KEY SSH_AUTH_SOCK PYTHONPATH NODE_OPTIONS GIT_CONFIG_COUNT https_proxy EXTENSION_UNDECLARED; do
    if /usr/bin/printenv "$name" >/dev/null; then
        printf 'Unexpected inherited variable: %s\n' "$name" >&2
        exit 41
    fi
done
test -n "$PATH"
test "$HOME" = "$HIBISCUS_WORKSPACE"
test "$LANG" = C
test "$TERM" = xterm-256color
test -f "$HIBISCUS_EXTENSION_DIR/main.sh"
command -v sh >/dev/null
case "$HIBISCUS_EXTENSION_DIR" in */app/extensions/.versions/demo/*) ;; *) exit 42 ;; esac
"#;

fn command(f: &Fixture) -> Command {
    let mut command = f.command();
    for name in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "AWS_SECRET_ACCESS_KEY",
        "SSH_AUTH_SOCK",
        "PYTHONPATH",
        "NODE_OPTIONS",
        "EXTENSION_UNDECLARED",
        "https_proxy",
    ] {
        command.env(name, "synthetic-must-not-reach-extension");
    }
    command
        .env("GIT_CONFIG_COUNT", "0")
        .env("EXTENSION_OPT_IN", TOKEN)
        .env_remove("EXTENSION_MISSING")
        .env("HIBISCUS_EXTENSION_DIR", "/synthetic-forged-context")
        .env("HIBISCUS_WORKSPACE", "/synthetic-forged-context")
        .env("LANG", "C")
        .env("TERM", "xterm-256color");
    command
}

fn program(declared: bool) -> String {
    let check = if declared {
        format!("test \"$EXTENSION_OPT_IN\" = '{TOKEN}'\n")
    } else {
        "test \"${EXTENSION_OPT_IN+x}\" != x\n".into()
    };
    format!("{PROBE}{check}{PROGRAM}")
}

fn manifest(directory: &Path) -> Value {
    serde_json::from_slice(&fs::read(directory.join("extension.json")).unwrap()).unwrap()
}

fn store_manifest(directory: &Path, value: &Value) {
    fs::write(directory.join("extension.json"), value.to_string()).unwrap();
}

#[test]
fn extension_tool_environment_is_minimal_or_explicit_for_every_provider() {
    for api in ["responses", "chat_completions", "anthropic"] {
        for declared in [false, true] {
            let f = Fixture::new(
                api,
                vec![
                    provider_tool(api, json!({"name":"test"})),
                    provider_done(api),
                ],
            );
            let directory = extension(&f, &program(declared));
            if declared {
                let mut value = manifest(&directory);
                value["inheritEnv"] = json!(["EXTENSION_OPT_IN"]);
                store_manifest(&directory, &value);
            }
            let output = command(&f)
                .arg("test extension environment")
                .output()
                .unwrap();
            good(&output);
            let captured = f.captured();
            assert_eq!(captured.len(), 2);
            assert!(captured[1].to_string().contains("EXTENSION_OUTPUT"));
            assert!(!captured[1].to_string().contains(TOKEN));
            assert!(f.root.join("extension-request.json").is_file());
        }
    }
}

#[test]
fn extension_slash_environment_is_filtered_in_cli_and_tui() {
    for cli in [false, true] {
        for declared in [false, true] {
            let f = Fixture::new("responses", vec![]);
            let directory = extension(&f, &program(declared));
            if declared {
                let mut value = manifest(&directory);
                value["inheritEnv"] = json!(["EXTENSION_OPT_IN"]);
                store_manifest(&directory, &value);
            }
            let mut command = command(&f);
            if cli {
                command.arg("--cli");
            }
            let mut tty = Pty::spawn(&mut command);
            tty.wait_text(if cli { "\x1b[?2004h" } else { "Enter send" });
            tty.send(b"/demo:greet test\r");
            tty.wait_text("EXTENSION_OUTPUT");
            tty.send(b"/quit\r");
            let shown = tty.finish();
            assert!(!shown.contains(TOKEN));
            assert!(f.captured().is_empty());
        }
    }
}

#[test]
fn missing_declared_variable_fails_without_starting_extension() {
    let f = Fixture::new(
        "responses",
        vec![
            provider_tool("responses", json!({"name":"test"})),
            provider_done("responses"),
        ],
    );
    let directory = extension(&f, PROGRAM);
    let mut value = manifest(&directory);
    value["inheritEnv"] = json!(["EXTENSION_MISSING"]);
    store_manifest(&directory, &value);
    good(&command(&f).arg("test missing variable").output().unwrap());
    let captured = f.captured();
    assert!(captured[1]
        .to_string()
        .contains("Declared extension environment variable is not set: EXTENSION_MISSING"));
    assert!(!f.root.join("extension-request.json").exists());
    assert!(!captured[1].to_string().contains(TOKEN));
}

#[test]
fn extension_validation_and_rollback_keep_the_versioned_environment() {
    let f = Fixture::new("responses", vec![]);
    let directory = extension(&f, &program(false));
    // Obtain the imported version from the actual application, not a predicted ID.
    let mut init = command(&f);
    let mut tty = Pty::spawn(&mut init);
    tty.wait_text("Enter send");
    let original = wait_version_state(&mut tty, &f, None);
    tty.send(b"/quit\r");
    tty.finish();

    let mut value = manifest(&directory);
    value["inheritEnv"] = json!(["EXTENSION_OPT_IN"]);
    value["checks"] = json!([
        {"kind":"exec","command":["/bin/sh","{extensionDir}/verify.sh"]},
        {"kind":"tool","name":"greet","arguments":{"name":"test"}},
        {"kind":"command","name":"greet","input":"test"}
    ]);
    let provider = Fixture::new(
        "responses",
        vec![
            calls(vec![(
                "update_extension",
                json!({"id":"demo","expectedVersion":original,"manifest":value,"files":[
                    {"path":"main.sh","content":program(true)},
                    {"path":"verify.sh","content":format!("{PROBE}test \"$EXTENSION_OPT_IN\" = '{TOKEN}'\nprintf exec-checked > \"$HIBISCUS_WORKSPACE/exec-checked\"")}
                ]}),
            )]),
            calls(vec![
                ("demo__greet", json!({"name":"test"})),
                ("list_extensions", json!({})),
            ]),
            response("ENVIRONMENT_UPDATED"),
        ],
    );
    fs::copy(
        provider.root.join("app/config.json"),
        f.root.join("app/config.json"),
    )
    .unwrap();
    good(&command(&f).arg("update environment").output().unwrap());
    let captured = provider.captured();
    assert!(
        f.root.join("exec-checked").exists(),
        "validation did not run: {:?}",
        captured[1]["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["type"] == "function_call_output")
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fs::read_to_string(f.root.join("exec-checked")).unwrap(),
        "exec-checked"
    );
    let state: Value = serde_json::from_slice(
        &fs::read(f.root.join("app/extensions/.versions/demo/active.json")).unwrap(),
    )
    .unwrap();
    let next = state["active"].as_str().unwrap();
    assert_ne!(next, original);
    assert_eq!(captured.len(), 3);
    assert!(captured[2].to_string().contains("EXTENSION_OUTPUT"));
    assert!(captured[2].to_string().contains("inheritEnv"));
    let catalog_output = captured[2]["input"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|row| row["type"] == "function_call_output")
        .unwrap();
    assert!(!catalog_output["output"].as_str().unwrap().contains(TOKEN));
    // The supplied script contains a synthetic expected token, but no resolved
    // environment value is added to the catalog or validation receipts.
    let store = f
        .root
        .join("app/extensions/.versions/demo/versions")
        .join(next);
    let record = fs::read_to_string(store.join("record.json")).unwrap();
    assert!(!record.contains(TOKEN));
    let retained = manifest(&store.join("files"));
    assert_eq!(retained["inheritEnv"], json!(["EXTENSION_OPT_IN"]));
    assert!(!retained.to_string().contains(TOKEN));

    let preserving = Fixture::new(
        "responses",
        vec![
            calls(vec![(
                "update_extension",
                json!({"id":"demo","expectedVersion":next,"files":[{"path":"note.txt","content":"keep the environment declaration"}]}),
            )]),
            calls(vec![("demo__greet", json!({"name":"test"}))]),
            response("ENVIRONMENT_PRESERVED"),
        ],
    );
    fs::copy(
        preserving.root.join("app/config.json"),
        f.root.join("app/config.json"),
    )
    .unwrap();
    good(
        &command(&f)
            .arg("update without replacing manifest")
            .output()
            .unwrap(),
    );
    assert!(preserving.captured()[2]
        .to_string()
        .contains("EXTENSION_OUTPUT"));
    let preserved: Value = serde_json::from_slice(
        &fs::read(f.root.join("app/extensions/.versions/demo/active.json")).unwrap(),
    )
    .unwrap();
    let preserved = preserved["active"].as_str().unwrap();
    assert_ne!(preserved, next);
    assert_eq!(
        manifest(
            &f.root
                .join("app/extensions/.versions/demo/versions")
                .join(preserved)
                .join("files")
        )["inheritEnv"],
        json!(["EXTENSION_OPT_IN"])
    );

    let mut missing = retained.clone();
    missing["inheritEnv"] = json!(["EXTENSION_MISSING"]);
    let rejected = Fixture::new(
        "responses",
        vec![
            calls(vec![(
                "update_extension",
                json!({"id":"demo","expectedVersion":preserved,"manifest":missing}),
            )]),
            response("ENVIRONMENT_REJECTED"),
        ],
    );
    fs::remove_file(f.root.join("exec-checked")).unwrap();
    fs::copy(
        rejected.root.join("app/config.json"),
        f.root.join("app/config.json"),
    )
    .unwrap();
    good(
        &command(&f)
            .arg("check missing variable failure")
            .output()
            .unwrap(),
    );
    assert!(!f.root.join("exec-checked").exists());
    let failure = &rejected.captured()[1]["input"];
    assert!(failure
        .to_string()
        .contains("Declared extension environment variable is not set: EXTENSION_MISSING"));
    let unchanged: Value = serde_json::from_slice(
        &fs::read(f.root.join("app/extensions/.versions/demo/active.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(unchanged["active"], preserved);

    let rollback = Fixture::new(
        "responses",
        vec![
            calls(vec![(
                "rollback_extension",
                json!({"id":"demo","expectedVersion":preserved,"version":original}),
            )]),
            calls(vec![("demo__greet", json!({"name":"test"}))]),
            response("ENVIRONMENT_RESTORED"),
        ],
    );
    fs::copy(
        rollback.root.join("app/config.json"),
        f.root.join("app/config.json"),
    )
    .unwrap();
    good(
        &command(&f)
            .arg("restore original environment")
            .output()
            .unwrap(),
    );
    assert!(rollback.captured()[2]
        .to_string()
        .contains("EXTENSION_OUTPUT"));
    // Original author files and their omitted declaration are untouched.
    assert!(manifest(&directory).get("inheritEnv").is_none());
    assert_eq!(
        fs::read_to_string(directory.join("main.sh")).unwrap(),
        program(false)
    );
}
