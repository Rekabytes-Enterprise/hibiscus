use super::*;
use protocol::tests::{access, receipt, runtime, Server};
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
};
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("hibiscus-oauth-{}", super::super::config::unique()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> store::Store {
        store::Store::at(self.0.join("codex-auth.json"))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn token() -> Token {
    Token {
        access: access("account-a"),
        refresh: "old-refresh".into(),
        expires: 1,
        account_id: "account-a".into(),
    }
}
#[test]
fn private_store_is_atomic_and_rejects_public_symlink_and_malformed_files() {
    let temp = Temp::new();
    let store = temp.store();
    assert!(store.read().unwrap().is_none());
    store.write(&token()).unwrap();
    assert_eq!(
        fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(store.read().unwrap().unwrap().refresh, "old-refresh");
    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.read().is_err());
    fs::remove_file(store.path()).unwrap();
    let target = temp.0.join("other");
    fs::write(&target, "PRIVATE-CONTENTS").unwrap();
    symlink(&target, store.path()).unwrap();
    assert!(store.read().is_err());
    fs::remove_file(store.path()).unwrap();
    fs::write(store.path(), "PRIVATE-INVALID-JSON").unwrap();
    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!store.read().err().unwrap().to_string().contains("PRIVATE"));
}
#[test]
fn native_refresh_rotates_and_serializes_credentials() {
    let temp = Temp::new();
    let store = temp.store();
    store.write(&token()).unwrap();
    let server = Server::new(vec![(200, receipt("account-a", "new-refresh"))]);
    runtime().block_on(async {
        let (one, two) = tokio::join!(
            credential_at(&store, &server.endpoint),
            credential_at(&store, &server.endpoint)
        );
        assert_eq!(one.unwrap().refresh, "new-refresh");
        assert_eq!(two.unwrap().refresh, "new-refresh");
    });
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1, "rotating token must be used once");
    assert_eq!(requests[0]["refresh_token"], "old-refresh");
    assert_eq!(requests[0]["grant_type"], "refresh_token");
    assert_eq!(store.read().unwrap().unwrap().refresh, "new-refresh");
}
#[test]
fn failed_refresh_and_account_mismatch_preserve_previous_credential() {
    for (status, body) in [
        (401, "PRIVATE-ERROR".into()),
        (200, receipt("account-b", "wrong-account")),
    ] {
        let temp = Temp::new();
        let store = temp.store();
        store.write(&token()).unwrap();
        let before = fs::read(store.path()).unwrap();
        let server = Server::new(vec![(status, body)]);
        assert!(runtime()
            .block_on(credential_at(&store, &server.endpoint))
            .is_err());
        assert_eq!(fs::read(store.path()).unwrap(), before);
    }
}
#[test]
fn cancelling_pending_login_releases_callback_without_touching_credentials() {
    let temp = Temp::new();
    let store = temp.store();
    store.write(&token()).unwrap();
    let before = fs::read(store.path()).unwrap();
    let server = Server::new(vec![]);
    let pending = protocol::Pending::local();
    let url = pending.url(&server.endpoint).unwrap();
    let redirect = url
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .unwrap()
        .1
        .into_owned();
    let address = reqwest::Url::parse(&redirect).unwrap();
    runtime().block_on(async {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        assert!(tokio::time::timeout(
            Duration::from_millis(10),
            pending.complete(rx, &server.endpoint)
        )
        .await
        .is_err());
    });
    let listener =
        std::net::TcpListener::bind((address.host_str().unwrap(), address.port().unwrap()))
            .unwrap();
    drop(listener);
    assert!(server.requests.lock().unwrap().is_empty());
    assert_eq!(fs::read(store.path()).unwrap(), before);
}
