mod support;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use support::Pty;

struct Fixture {
    root: PathBuf,
    binary: PathBuf,
    archive: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hibiscus-update-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("install")).unwrap();
        fs::create_dir_all(root.join("package")).unwrap();
        let root = root.canonicalize().unwrap();
        let binary = root.join("install/hibiscus");
        fs::copy(env!("CARGO_BIN_EXE_hibiscus"), &binary).unwrap();
        let archive = format!(
            "hibiscus-{}-{}.tar.gz",
            match std::env::consts::ARCH {
                "aarch64" => "aarch64",
                _ => "x86_64",
            },
            if cfg!(target_os = "macos") {
                "apple-darwin"
            } else {
                "unknown-linux-musl"
            }
        );
        let replacement = root.join("package/hibiscus");
        fs::write(&replacement, "#!/bin/sh\necho updated-binary\n").unwrap();
        fs::set_permissions(&replacement, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Command::new("tar")
            .args([
                "-C",
                root.join("package").to_str().unwrap(),
                "-czf",
                root.join(&archive).to_str().unwrap(),
                "hibiscus"
            ])
            .status()
            .unwrap()
            .success());
        let hash = Command::new("shasum")
            .args(["-a", "256", root.join(&archive).to_str().unwrap()])
            .output()
            .unwrap();
        assert!(hash.status.success());
        let digest = String::from_utf8(hash.stdout).unwrap();
        fs::write(
            root.join("SHA256SUMS"),
            format!("{}  {archive}\n", digest.split_whitespace().next().unwrap()),
        )
        .unwrap();
        let curl = root.join("bin/curl");
        fs::write(
            &curl,
            r#"#!/bin/sh
[ -z "$HIBISCUS_TEST_OFFLINE" ] || exit 22
url=
destination=
while [ "$#" -gt 0 ]; do
 case "$1" in
  -o) destination=$2; shift 2 ;;
  https:*) url=$1; shift ;;
  *) shift ;;
 esac
done
printf '%s\n' "$url" >> "$HIBISCUS_TEST_LOG"
case "$url" in
 */releases/latest)
  [ -z "$HIBISCUS_TEST_DELAY" ] || sleep 1
  printf '{"tag_name":"v0.2.0"}\n' ;;
 */releases/download/v0.2.0/*)
  case "$url" in
   */SHA256SUMS) cp "$HIBISCUS_TEST_ROOT/SHA256SUMS" "$destination" ;;
   *) cp "$HIBISCUS_TEST_ROOT/${url##*/}" "$destination" ;;
  esac ;;
 *) exit 23 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(curl, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            root,
            binary,
            archive,
        }
    }

    fn chat_command(&self, pi: &Path) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .env("HIBISCUS_PI", pi)
            .env("HIBISCUS_INSTALL_DIR", self.root.join("install"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("HIBISCUS_TEST_ROOT", &self.root)
            .env("HIBISCUS_TEST_LOG", self.root.join("downloads"))
            .env("HIBISCUS_TEST_OFFLINE", "")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.root.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            );
        command
    }

    fn run(&self, offline: bool) -> Output {
        Command::new(&self.binary)
            .arg("update")
            .env("HIBISCUS_INSTALL_DIR", self.root.join("install"))
            .env("HIBISCUS_TEST_ROOT", &self.root)
            .env("HIBISCUS_TEST_LOG", self.root.join("downloads"))
            .env("HIBISCUS_TEST_OFFLINE", if offline { "1" } else { "" })
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.root.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn update_downloads_pinned_release_and_atomically_replaces_installed_binary() {
    let fixture = Fixture::new();
    let output = fixture.run(false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Restart Hibiscus to use v0.2.0"));
    assert_eq!(
        fs::read_to_string(&fixture.binary).unwrap(),
        "#!/bin/sh\necho updated-binary\n"
    );
    let urls = fs::read_to_string(fixture.root.join("downloads")).unwrap();
    assert!(urls.contains("api.github.com/repos/Rekabytes-Enterprise/hibiscus/releases/latest"));
    assert!(urls.contains(&format!("/releases/download/v0.2.0/{}", fixture.archive)));
    assert!(urls.contains("/releases/download/v0.2.0/SHA256SUMS"));
}

#[test]
fn invalid_checksum_and_offline_fail_without_replacing_the_binary() {
    let fixture = Fixture::new();
    let original = fs::read(&fixture.binary).unwrap();
    assert!(!fixture.run(true).status.success());
    assert_eq!(fs::read(&fixture.binary).unwrap(), original);
    fs::write(
        fixture.root.join("SHA256SUMS"),
        format!("{}  {}\n", "0".repeat(64), fixture.archive),
    )
    .unwrap();
    let output = fixture.run(false);
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum verification failed"));
    assert_eq!(fs::read(&fixture.binary).unwrap(), original);
    assert!(Command::new(&fixture.binary)
        .arg("--version")
        .output()
        .unwrap()
        .status
        .success());
}

#[test]
fn startup_picker_updates_only_with_explicit_choice_and_keeps_chat_alive() {
    let fixture = Fixture::new();
    let pi = fixture.root.join("mock-pi");
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{"sessionId":"test"}}\n' "$id" ;;
  *) exit 12 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = fixture.chat_command(&pi);
    command.env("HIBISCUS_TEST_DELAY", "1"); // Arrives after the idle composer opens.
    let original = fs::read(&fixture.binary).unwrap();
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Hibiscus v0.2.0 is available");
    tty.send(b"\r"); // Default choice is Later, not a silent install.
    tty.send(b"/quit\r");
    tty.finish();
    drop(tty);
    assert_eq!(fs::read(&fixture.binary).unwrap(), original);
    // Expire the once-per-day notice to exercise an explicit Update now.
    fs::remove_file(fixture.root.join("cache/hibiscus/update-check")).unwrap();
    let mut command = fixture.chat_command(&pi);
    command.env("HIBISCUS_TEST_DELAY", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Hibiscus v0.2.0 is available");
    tty.send(b"\x1b[B\r");
    tty.wait_text("Updated to v0.2.0. Restart Hibiscus to use it.");
    tty.send(b"/quit\r");
    tty.finish();
    assert_eq!(
        fs::read_to_string(&fixture.binary).unwrap(),
        "#!/bin/sh\necho updated-binary\n"
    );
}

#[test]
fn source_build_refuses_self_update_without_network() {
    let output = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
        .arg("update")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("self-update is only available"));
}
