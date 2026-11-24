use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn legacy_configuration_starts_both_frontends_without_rewriting_user_data() {
    for window in ["1000000", "1050000", "null"] {
        for mode in ["--tui", "--cli"] {
            let f = Fixture::new("responses", vec![]);
            let config = format!("{{\"codexContextWindow\":{window}}}\n");
            let path = f.root.join("app/config.json");
            fs::write(&path, &config).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            let auth = f.root.join("app/auth.json");
            let credentials = b"{\"unused\":\"SYNTHETIC-SAVED-KEY\"}";
            fs::write(&auth, credentials).unwrap();
            fs::set_permissions(&auth, fs::Permissions::from_mode(0o600)).unwrap();
            let mut command = f.command();
            // Select a built-in model without sending a prompt, so this exact
            // legacy file exercises startup without public catalog traffic.
            command.arg(mode).env("HIBISCUS_MODEL", "openai/gpt-4.1");
            let mut tty = Pty::spawn(&mut command);
            tty.wait_text("/help");
            if mode == "--cli" {
                tty.wait_text("\x1b[?2004h");
            }
            tty.send(b"/quit\r");
            let shown = tty.finish();
            assert!(!shown.contains("Legacy config.json"), "{shown}");
            assert_eq!(fs::read(&path).unwrap(), config.as_bytes());
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(fs::read(&auth).unwrap(), credentials);
            assert!(f.headers.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn retired_context_setting_preserves_native_provider_and_saved_key() {
    let f = Fixture::new("responses", vec![response("NATIVE_CONFIG_KEPT")]);
    let path = f.root.join("app/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["codexContextWindow"] = json!(1_000_000);
    config["providers"][0]["apiKeyEnv"] = Value::Null;
    config["customSetting"] = json!({"preserved":true});
    let before = serde_json::to_vec_pretty(&config).unwrap();
    fs::write(&path, &before).unwrap();
    let auth = f.root.join("app/auth.json");
    let credentials = b"{\"test\":\"SYNTHETIC-SAVED-KEY\"}";
    fs::write(&auth, credentials).unwrap();
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o600)).unwrap();
    let output = f.run("", &["hello"]);
    good(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("NATIVE_CONFIG_KEPT"));
    let requests = f.captured();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["model"], "model");
    assert_eq!(requests[0]["max_output_tokens"], 4096);
    assert!(f.headers.lock().unwrap()[0]["headers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h[0] == "authorization" && h[1] == "Bearer SYNTHETIC-SAVED-KEY"));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&auth).unwrap(), credentials);
}

#[test]
fn retired_context_setting_does_not_mask_invalid_native_configuration() {
    for config in [
        "{\"codexContextWindow\":1000000,\"providers\":null}",
        "{\"codexContextWindow\":1000000,\"providers\":[{\"id\":\"bad\",\"api\":\"responses\",\"baseUrl\":\"http://example.com/v1\"}]}",
        "{\"codexContextWindow\":1000000,",
    ] {
        let f = Fixture::new("responses", vec![]);
        let path = f.root.join("app/config.json");
        fs::write(&path, config).unwrap();
        let output = f.run("", &["hello"]);
        assert!(!output.status.success());
        assert_eq!(fs::read(&path).unwrap(), config.as_bytes());
        assert!(f.headers.lock().unwrap().is_empty());
    }
}

#[test]
fn retired_codex_config_and_session_load_without_credential_reuse() {
    let f = Fixture::new("responses", vec![]);
    let path = f.root.join("app/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["model"] = json!("openai-codex/old-model");
    config["providers"].as_array_mut().unwrap().push(json!({
        "id":"openai-codex",
        "baseUrl":"https://chatgpt.com/backend-api/codex",
        "api":"responses",
        "authentication":"codex"
    }));
    let original = serde_json::to_vec_pretty(&config).unwrap();
    fs::write(&path, &original).unwrap();

    let credentials = f.root.join("app/codex-auth.json");
    fs::write(&credentials, b"unused retired credential fixture").unwrap();
    fs::set_permissions(&credentials, fs::Permissions::from_mode(0o600)).unwrap();

    let sessions = f.root.join("app/sessions");
    fs::create_dir(&sessions).unwrap();
    let session = sessions.join("retired.jsonl");
    let history = format!(
        "{}\n{}\n",
        json!({"type":"session", "format":"hibiscus", "version":1, "id":"retired",
            "cwd":f.root.canonicalize().unwrap(), "model":["openai-codex","old-model"], "thinking":"high"}),
        json!({"type":"message", "message":{"role":"user", "content":[{"type":"text","text":"Retired chat history"}]}}),
    );
    fs::write(&session, history.as_bytes()).unwrap();

    let mut command = f.command();
    command.arg("--continue");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Retired chat history");
    tty.wait_text("Enter send");
    tty.send(b"/models\r");
    tty.wait_text("Models · all providers");
    tty.send(b"\x1b");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("openai-codex"), "{shown}");
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::read(&session).unwrap(), history.as_bytes());
    assert_eq!(
        fs::read(&credentials).unwrap(),
        b"unused retired credential fixture"
    );
    assert!(f.captured().is_empty());
}
