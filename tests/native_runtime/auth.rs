use super::*;
use std::os::unix::{
    ffi::OsStrExt,
    fs::{symlink, PermissionsExt},
};

#[test]
fn invalid_api_key_stores_fail_without_contacting_a_provider_or_changing_the_store() {
    for kind in [
        "public",
        "directory",
        "symlink",
        "malformed",
        "wrong_shape",
        "oversized",
        "fifo",
    ] {
        let f = Fixture::new("responses", vec![]);
        let config_path = f.root.join("app/config.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        // Exercise the saved-store boundary, independent of inherited keys.
        config["providers"][0]["apiKeyEnv"] = Value::Null;
        fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let auth = f.root.join("app/auth.json");
        let target = f.root.join("private-target");
        let secret = b"SYNTHETIC-PRIVATE-STORE-CONTENTS";
        let mut content = secret.to_vec();
        match kind {
            "directory" => fs::create_dir(&auth).unwrap(),
            "fifo" => {
                let path = std::ffi::CString::new(auth.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            }
            "symlink" => {
                fs::write(&target, secret).unwrap();
                fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
                symlink(&target, &auth).unwrap();
            }
            "public" | "malformed" | "wrong_shape" | "oversized" => {
                if kind == "wrong_shape" {
                    content = serde_json::to_vec(std::str::from_utf8(secret).unwrap()).unwrap();
                } else if kind == "oversized" {
                    content = br#"{"test":"SYNTHETIC"}"#.to_vec();
                    content.resize(128 * 1024 + 1, b' ');
                }
                fs::write(&auth, &content).unwrap();
                fs::set_permissions(
                    &auth,
                    fs::Permissions::from_mode(if kind == "public" { 0o644 } else { 0o600 }),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before = fs::symlink_metadata(&auth).unwrap();
        let result = f.run("", &["hello"]);
        assert!(!result.status.success(), "{kind}");
        let shown = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!shown.contains(std::str::from_utf8(secret).unwrap()));
        assert!(f.captured().is_empty(), "{kind}");
        let after = fs::symlink_metadata(&auth).unwrap();
        assert_eq!(before.file_type(), after.file_type());
        assert_eq!(before.permissions().mode(), after.permissions().mode());
        if kind == "symlink" {
            assert_eq!(fs::read_link(&auth).unwrap(), target);
            assert_eq!(fs::read(&target).unwrap(), secret);
        } else if !matches!(kind, "directory" | "fifo") {
            assert_eq!(fs::read(&auth).unwrap(), content);
        }
    }
}

#[test]
fn malformed_stored_credentials_are_redacted_in_local_logout_errors() {
    let f = Fixture::new("responses", vec![]);
    let auth = f.root.join("app/auth.json");
    let bytes = serde_json::to_vec("SYNTHETIC-PRIVATE-CREDENTIAL").unwrap();
    fs::write(&auth, &bytes).unwrap();
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o600)).unwrap();
    let mut tty = Pty::spawn(&mut f.command());
    tty.wait_text("Enter send");
    tty.send(b"/logout\r");
    tty.wait_text("Invalid auth.json");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("SYNTHETIC-PRIVATE-CREDENTIAL"));
    assert_eq!(fs::read(&auth).unwrap(), bytes);
    assert!(f.captured().is_empty());
}

#[test]
fn api_key_login_rejects_blank_input_without_saving_credentials() {
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
    tty.send(b"   \r");
    tty.wait_text("API key must be nonempty");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(!shown.contains("SYNTHETIC"));
    assert!(!f.root.join("app/auth.json").exists());
    assert!(f.captured().is_empty());
}
