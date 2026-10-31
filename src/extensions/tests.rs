use super::*;
use std::fs;
use tokio::sync::watch;
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hibiscus-extension-{}-{}",
            std::process::id(),
            crate::runtime::config::unique()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn extension(&self, id: &str, script: &str) -> PathBuf {
        let directory = self.0.join(id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("main.sh"), script).unwrap();
        fs::write(directory.join("extension.json"),serde_json::to_vec(&json!({"apiVersion":1,"id":id,"description":"Test extension","command":["/bin/sh","{extensionDir}/main.sh"],"tools":[{"name":"greet","description":"Test tool","parameters":{"type":"object","properties":{"name":{"type":"string","minLength":1}},"required":["name"],"additionalProperties":false}}],"commands":[{"name":"greet","description":"Test command"}]})).unwrap()).unwrap();
        directory
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const REPLY: &str = r#"IFS= read -r request
id=${request#*\"id\":\"}
id=${id%%\"*}
printf '{"apiVersion":1,"id":"%s","text":"hello","data":{"count":1}}\n' "$id"
"#;
#[test]
fn discovery_validates_whole_manifests_without_running_programs() {
    let home = Home::new();
    home.extension(
        "good",
        &format!("printf ran > '{}'\n", home.0.join("ran").display()),
    );
    let bad = home.extension("bad", REPLY);
    let mut data: Value =
        serde_json::from_slice(&fs::read(bad.join("extension.json")).unwrap()).unwrap();
    data["commands"][0]["name"] = json!("bad:name");
    fs::write(bad.join("extension.json"), data.to_string()).unwrap();
    let registry = Registry::load(&home.0).unwrap();
    assert!(!home.0.join("ran").exists());
    assert!(registry.has_tool("good__greet"));
    assert!(!registry.has_tool("bad__greet"));
    assert_eq!(registry.commands().len(), 1);
    assert_eq!(registry.warnings().len(), 1);
}
#[test]
fn invalid_versions_external_schema_references_and_namespace_collisions_are_reported() {
    let home = Home::new();
    let bad = home.extension("bad", REPLY);
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(bad.join("extension.json")).unwrap()).unwrap();
    manifest["apiVersion"] = json!(2);
    fs::write(bad.join("extension.json"), manifest.to_string()).unwrap();
    assert!(!Registry::load(&home.0).unwrap().has_tool("bad__greet"));
    manifest["apiVersion"] = json!(1);
    manifest["tools"][0]["parameters"]["$ref"] =
        json!("file:///this-file-must-not-be-resolved.json");
    fs::write(bad.join("extension.json"), manifest.to_string()).unwrap();
    let registry = Registry::load(&home.0).unwrap();
    assert_eq!(registry.definitions().count(), 0);
    assert_eq!(registry.warnings().len(), 1);
    fs::remove_dir_all(&bad).unwrap();
    let first = home.extension("a", REPLY);
    let second = home.extension("a__b", REPLY);
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(first.join("extension.json")).unwrap()).unwrap();
    manifest["tools"][0]["name"] = json!("b__greet");
    fs::write(first.join("extension.json"), manifest.to_string()).unwrap();
    let registry = Registry::load(&home.0).unwrap();
    assert_eq!(registry.definitions().count(), 1);
    assert!(registry.warnings().iter().any(|w| w.contains("collision")));
    assert!(second.exists());
}
#[tokio::test]
async fn schema_validation_precedes_execution_and_replies_are_correlated() {
    let home = Home::new();
    home.extension(
        "test",
        &format!("printf ran > '{}'\n{REPLY}", home.0.join("ran").display()),
    );
    let registry = Registry::load(&home.0).unwrap();
    let (_cancel, mut stop) = watch::channel(false);
    for args in [
        json!({}),
        json!({"name":3}),
        json!({"name":""}),
        json!({"name":"ok","extra":true}),
    ] {
        assert!(registry
            .execute_tool(
                "call",
                "test__greet",
                &args,
                (&home.0, "session"),
                &mut stop
            )
            .await
            .is_err());
    }
    assert!(!home.0.join("ran").exists());
    let result = registry
        .execute_tool(
            "call",
            "test__greet",
            &json!({"name":"valid"}),
            (&home.0, "session"),
            &mut stop,
        )
        .await
        .unwrap();
    assert!(home.0.join("ran").exists());
    assert_eq!(result["content"][0]["text"], "hello");
    assert_eq!(result["details"]["exitCode"], 0);
    let command = registry
        .execute_command(
            "other",
            "/test:greet",
            "literal ; input",
            (&home.0, "session"),
            &mut stop,
        )
        .await
        .unwrap();
    assert_eq!(command["text"], "hello");
}
#[tokio::test]
async fn malformed_mismatched_and_failed_extension_processes_return_errors() {
    for script in [
        "printf 'not json'",
        r#"printf '{"apiVersion":1,"id":"wrong","text":"bad"}'"#,
        "printf failure >&2; exit 7",
    ] {
        let home = Home::new();
        home.extension("test", script);
        let registry = Registry::load(&home.0).unwrap();
        let (_cancel, mut stop) = watch::channel(false);
        assert!(registry
            .execute_command("call", "test:greet", "", (&home.0, "session"), &mut stop)
            .await
            .is_err());
    }
}
#[tokio::test]
async fn extension_output_and_runtime_are_bounded() {
    let home = Home::new();
    let directory = home.extension("test", &format!("printf '%s' '{}'", "x".repeat(65537)));
    let registry = Registry::load(&home.0).unwrap();
    let (_cancel, mut stop) = watch::channel(false);
    let error = registry
        .execute_command("call", "test:greet", "", (&home.0, "session"), &mut stop)
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("64 KiB"));
    fs::write(
        directory.join("main.sh"),
        "IFS= read -r request\nsleep 10\n",
    )
    .unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(directory.join("extension.json")).unwrap()).unwrap();
    manifest["timeoutSeconds"] = json!(1);
    fs::write(directory.join("extension.json"), manifest.to_string()).unwrap();
    let registry = Registry::load(&home.0).unwrap();
    let started = std::time::Instant::now();
    let error = registry
        .execute_command("call", "test:greet", "", (&home.0, "session"), &mut stop)
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
}

fn authoring(id: &str) -> Value {
    json!({"manifest":{"apiVersion":1,"id":id,"description":"An agent extension","command":["{extensionDir}/bin/main.sh"],"tools":[{"name":"greet","description":"Greet","parameters":{"type":"object"}}],"commands":[{"name":"greet","description":"Greet locally","slashCommand":"/review"}]},"files":[{"path":"bin/main.sh","content":format!("#!/bin/sh\n{REPLY}"),"executable":true},{"path":"notes.txt","content":"agent's own implementation"}]})
}
#[tokio::test]
async fn authored_extensions_are_private_executable_and_never_overwritten() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new();
    let args = authoring("mine");
    let directory = agent::create_at(&home.0, &args, &Registry::default()).unwrap();
    for (path, mode) in [
        ("", 0o700),
        ("bin", 0o700),
        ("bin/main.sh", 0o700),
        ("notes.txt", 0o600),
        ("extension.json", 0o600),
    ] {
        assert_eq!(
            fs::metadata(directory.join(path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            mode
        );
    }
    let before = fs::read(directory.join("bin/main.sh")).unwrap();
    assert!(agent::create_at(&home.0, &args, &Registry::default()).is_err());
    assert_eq!(fs::read(directory.join("bin/main.sh")).unwrap(), before);
    let registry = Registry::load(&home.0).unwrap();
    assert!(registry.has_tool("mine__greet"));
    assert_eq!(registry.commands()[0].name, "/review");
    let (_cancel, mut stop) = watch::channel(false);
    let result = registry
        .execute_command(
            "custom-id",
            "/review",
            "test",
            (&home.0, "session"),
            &mut stop,
        )
        .await
        .unwrap();
    assert_eq!(result["text"], "hello");
}
#[test]
fn invalid_source_paths_and_manifests_are_rejected_before_any_files_are_written() {
    let home = Home::new();
    for path in [
        "../outside",
        "/absolute",
        "bin/../outside",
        "extension.json",
        "extension.json/nested",
        "",
        "./main.sh",
        "bin/./main.sh",
        "bin//main.sh",
        "bin/main.sh/",
        "line\nbreak",
    ] {
        let mut args = authoring("bad");
        args["files"] = json!([{"path":path,"content":"no side effects"}]);
        assert!(
            agent::create_at(&home.0, &args, &Registry::default()).is_err(),
            "accepted {path}"
        );
        assert!(!home.0.join("bad").exists());
    }
    for files in [
        json!([{"path":"same","content":"1"},{"path":"same","content":"2"}]),
        json!([{"path":"bin","content":"1"},{"path":"bin/main.sh","content":"2"}]),
    ] {
        let mut args = authoring("bad");
        args["files"] = files;
        assert!(agent::create_at(&home.0, &args, &Registry::default()).is_err());
        assert!(!home.0.join("bad").exists());
    }
    let mut args = authoring("bad");
    args["manifest"]["apiVersion"] = json!(2);
    assert!(agent::create_at(&home.0, &args, &Registry::default()).is_err());
    assert!(!home.0.join("bad").exists());
}
#[test]
fn custom_slash_names_cannot_replace_native_or_existing_commands() {
    let home = Home::new();
    for name in ["/help", "/thinking", "/sessions", "/output", "/quit"] {
        let mut args = authoring("bad");
        args["manifest"]["commands"][0]["slashCommand"] = json!(name);
        assert!(agent::create_at(&home.0, &args, &Registry::default()).is_err());
        assert!(!home.0.join("bad").exists());
    }
    agent::create_at(&home.0, &authoring("first"), &Registry::default()).unwrap();
    let registry = Registry::load(&home.0).unwrap();
    assert!(agent::create_at(&home.0, &authoring("second"), &registry).is_err());
    assert!(!home.0.join("second").exists());
    let mut args = authoring("second");
    args["manifest"]["commands"].as_array_mut().unwrap().push(
        json!({"name":"duplicate","description":"Same public name","slashCommand":"/review"}),
    );
    assert!(agent::create_at(&home.0, &args, &Registry::default()).is_err());
    assert!(!home.0.join("second").exists());
}
#[test]
fn custom_slash_syntax_and_agent_tool_arguments_are_validated() {
    let home = Home::new();
    for name in [
        "review",
        "/Review",
        "/two words",
        "/",
        "/bad/name",
        "/bad:name",
    ] {
        let mut args = authoring("bad");
        args["manifest"]["commands"][0]["slashCommand"] = json!(name);
        assert!(agent::create_at(&home.0, &args, &Registry::default()).is_err());
    }
    let catalog = Catalog::new(Registry::default());
    for tool in [AgentTool::List, AgentTool::Reload] {
        for args in [json!(null), json!([]), json!({"unexpected":true})] {
            assert!(catalog.execute(tool, &args).is_err());
        }
    }
}
#[test]
fn authoring_cannot_displace_loaded_extensions_at_the_directory_limit() {
    let home = Home::new();
    for n in 0..32 {
        fs::create_dir(home.0.join(format!("extension_{n}"))).unwrap();
    }
    let error = agent::create_at(&home.0, &authoring("first"), &Registry::default()).unwrap_err();
    assert!(error.to_string().contains("limit"));
    assert!(!home.0.join("first").exists());
    assert_eq!(fs::read_dir(&home.0).unwrap().count(), 32);
}
