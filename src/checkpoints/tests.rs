use super::*;
use std::os::unix::fs::PermissionsExt;
struct Fixture {
    root: PathBuf,
    workspace: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("hibiscus-checkpoints-{}", Id::new()));
        let workspace = root.join("project");
        let home = root.join("data");
        files::directory(&workspace).unwrap();
        files::directory(&home).unwrap();
        Self {
            root,
            workspace,
            home,
        }
    }
    fn store(&self) -> Store {
        Store::at(&self.workspace, &self.home).unwrap()
    }
    fn write(&self, store: &Store, request: &mut Request, path: &str, text: &str) -> Result<Value> {
        store.write(
            &mut Context {
                cwd: &self.workspace,
                request,
                call_id: "tool-0",
            },
            &self.workspace.join(path),
            text.as_bytes(),
            None,
        )
    }
    fn context<'a>(&'a self, request: &'a mut Request) -> Context<'a> {
        Context {
            cwd: &self.workspace,
            request,
            call_id: "call",
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn select(id: &Id, paths: Option<Vec<&str>>) -> Selection {
    Selection {
        checkpoint: id.clone(),
        paths: paths.map(|paths| {
            paths
                .into_iter()
                .map(|p| Relative::try_from(p.to_owned()).unwrap())
                .collect()
        }),
    }
}
#[tokio::test]
async fn request_group_preserves_user_bytes_permissions_and_selectively_removes_new_files() {
    let f = Fixture::new();
    let store = f.store();
    let mut request = Request::new("session");
    let id = request.id.clone();
    let source = f.workspace.join("script.sh");
    fs::write(&source, b"user work\r\noriginal\r\n").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o750)).unwrap();
    files::directory(&f.workspace.join(".git")).unwrap();
    fs::write(f.workspace.join(".git/index"), "uncommitted-index").unwrap();
    f.write(&store, &mut request, "script.sh", "first edit")
        .unwrap();
    f.write(&store, &mut request, "script.sh", "second edit")
        .unwrap();
    f.write(&store, &mut request, "new/sub.txt", "new file")
        .unwrap();
    let record = store.load(&id).unwrap();
    assert_eq!(record.files.len(), 2);
    assert_eq!(record.files[&Relative("script.sh".into())].calls.len(), 2);
    assert_eq!(
        store.history(10, None).unwrap()["checkpoints"][0]["active"],
        true
    );
    let (_send, mut cancel) = watch::channel(false);
    assert!(store.restore(select(&id, None), &mut cancel).await.is_err());
    drop(request);
    let report = store
        .restore(select(&id, Some(vec!["script.sh"])), &mut cancel)
        .await
        .unwrap();
    assert_eq!(report["restored"], json!(["script.sh"]));
    assert_eq!(fs::read(&source).unwrap(), b"user work\r\noriginal\r\n");
    assert_eq!(
        fs::metadata(&source).unwrap().permissions().mode() & 0o777,
        0o750
    );
    assert!(f.workspace.join("new/sub.txt").exists());
    store.restore(select(&id, None), &mut cancel).await.unwrap();
    assert!(!f.workspace.join("new/sub.txt").exists());
    let again = store.restore(select(&id, None), &mut cancel).await.unwrap();
    assert_eq!(again["restored"], json!([]));
    assert_eq!(again["alreadyRestored"].as_array().unwrap().len(), 2);
    assert_eq!(
        fs::read_to_string(f.workspace.join(".git/index")).unwrap(),
        "uncommitted-index"
    );
    let diff = store.diff(select(&id, None)).unwrap();
    assert!(diff["diff"].as_str().unwrap().contains("+second edit"));
}
#[tokio::test]
async fn preflight_conflicts_preserve_all_selected_files_and_later_manual_edits() {
    let f = Fixture::new();
    let store = f.store();
    let mut request = Request::new("session");
    let id = request.id.clone();
    for path in ["a", "b"] {
        fs::write(f.workspace.join(path), "before").unwrap();
        f.write(&store, &mut request, path, "after").unwrap();
    }
    drop(request);
    fs::write(f.workspace.join("b"), "manual edit").unwrap();
    let (_send, mut cancel) = watch::channel(false);
    let error = store
        .restore(select(&id, None), &mut cancel)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("no selected files were changed"));
    assert_eq!(fs::read_to_string(f.workspace.join("a")).unwrap(), "after");
    assert_eq!(
        fs::read_to_string(f.workspace.join("b")).unwrap(),
        "manual edit"
    );
    assert!(store.diff(select(&id, None)).unwrap()["files"][1]["conflict"].is_string());
    store
        .restore(select(&id, Some(vec!["a"])), &mut cancel)
        .await
        .unwrap();
    assert_eq!(fs::read_to_string(f.workspace.join("a")).unwrap(), "before");
    fs::write(f.workspace.join("a"), "later after restore").unwrap();
    assert!(store
        .restore(select(&id, Some(vec!["a"])), &mut cancel)
        .await
        .is_err());
}
#[tokio::test]
async fn interrupted_write_intents_reconcile_only_known_before_or_result_states() {
    for outcome in ["applied", "unapplied", "unknown"] {
        let f = Fixture::new();
        let store = f.store();
        let mut request = Request::new("s");
        let id = request.id.clone();
        fs::write(f.workspace.join("file"), "before").unwrap();
        f.write(&store, &mut request, "file", "after").unwrap();
        drop(request);
        let mut record = store.load(&id).unwrap();
        let entry = record.files.get_mut(&Relative("file".into())).unwrap();
        entry.phase = Phase::PendingWrite {
            previous: entry.before.clone(),
        };
        store.save(&record).unwrap();
        if outcome == "unapplied" {
            fs::write(f.workspace.join("file"), "before").unwrap();
        }
        if outcome == "unknown" {
            fs::write(f.workspace.join("file"), "other process").unwrap();
        }
        let (_send, mut cancel) = watch::channel(false);
        let result = store.restore(select(&id, None), &mut cancel).await;
        if outcome == "unknown" {
            assert!(result.is_err());
            assert_eq!(
                fs::read_to_string(f.workspace.join("file")).unwrap(),
                "other process"
            );
        } else {
            assert!(result.is_ok());
            assert_eq!(
                fs::read_to_string(f.workspace.join("file")).unwrap(),
                "before"
            );
        }
    }
}
#[tokio::test]
async fn interrupted_multi_file_restore_and_cancellation_are_idempotent_after_restart() {
    let f = Fixture::new();
    let store = f.store();
    let mut request = Request::new("s");
    let id = request.id.clone();
    for path in ["a", "b"] {
        fs::write(f.workspace.join(path), "before").unwrap();
        f.write(&store, &mut request, path, "after").unwrap();
    }
    drop(request);
    let mut record = store.load(&id).unwrap();
    for entry in record.files.values_mut() {
        entry.phase = Phase::Restoring;
    }
    store.save(&record).unwrap();
    let entry = &record.files[&Relative("a".into())];
    files::apply(
        &f.workspace.join("a"),
        &entry.before,
        &store.bytes(&entry.before).unwrap(),
    )
    .unwrap();
    let restarted = f.store();
    let (_send, mut cancel) = watch::channel(true);
    assert!(restarted
        .restore(select(&id, None), &mut cancel)
        .await
        .is_err());
    assert_eq!(fs::read_to_string(f.workspace.join("b")).unwrap(), "after");
    let (_send, mut cancel) = watch::channel(false);
    let result = restarted
        .restore(select(&id, None), &mut cancel)
        .await
        .unwrap();
    assert_eq!(result["alreadyRestored"], json!(["a"]));
    assert_eq!(result["restored"], json!(["b"]));
    assert_eq!(
        restarted.history(1, None).unwrap()["checkpoints"][0]["pendingFiles"],
        0
    );
}
#[tokio::test]
async fn missing_or_corrupt_images_never_restore_partial_selected_files() {
    let f = Fixture::new();
    let store = f.store();
    let mut request = Request::new("s");
    let id = request.id.clone();
    fs::write(f.workspace.join("file"), "before").unwrap();
    f.write(&store, &mut request, "file", "after").unwrap();
    drop(request);
    let record = store.load(&id).unwrap();
    let Image::Present { hash, .. } = &record.files[&Relative("file".into())].before else {
        panic!()
    };
    fs::write(store.root.join("blobs").join(hash), "damaged").unwrap();
    let (_send, mut cancel) = watch::channel(false);
    assert!(store
        .restore(select(&id, None), &mut cancel)
        .await
        .unwrap_err()
        .to_string()
        .contains("corrupt"));
    assert_eq!(
        fs::read_to_string(f.workspace.join("file")).unwrap(),
        "after"
    );
}
#[test]
fn repeated_writes_refuse_later_edits_and_stale_edit_reads() {
    let f = Fixture::new();
    let store = f.store();
    let mut request = Request::new("s");
    fs::write(f.workspace.join("file"), "original").unwrap();
    f.write(&store, &mut request, "file", "agent").unwrap();
    fs::write(f.workspace.join("file"), "manual").unwrap();
    assert!(f.write(&store, &mut request, "file", "overwrite").is_err());
    let mut new_request = Request::new("s");
    let mut context = f.context(&mut new_request);
    assert!(store
        .write(
            &mut context,
            &f.workspace.join("file"),
            b"edited",
            Some(b"stale read")
        )
        .is_err());
    assert_eq!(
        fs::read_to_string(f.workspace.join("file")).unwrap(),
        "manual"
    );
}
#[tokio::test]
async fn binary_originals_and_mode_only_conflicts_are_checked() {
    let f = Fixture::new();
    let store = f.store();
    let mut request = Request::new("s");
    let id = request.id.clone();
    let path = f.workspace.join("binary");
    fs::write(&path, [0, 255, 128, 10]).unwrap();
    f.write(&store, &mut request, "binary", "utf8 replacement")
        .unwrap();
    drop(request);
    let (_send, mut cancel) = watch::channel(false);
    let before_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(store.restore(select(&id, None), &mut cancel).await.is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(before_mode)).unwrap();
    store.restore(select(&id, None), &mut cancel).await.unwrap();
    assert_eq!(fs::read(&path).unwrap(), [0, 255, 128, 10]);
}
#[test]
fn scope_links_and_special_files_have_explicit_coverage() {
    let f = Fixture::new();
    let store = f.store();
    let mut request = Request::new("s");
    let external = f.root.join("external");
    let result = store
        .write(&mut f.context(&mut request), &external, b"outside", None)
        .unwrap();
    assert_eq!(result["protected"], false);
    let result = f
        .write(&store, &mut request, ".git/index", "explicit native write")
        .unwrap();
    assert_eq!(result["protected"], false);
    assert_eq!(store.history(10, None).unwrap()["checkpoints"], json!([]));
    std::os::unix::fs::symlink(&external, f.workspace.join("link")).unwrap();
    assert!(f.write(&store, &mut request, "link", "bad").is_err());
    assert_eq!(fs::read_to_string(external).unwrap(), "outside");
    std::os::unix::fs::symlink(&f.root, f.workspace.join("parent_link")).unwrap();
    assert!(f
        .write(&store, &mut request, "parent_link/escape", "bad")
        .is_err());
    assert!(!f.root.join("escape").exists());
    let fifo = f.workspace.join("fifo");
    let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(f
        .write(&store, &mut request, "fifo", "no blocking read")
        .is_err());
}
#[tokio::test]
async fn workspace_isolation_paths_and_snapshot_metadata_are_validated() {
    let f = Fixture::new();
    let store = f.store();
    let mut request = Request::new("s");
    let id = request.id.clone();
    f.write(&store, &mut request, "file", "after").unwrap();
    drop(request);
    let other = f.root.join("other");
    files::directory(&other).unwrap();
    let other = Store::at(&other, &f.home).unwrap();
    assert_eq!(other.history(10, None).unwrap()["checkpoints"], json!([]));
    let (_send, mut cancel) = watch::channel(false);
    assert!(other.restore(select(&id, None), &mut cancel).await.is_err());
    for path in [
        "../outside",
        "/absolute",
        ".git/index",
        "a/../b",
        "a//b",
        "a/./b",
        "",
    ] {
        assert!(Relative::try_from(path.to_owned()).is_err());
    }
    assert!(Id::try_from("../../wrong".to_owned()).is_err());
    let mut record = store.load(&id).unwrap();
    record.workspace = other.workspace.clone();
    files::atomic(
        &store.directory(&id).join("record.json"),
        &serde_json::to_vec(&record).unwrap(),
        0o600,
    )
    .unwrap();
    assert!(store.load(&id).is_err());
}
#[test]
fn file_storage_and_trace_limits_fail_before_mutation() {
    let f = Fixture::new();
    let store = f.store();
    let mut request = Request::new("s");
    for i in 0..MAX_FILES {
        f.write(&store, &mut request, &format!("file{i}"), "a")
            .unwrap();
    }
    assert!(f.write(&store, &mut request, "too_many", "b").is_err());
    assert!(!f.workspace.join("too_many").exists());
    let f = Fixture::new();
    let store = f.store();
    let mut request = Request::new("s");
    let path = f.workspace.join("unchanged");
    fs::write(&path, "before").unwrap();
    let sparse = fs::File::create(store.root.join("blobs").join("0".repeat(64))).unwrap();
    sparse.set_len(MAX_WORKSPACE).unwrap();
    assert!(f
        .write(&store, &mut request, "unchanged", "after")
        .unwrap_err()
        .to_string()
        .contains("storage limit"));
    assert_eq!(fs::read_to_string(path).unwrap(), "before");
}
#[test]
fn explicit_pruning_retains_active_and_pending_requests_and_collects_only_orphans() {
    let f = Fixture::new();
    let store = f.store();
    let mut active = Request::new("s");
    let active_id = active.id.clone();
    f.write(&store, &mut active, "active", "live").unwrap();
    let mut pending_id = None;
    for i in 0..4 {
        let mut request = Request::new("s");
        let id = request.id.clone();
        f.write(
            &store,
            &mut request,
            &format!("file{i}"),
            &format!("value{i}"),
        )
        .unwrap();
        drop(request);
        if i == 0 {
            let mut record = store.load(&id).unwrap();
            let entry = record.files.values_mut().next().unwrap();
            entry.phase = Phase::PendingWrite {
                previous: Image::Missing,
            };
            store.save(&record).unwrap();
            pending_id = Some(id);
        }
    }
    let orphan = store.root.join("blobs").join(files::digest(b"orphan"));
    fs::write(&orphan, "orphan").unwrap();
    let result = store.prune(1).unwrap();
    assert!(store.load(&active_id).is_ok());
    assert!(store.load(&pending_id.unwrap()).is_ok());
    assert!(!orphan.exists());
    assert_eq!(result["deletedCheckpoints"].as_array().unwrap().len(), 2);
}
#[test]
fn workspace_lock_serializes_writers_and_record_permissions_are_private() {
    let f = Fixture::new();
    let store = f.store();
    let lock = store.lock().unwrap();
    let mut request = Request::new("s");
    assert!(f.write(&store, &mut request, "file", "after").is_err());
    assert!(!f.workspace.join("file").exists());
    drop(lock);
    f.write(&store, &mut request, "file", "after").unwrap();
    assert_eq!(
        fs::metadata(store.directory(&request.id).join("record.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&store.root).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn active_newest_requests_do_not_count_toward_retained_completed_history() {
    let f = Fixture::new();
    let store = f.store();
    let mut old = Request::new("s");
    let old_id = old.id.clone();
    f.write(&store, &mut old, "old", "old data").unwrap();
    drop(old);
    let mut live = Request::new("s");
    let live_id = live.id.clone();
    f.write(&store, &mut live, "new", "live data").unwrap();
    let temporary = store
        .root
        .join("blobs")
        .join(format!(".hibiscus-{}.tmp", Id::new()));
    fs::write(&temporary, "interrupted image write").unwrap();
    assert_eq!(store.prune(1).unwrap()["deletedCheckpoints"], json!([]));
    assert!(store.load(&old_id).is_ok());
    assert!(store.load(&live_id).is_ok());
    assert!(!temporary.exists());
}
#[test]
fn full_checkpoint_store_refuses_a_new_write_without_changing_the_project() {
    let f = Fixture::new();
    let store = f.store();
    for _ in 0..MAX_REQUESTS {
        let record = Record {
            format: 1,
            id: Id::new(),
            workspace: store.workspace.clone(),
            session_id: "s".into(),
            created_at: crate::runtime::config::unique().to_string(),
            files: BTreeMap::new(),
        };
        store.save(&record).unwrap();
    }
    fs::write(f.workspace.join("user.txt"), "user original").unwrap();
    let mut request = Request::new("s");
    assert!(f
        .write(&store, &mut request, "user.txt", "replacement")
        .unwrap_err()
        .to_string()
        .contains("request limit"));
    assert_eq!(
        fs::read_to_string(f.workspace.join("user.txt")).unwrap(),
        "user original"
    );
    store.prune(2).unwrap();
    f.write(&store, &mut request, "user.txt", "replacement")
        .unwrap();
}

#[tokio::test]
async fn checkpoint_tools_reject_non_object_arguments_before_opening_storage() {
    let f = Fixture::new();
    let mut request = Request::new("s");
    let (_send, mut cancel) = watch::channel(false);
    for tool in Tool::ALL {
        for args in [json!(null), json!([]), json!(true)] {
            assert!(tool
                .execute(&args, &mut f.context(&mut request), &mut cancel)
                .await
                .unwrap_err()
                .to_string()
                .contains("JSON object"));
        }
    }
}
