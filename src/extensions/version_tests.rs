use super::*;
use crate::extensions::{
    validation,
    versions::{Store, Validation},
};

fn catalog(home: &Home) -> Catalog {
    Catalog::new(Registry::load(&home.0).unwrap())
}
fn active(catalog: &Catalog, id: &str) -> String {
    catalog.snapshot().extensions[id].version.clone().unwrap()
}
async fn execute(catalog: &Catalog, home: &Home, tool: AgentTool, args: Value) -> Result<Value> {
    let (_sender, mut cancel) = watch::channel(false);
    catalog
        .execute(tool, &args, (&home.0, "version-tests"), &mut cancel)
        .await
}
async fn greet(registry: &Registry, home: &Home) -> String {
    let (_sender, mut cancel) = watch::channel(false);
    registry
        .execute_command("call", "/demo:greet", "", (&home.0, "test"), &mut cancel)
        .await
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .into()
}
fn patch(version: &str, script: &str) -> Value {
    json!({"id":"demo","expectedVersion":version,"files":[{"path":"main.sh","content":script}]})
}
fn manifest_at(dir: &Path) -> Value {
    serde_json::from_slice(&fs::read(dir.join("extension.json")).unwrap()).unwrap()
}

#[tokio::test]
async fn migration_updates_and_rollback_keep_implementations_and_metadata_together() {
    let home = Home::new();
    let source = home.extension("demo", REPLY);
    fs::write(source.join("keep.txt"), "original author file").unwrap();
    let catalog = catalog(&home);
    let original = active(&catalog, "demo");
    let old_registry = catalog.snapshot();
    let old_dir = old_registry.extensions["demo"].directory.clone();
    assert_ne!(old_dir, source);
    let next_script = REPLY.replace("hello", "new version");
    let result = execute(
        &catalog,
        &home,
        AgentTool::Update,
        patch(&original, &next_script),
    )
    .await
    .unwrap();
    let next = result["details"]["activeVersion"].as_str().unwrap();
    assert_ne!(next, original);
    assert_eq!(greet(&old_registry, &home).await, "hello");
    assert_eq!(greet(&catalog.snapshot(), &home).await, "new version");
    assert_eq!(fs::read_to_string(source.join("main.sh")).unwrap(), REPLY);
    let current_dir = catalog.snapshot().extensions["demo"].directory.clone();
    assert_eq!(
        fs::read_to_string(current_dir.join("keep.txt")).unwrap(),
        "original author file"
    );
    assert_eq!(result["details"]["record"]["parent"], original);
    assert_eq!(
        result["details"]["record"]["changedFiles"],
        json!([{"path":"main.sh","kind":"modified"}])
    );
    let diff = execute(
        &catalog,
        &home,
        AgentTool::Diff,
        json!({"id":"demo","from":original,"to":next}),
    )
    .await
    .unwrap();
    assert!(diff["details"]["diff"]
        .as_str()
        .unwrap()
        .contains("+printf"));
    let restored = execute(
        &catalog,
        &home,
        AgentTool::Rollback,
        json!({"id":"demo","expectedVersion":next,"version":original}),
    )
    .await
    .unwrap();
    assert_eq!(restored["details"]["record"]["parent"], next);
    assert_eq!(restored["details"]["record"]["restoredFrom"], original);
    assert_ne!(restored["details"]["activeVersion"], original);
    assert_eq!(greet(&catalog.snapshot(), &home).await, "hello");
    let history = execute(&catalog, &home, AgentTool::History, json!({"id":"demo"}))
        .await
        .unwrap();
    assert_eq!(history["details"]["entries"].as_array().unwrap().len(), 3);
    assert!(old_dir.join("main.sh").is_file());
    // An ordinary restart must not replace the restored version with old source.
    assert_eq!(
        Registry::load_mode(&home.0, false).unwrap().extensions["demo"].version,
        Some(active(&catalog, "demo"))
    );
}
#[tokio::test]
async fn invalid_candidates_checks_and_stale_updates_preserve_the_active_version() {
    let home = Home::new();
    let source = home.extension("demo", REPLY);
    let catalog = catalog(&home);
    let original = active(&catalog, "demo");
    let mut bad = manifest_at(&source);
    bad["commands"][0]["slashCommand"] = json!("/help");
    assert!(execute(
        &catalog,
        &home,
        AgentTool::Update,
        json!({"id":"demo","expectedVersion":original,"manifest":bad})
    )
    .await
    .is_err());
    let mut checked = manifest_at(&source);
    checked["checks"] =
        json!([{"kind":"exec","command":["/bin/sh","-c","echo regression-failed >&2; exit 9"]}]);
    assert!(execute(
        &catalog,
        &home,
        AgentTool::Update,
        json!({"id":"demo","expectedVersion":original,"manifest":checked})
    )
    .await
    .unwrap_err()
    .to_string()
    .contains("regression-failed"));
    assert_eq!(active(&catalog, "demo"), original);
    assert_eq!(greet(&catalog.snapshot(), &home).await, "hello");
    let history = execute(&catalog, &home, AgentTool::History, json!({"id":"demo"}))
        .await
        .unwrap();
    assert_eq!(history["details"]["entries"].as_array().unwrap().len(), 3);
    for entry in history["details"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .take(2)
    {
        assert_eq!(entry["record"]["validation"]["status"], "failed");
    }
    let failed = history["details"]["entries"][0]["record"]["version"]
        .as_str()
        .unwrap();
    assert!(execute(
        &catalog,
        &home,
        AgentTool::Diff,
        json!({"id":"demo","from":original,"to":failed})
    )
    .await
    .unwrap()["details"]["diff"]
        .as_str()
        .unwrap()
        .contains("checks"));
    execute(
        &catalog,
        &home,
        AgentTool::Update,
        patch(&original, &REPLY.replace("hello", "second")),
    )
    .await
    .unwrap();
    assert!(
        execute(&catalog, &home, AgentTool::Update, patch(&original, REPLY))
            .await
            .unwrap_err()
            .to_string()
            .contains("expectedVersion")
    );
    assert_eq!(greet(&catalog.snapshot(), &home).await, "second");
}
#[tokio::test]
async fn protocol_checks_reject_bad_replies_and_run_in_a_disposable_copy() {
    let home = Home::new();
    let source = home.extension("demo", REPLY);
    let catalog = catalog(&home);
    let original = active(&catalog, "demo");
    let mut manifest = manifest_at(&source);
    manifest["checks"] = json!([
        {"kind":"exec","command":["/bin/sh","-c","printf cache > generated-cache"]},
        {"kind":"tool","name":"greet","arguments":{"name":"test"}},
        {"kind":"command","name":"greet","input":""}
    ]);
    let mut update = patch(
        &original,
        "printf '{\"apiVersion\":1,\"id\":\"wrong\",\"text\":\"oops\"}'",
    );
    update["manifest"] = manifest.clone();
    assert!(execute(&catalog, &home, AgentTool::Update, update)
        .await
        .is_err());
    let result = execute(
        &catalog,
        &home,
        AgentTool::Update,
        json!({"id":"demo","expectedVersion":original,"manifest":manifest}),
    )
    .await
    .unwrap();
    assert_eq!(
        result["details"]["record"]["validation"]["checks"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(!catalog.snapshot().extensions["demo"]
        .directory
        .join("generated-cache")
        .exists());
    let current = active(&catalog, "demo");
    // Updating code cannot skip the configured checks simply by omitting manifest.
    assert!(execute(
        &catalog,
        &home,
        AgentTool::Update,
        patch(&current, "echo bad-json")
    )
    .await
    .is_err());
    assert_eq!(active(&catalog, "demo"), current);
}
#[tokio::test]
async fn explicit_reload_imports_source_but_invalid_edits_do_not_break_the_active_copy() {
    let home = Home::new();
    let source = home.extension("demo", REPLY);
    let catalog = catalog(&home);
    let original = active(&catalog, "demo");
    fs::write(
        source.join("main.sh"),
        REPLY.replace("hello", "edited source"),
    )
    .unwrap();
    let startup = Registry::load_mode(&home.0, false).unwrap();
    assert_eq!(greet(&startup, &home).await, "hello");
    execute(&catalog, &home, AgentTool::Reload, json!({}))
        .await
        .unwrap();
    assert_ne!(active(&catalog, "demo"), original);
    assert_eq!(greet(&catalog.snapshot(), &home).await, "edited source");
    fs::write(source.join("extension.json"), "broken").unwrap();
    let result = execute(&catalog, &home, AgentTool::Reload, json!({}))
        .await
        .unwrap();
    assert!(
        result["details"]["catalog"]["errors"]
            .as_array()
            .unwrap()
            .len()
            == 1
    );
    assert_eq!(greet(&catalog.snapshot(), &home).await, "edited source");
}
#[tokio::test]
async fn configured_source_checks_require_an_explicit_checked_import_and_rollback_rechecks() {
    let home = Home::new();
    let source = home.extension("demo", REPLY);
    let mut manifest = manifest_at(&source);
    // Migration records deferred checks without executing code during startup.
    let marker = home.0.join("allow-check");
    manifest["checks"] = json!([{"kind":"exec","command":["/bin/sh","-c",format!("test -f '{}'", marker.display())]}]);
    fs::write(source.join("extension.json"), manifest.to_string()).unwrap();
    let catalog = catalog(&home);
    let original = active(&catalog, "demo");
    assert!(!marker.exists());
    fs::write(
        source.join("main.sh"),
        REPLY.replace("hello", "checked source"),
    )
    .unwrap();
    execute(&catalog, &home, AgentTool::Reload, json!({}))
        .await
        .unwrap();
    assert_eq!(active(&catalog, "demo"), original);
    fs::write(&marker, "allowed").unwrap();
    execute(
        &catalog,
        &home,
        AgentTool::Update,
        json!({"id":"demo","expectedVersion":original,"fromSource":true}),
    )
    .await
    .unwrap();
    let next = active(&catalog, "demo");
    fs::remove_file(marker).unwrap();
    assert!(execute(
        &catalog,
        &home,
        AgentTool::Rollback,
        json!({"id":"demo","expectedVersion":next,"version":original})
    )
    .await
    .is_err());
    assert_eq!(greet(&catalog.snapshot(), &home).await, "checked source");
}
#[tokio::test]
async fn interrupted_candidates_orphans_and_concurrent_commits_are_recoverable() {
    let home = Home::new();
    home.extension("demo", REPLY);
    let _catalog = catalog(&home);
    let store = Store::new(&home.0, "demo").unwrap();
    let (state, dir) = store.active().unwrap().unwrap();
    let mut first = store.prepare(&dir, Some(&state), None, false).unwrap();
    let mut second = store.prepare(&dir, Some(&state), None, false).unwrap();
    first
        .replace("main.sh", REPLY.replace("hello", "first").as_bytes(), false)
        .unwrap();
    first.refresh().unwrap();
    // A fresh process sees the old active version during a half-written update.
    assert_eq!(
        greet(&Registry::load_mode(&home.0, false).unwrap(), &home).await,
        "hello"
    );
    let validation = Validation::Passed { checks: vec![] };
    first.activate(validation.clone()).unwrap();
    assert!(second
        .activate(validation)
        .unwrap_err()
        .to_string()
        .contains("changed while"));
    // Simulate interruption after moving a valid candidate, before publishing it.
    let current = store.state().unwrap().unwrap();
    let (_, current_dir) = store.retained(&current.active).unwrap();
    let orphan = store
        .prepare(&current_dir, Some(&current), None, false)
        .unwrap();
    let orphan_version = orphan.record.version.clone();
    fs::rename(
        &orphan.directory,
        home.0.join(".versions/demo/versions").join(&orphan_version),
    )
    .unwrap();
    assert_eq!(store.state().unwrap().unwrap().active, current.active);
    assert_eq!(
        greet(&Registry::load_mode(&home.0, false).unwrap(), &home).await,
        "first"
    );
    let history = store.history(20).unwrap();
    assert_eq!(history["entries"].as_array().unwrap().len(), 4);
    assert!(store.retained(&orphan_version).is_err());
    assert!(second.directory.exists());
}
#[tokio::test]
async fn regression_cancellation_and_timeout_leave_active_code_and_record_failure() {
    for timeout in [false, true] {
        let home = Home::new();
        let source = home.extension("demo", REPLY);
        let catalog = catalog(&home);
        let original = active(&catalog, "demo");
        let mut manifest = manifest_at(&source);
        let marker = home.0.join("descendant-survived");
        manifest["checks"] = json!([{"kind":"exec","command":["/bin/sh","-c",format!("(sleep 2; touch '{}') & wait", marker.display())],"timeoutSeconds":if timeout {1} else {30}}]);
        let (sender, mut cancel) = watch::channel(false);
        let update = json!({"id":"demo","expectedVersion":original,"manifest":manifest});
        let change = catalog.execute(AgentTool::Update, &update, (&home.0, "test"), &mut cancel);
        let stop = async {
            if !timeout {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                sender.send(true).unwrap();
            }
        };
        let (result, _) = tokio::join!(change, stop);
        assert!(result.is_err());
        assert_eq!(active(&catalog, "demo"), original);
        tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
        assert!(!marker.exists());
        assert_eq!(
            Store::new(&home.0, "demo").unwrap().history(20).unwrap()["entries"][0]["record"]
                ["validation"]["status"],
            "failed"
        );
    }
}
#[tokio::test]
async fn nested_symlinks_traversal_missing_entrypoints_and_explicit_removal_are_validated() {
    let home = Home::new();
    let source = home.extension("demo", REPLY);
    fs::write(source.join("keep.txt"), "keep").unwrap();
    let catalog = catalog(&home);
    let original = active(&catalog, "demo");
    for path in [
        "../outside",
        "/outside",
        "extension.json",
        "main.sh/../outside",
    ] {
        let mut input = patch(&original, REPLY);
        input["files"][0]["path"] = json!(path);
        assert!(execute(&catalog, &home, AgentTool::Update, input)
            .await
            .is_err());
    }
    assert!(execute(
        &catalog,
        &home,
        AgentTool::Update,
        json!({"id":"demo","expectedVersion":original,"remove":["main.sh"]})
    )
    .await
    .is_err());
    let mut manifest = manifest_at(&source);
    manifest["command"] = json!(["hibiscus-nonexistent-runtime-29"]);
    assert!(validation::executable(
        &manifest::parse(&serde_json::to_vec(&manifest).unwrap())
            .unwrap()
            .0,
        &source
    )
    .is_err());
    std::os::unix::fs::symlink("/etc/hosts", source.join("escape")).unwrap();
    assert!(execute(
        &catalog,
        &home,
        AgentTool::Update,
        json!({"id":"demo","expectedVersion":original,"fromSource":true})
    )
    .await
    .is_err());
    fs::remove_file(source.join("escape")).unwrap();
    execute(
        &catalog,
        &home,
        AgentTool::Update,
        json!({"id":"demo","expectedVersion":original,"remove":["keep.txt"]}),
    )
    .await
    .unwrap();
    assert!(!catalog.snapshot().extensions["demo"]
        .directory
        .join("keep.txt")
        .exists());
    assert!(source.join("keep.txt").exists());
}
#[tokio::test]
async fn commit_rechecks_collisions_and_tampering_instead_of_publishing_partial_state() {
    let home = Home::new();
    home.extension("demo", REPLY);
    let catalog = catalog(&home);
    let store = Store::new(&home.0, "demo").unwrap();
    let (state, dir) = store.active().unwrap().unwrap();
    let mut candidate = store.prepare(&dir, Some(&state), None, false).unwrap();
    fs::write(candidate.files().join("main.sh"), "tampered").unwrap();
    assert!(candidate
        .activate(Validation::Passed { checks: vec![] })
        .unwrap_err()
        .to_string()
        .contains("changed during validation"));
    let mut candidate = store.prepare(&dir, Some(&state), None, false).unwrap();
    let mut manifest = manifest_at(&dir);
    manifest["commands"][0]["slashCommand"] = json!("/review");
    candidate
        .replace("extension.json", manifest.to_string().as_bytes(), false)
        .unwrap();
    candidate.refresh().unwrap();
    // Another extension registers the desired command while this candidate checks.
    agent::create_at(&home.0, &authoring("other"), &catalog.snapshot()).unwrap();
    Registry::load(&home.0).unwrap();
    assert!(candidate
        .activate(Validation::Passed { checks: vec![] })
        .is_err());
    assert_eq!(store.state().unwrap().unwrap().active, state.active);
    let lock = crate::extensions::versions::lock(&home.0).unwrap();
    assert!(crate::extensions::versions::lock(&home.0).is_err());
    drop(lock);
    assert!(crate::extensions::versions::lock(&home.0).is_ok());
}

#[tokio::test]
async fn a_running_invocation_reads_its_original_files_after_activation() {
    let home = Home::new();
    let source = home.extension("demo", &format!("{REPLY}\n"));
    let program = r#"IFS= read -r request
id=${request#*\"id\":\"}; id=${id%%\"*}
touch ready
while [ ! -f release ]; do sleep 0.01; done
text=$(cat "$HIBISCUS_EXTENSION_DIR/version.txt")
printf '{"apiVersion":1,"id":"%s","text":"%s"}' "$id" "$text"
"#;
    fs::write(source.join("main.sh"), program).unwrap();
    fs::write(source.join("version.txt"), "original").unwrap();
    let catalog = catalog(&home);
    let old = catalog.snapshot();
    let original = active(&catalog, "demo");
    let (_sender, mut cancel) = watch::channel(false);
    let running = old.execute_command("running", "/demo:greet", "", (&home.0, "test"), &mut cancel);
    let updating = async {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        while !home.0.join("ready").exists() {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        execute(&catalog, &home, AgentTool::Update, json!({"id":"demo","expectedVersion":original,"files":[{"path":"version.txt","content":"replacement"}]})).await.unwrap();
        fs::write(home.0.join("release"), "go").unwrap();
    };
    let (result, _) = tokio::time::timeout(std::time::Duration::from_secs(6), async {
        tokio::join!(running, updating)
    })
    .await
    .unwrap();
    assert_eq!(result.unwrap()["text"], "original");
    assert_eq!(greet(&catalog.snapshot(), &home).await, "replacement");
}
#[tokio::test]
async fn legacy_absolute_entrypoints_are_rebased_and_corrupt_sources_keep_active_code() {
    let home = Home::new();
    let source = home.extension("demo", REPLY);
    let mut manifest = manifest_at(&source);
    manifest["command"] = json!(["/bin/sh", source.join("main.sh")]);
    fs::write(source.join("extension.json"), manifest.to_string()).unwrap();
    let catalog = catalog(&home);
    let old = catalog.snapshot();
    let original = active(&catalog, "demo");
    fs::write(source.join("main.sh"), "exit 9").unwrap();
    assert_eq!(greet(&old, &home).await, "hello");
    for malformed in ["[]", "null", "{bad"] {
        fs::write(source.join("extension.json"), malformed).unwrap();
        execute(&catalog, &home, AgentTool::Reload, json!({}))
            .await
            .unwrap();
        assert_eq!(active(&catalog, "demo"), original);
        assert_eq!(greet(&catalog.snapshot(), &home).await, "hello");
    }
    std::os::unix::fs::symlink("/etc/hosts", source.join("bad-link")).unwrap();
    execute(&catalog, &home, AgentTool::Reload, json!({}))
        .await
        .unwrap();
    assert_eq!(active(&catalog, "demo"), original);
    let active_dir = catalog.snapshot().extensions["demo"].directory.clone();
    fs::write(active_dir.join("main.sh"), "tampered").unwrap();
    assert!(Store::new(&home.0, "demo")
        .unwrap()
        .retained(&original)
        .is_err());
    assert!(!Registry::load_mode(&home.0, false)
        .unwrap()
        .has_tool("demo__greet"));
}
