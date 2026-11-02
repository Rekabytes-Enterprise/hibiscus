use super::*;
use std::io::Seek;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hibiscus-auth-{}-{}",
            std::process::id(),
            crate::runtime::config::unique()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("auth.json")
    }
    fn write(&self, bytes: &[u8]) {
        fs::write(self.path(), bytes).unwrap();
        fs::set_permissions(self.path(), fs::Permissions::from_mode(0o600)).unwrap();
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn oversized_valid_json_is_rejected_without_modifying_the_store() {
    let temp = Temp::new();
    let mut bytes = br#"{"test":"SYNTHETIC"}"#.to_vec();
    bytes.resize(MAX_AUTH_BYTES + 1, b' ');
    temp.write(&bytes);
    assert!(keys_at(&temp.path()).is_err());
    assert_eq!(fs::read(temp.path()).unwrap(), bytes);
}

#[test]
fn invalid_json_and_wrong_shapes_never_echo_credential_values_or_paths() {
    let temp = Temp::new();
    for bytes in [
        br#""SYNTHETIC-PRIVATE-CREDENTIAL""#.as_slice(),
        br#"{"test": "SYNTHETIC-PRIVATE-CREDENTIAL""#.as_slice(),
        br#"{"test": ["SYNTHETIC-PRIVATE-CREDENTIAL"]}"#.as_slice(),
    ] {
        temp.write(bytes);
        let error = keys_at(&temp.path()).unwrap_err().to_string();
        assert!(!error.contains("SYNTHETIC-PRIVATE-CREDENTIAL"));
        assert!(!error.contains(temp.0.to_str().unwrap()));
        assert_eq!(fs::read(temp.path()).unwrap(), bytes);
    }
}

#[test]
fn missing_private_and_exact_limit_stores_keep_the_existing_json_format() {
    let temp = Temp::new();
    assert!(keys_at(&temp.path()).unwrap().is_empty());
    let keys = HashMap::from([("test".into(), "SYNTHETIC".into())]);
    save_at(&temp.path(), &keys).unwrap();
    assert_eq!(keys_at(&temp.path()).unwrap(), keys);
    assert_eq!(
        fs::metadata(temp.path()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let mut bytes = serde_json::to_vec(&keys).unwrap();
    bytes.resize(MAX_AUTH_BYTES, b' ');
    temp.write(&bytes);
    assert_eq!(keys_at(&temp.path()).unwrap(), keys);
}

#[test]
fn path_replacement_after_open_does_not_redirect_or_change_the_checked_file() {
    let temp = Temp::new();
    temp.write(br#"{"test":"SYNTHETIC-ORIGINAL"}"#);
    let file = open_key_file(&temp.path()).unwrap().unwrap();
    let original = temp.0.join("original");
    fs::rename(temp.path(), &original).unwrap();
    let replacement = temp.0.join("replacement");
    fs::write(&replacement, br#"{"test":"SYNTHETIC-REPLACEMENT"}"#).unwrap();
    symlink(&replacement, temp.path()).unwrap();
    assert_eq!(read_keys(file).unwrap()["test"], "SYNTHETIC-ORIGINAL");
    assert!(keys_at(&temp.path()).is_err());
    assert_eq!(fs::read_link(temp.path()).unwrap(), replacement);
    assert_eq!(
        fs::read(&original).unwrap(),
        br#"{"test":"SYNTHETIC-ORIGINAL"}"#
    );
    assert_eq!(
        fs::read(&replacement).unwrap(),
        br#"{"test":"SYNTHETIC-REPLACEMENT"}"#
    );
}

#[test]
fn growth_after_metadata_validation_still_stops_at_the_read_limit() {
    let temp = Temp::new();
    temp.write(br#"{"test":"SYNTHETIC"}"#);
    let mut file = open_key_file(&temp.path()).unwrap().unwrap();
    let mut bytes = fs::read(temp.path()).unwrap();
    bytes.resize(MAX_AUTH_BYTES * 2, b' ');
    fs::write(temp.path(), &bytes).unwrap();
    assert!(read_keys(&mut file).is_err());
    assert_eq!(file.stream_position().unwrap(), MAX_AUTH_BYTES as u64 + 1);
    assert_eq!(fs::read(temp.path()).unwrap(), bytes);
}

#[test]
fn oversized_save_preserves_the_previous_store_and_creates_no_temporary_file() {
    let temp = Temp::new();
    let old = HashMap::from([("test".into(), "SYNTHETIC-ORIGINAL".into())]);
    save_at(&temp.path(), &old).unwrap();
    let before = fs::read(temp.path()).unwrap();
    let oversized = HashMap::from([("test".into(), "X".repeat(MAX_AUTH_BYTES))]);
    assert!(save_at(&temp.path(), &oversized).is_err());
    assert_eq!(fs::read(temp.path()).unwrap(), before);
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
}
