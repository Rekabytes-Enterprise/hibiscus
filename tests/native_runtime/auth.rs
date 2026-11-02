use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};

#[test]
fn invalid_api_key_stores_fail_without_contacting_a_provider_or_changing_the_store() {
    for kind in ["public", "directory", "symlink", "malformed"] {
        let f = Fixture::new("responses", vec![]);
        let auth = f.root.join("app/auth.json");
        let target = f.root.join("private-target");
        let secret = b"SYNTHETIC-PRIVATE-STORE-CONTENTS";
        match kind {
            "directory" => fs::create_dir(&auth).unwrap(),
            "symlink" => {
                fs::write(&target, secret).unwrap();
                symlink(&target, &auth).unwrap();
            }
            "public" | "malformed" => {
                fs::write(&auth, secret).unwrap();
                fs::set_permissions(
                    &auth,
                    fs::Permissions::from_mode(if kind == "public" { 0o644 } else { 0o600 }),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before = fs::symlink_metadata(&auth).unwrap();
        let result = f
            .command()
            .env_remove("HIBISCUS_TEST_NATIVE_KEY")
            .arg("hello")
            .output()
            .unwrap();
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
        } else if kind != "directory" {
            assert_eq!(fs::read(&auth).unwrap(), secret);
        }
    }
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
