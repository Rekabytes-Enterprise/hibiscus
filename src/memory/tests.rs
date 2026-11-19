use super::manager::{Manager, Scope};
use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};

#[test]
fn manager_conflicts_delete_and_restore_preserve_versions() {
    let f = Fixture::new();
    let manager = Manager::open(&f.0, &f.0.join("workspace")).unwrap();
    let global = manager.index(Scope::Global).to_path_buf();
    assert!(manager.save(&global, None, "first").unwrap());
    assert!(!manager.save(&global, Some("first"), "first").unwrap());
    assert!(manager.save(&global, None, "stale missing").is_err());
    assert!(manager.save(&global, Some("first"), "second").unwrap());
    assert!(manager.save(&global, Some("first"), "stale edit").is_err());
    assert!(manager.delete(&global, Some("first")).is_err());
    assert_eq!(manager.load(&global).unwrap().as_deref(), Some("second"));
    assert!(manager
        .load(manager.index(Scope::Project))
        .unwrap()
        .is_none());
    assert!(manager.delete(&global, Some("second")).unwrap());
    assert!(manager.load(&global).unwrap().is_none());
    let revisions = manager.revisions(&global).unwrap();
    assert_eq!(revisions.len(), 2);
    let restored = manager.revision(&global, &revisions[1].id).unwrap();
    assert_eq!(restored, "first");
    manager.save(&global, None, &restored).unwrap();
    assert_eq!(manager.load(&global).unwrap().unwrap(), "first");
    assert!(manager.revision(&global, "../../config.json").is_err());
    assert!(manager
        .save(&global, Some("first"), &"x".repeat(65537))
        .is_err());
    assert_eq!(manager.load(&global).unwrap().unwrap(), "first");
    assert!(manager.load(&f.0.join("config.json")).is_err());
}

#[test]
fn topic_files_are_memory_with_their_own_revisions() {
    let f = Fixture::new();
    let manager = Manager::open(&f.0, &f.0.join("workspace")).unwrap();
    let release = manager.topic(Scope::Project, "release").unwrap();
    assert_eq!(
        release,
        manager.topic(Scope::Project, "release.md").unwrap()
    );
    for bad in ["", "Release", "-x", "a/b", "../x", "MEMORY"] {
        assert!(manager.topic(Scope::Project, bad).is_err(), "{bad}");
    }
    assert!(scope(&f.0, &release).is_some());
    assert!(scope(&f.0, &release.with_file_name("Notes.md")).is_none());
    assert!(scope(&f.0, &release.with_file_name("release.txt")).is_none());
    manager.save(&release, None, "one").unwrap();
    manager.save(&release, Some("one"), "two").unwrap();
    let index = manager.index(Scope::Project).to_path_buf();
    manager
        .save(&index, None, "- Release → release.md")
        .unwrap();
    assert_eq!(manager.topics(Scope::Project).unwrap(), ["release.md"]);
    assert_eq!(manager.revisions(&release).unwrap().len(), 1);
    assert_eq!(
        manager
            .revision(&release, &manager.revisions(&release).unwrap()[0].id)
            .unwrap(),
        "one"
    );
    assert!(manager.revisions(&index).unwrap().is_empty());
}

#[test]
fn dates_round_trip_and_stale_facts_are_counted() {
    for (days, text) in [
        (0, "1970-01-01"),
        (11_016, "2000-02-29"),
        (20_776, "2026-11-19"),
    ] {
        assert_eq!(date(days), text);
        let (y, m, d) = (&text[..4], &text[5..7], &text[8..]);
        assert_eq!(
            days_from_civil(y.parse().unwrap(), m.parse().unwrap(), d.parse().unwrap()),
            days
        );
    }
    assert_eq!(confirmed("- Uses Feather icons [2026-11-19]"), Some(20_776));
    assert_eq!(confirmed("- see [docs] then [2026-13-01]"), None);
    assert_eq!(confirmed("- no date"), None);
    let f = Fixture::new();
    let layout = f.layout("w");
    layout.prepare(&f.0.join("w")).unwrap();
    let dir = layout.project.parent().unwrap();
    fs::write(&layout.project, "- Release → release.md [2026-11-19]\n").unwrap();
    fs::write(
        dir.join("release.md"),
        "- 16 files [2026-11-19]\n- old [2026-01-02]\n- older [2025-06-01]\n",
    )
    .unwrap();
    let (text, topics, stale) = scan(&layout.project, 20_776);
    assert!(text.unwrap().contains("release.md"));
    assert_eq!(topics, ["release.md"]);
    assert_eq!(stale, ["release.md: 2"]);
}

#[test]
fn context_loads_indexes_cuts_big_ones_and_asks_for_a_tidy() {
    let f = Fixture::new();
    let layout = f.layout("w");
    layout.prepare(&f.0.join("w")).unwrap();
    fs::write(&layout.global, "- Feather icons, no emoji [2099-01-01]\n").unwrap();
    let small = context(&f.0, &f.0.join("w"));
    assert!(small.contains("today is "));
    assert!(small.contains("- Feather icons, no emoji"));
    assert!(small.contains("Project memory index"));
    assert!(!small.contains("Memory needs tidying"));
    let big: String = (0..400)
        .map(|i| format!("- fact {i:04} [2099-01-01]\n"))
        .collect();
    fs::write(&layout.project, &big).unwrap();
    fs::write(layout.project.with_file_name("ui-style.md"), "- x\n").unwrap();
    let cut = context(&f.0, &f.0.join("w"));
    assert!(cut.contains("- fact 0000"));
    assert!(!cut.contains("- fact 0399"));
    assert!(cut.contains("[Index cut at 7 KiB of 9 KiB"), "{cut}");
    assert!(cut.contains("Topic files: ui-style.md"));
    assert!(cut.contains("Memory needs tidying: the Project index is over 4 KiB"));
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hibiscus-memory-{}-{}",
            std::process::id(),
            crate::engine::config::unique()
        ));
        fs::create_dir(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
    fn layout(&self, name: &str) -> Layout {
        Layout::new(&self.0, &self.0.join(name))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fresh_layout_is_private_scoped_and_does_not_invent_facts() {
    let f = Fixture::new();
    let a = f.layout("a");
    let b = f.layout("b");
    a.prepare(&f.0.join("a")).unwrap();
    b.prepare(&f.0.join("b")).unwrap();
    assert_eq!(a.global, b.global);
    assert_ne!(a.project, b.project);
    assert!(!a.global.exists());
    assert!(!a.project.exists());
    let metadata = a.project.with_file_name("workspace.json");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
    assert_eq!(value["name"], "a");
    assert_eq!(value["path"], f.0.join("a").to_str().unwrap());
    assert_eq!(
        fs::metadata(metadata).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(a.project.with_file_name("revisions"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn migration_preserves_originals_and_does_not_resurrect_deleted_memory() {
    let f = Fixture::new();
    let layout = f.layout("a");
    fs::write(&layout.legacy_global, "global fact").unwrap();
    fs::create_dir_all(layout.legacy_project.parent().unwrap()).unwrap();
    fs::write(&layout.legacy_project, "project fact").unwrap();
    layout.prepare(&f.0.join("a")).unwrap();
    for (new, old) in [
        (&layout.global, &layout.legacy_global),
        (&layout.project, &layout.legacy_project),
    ] {
        assert_eq!(fs::read(new).unwrap(), fs::read(old).unwrap());
        assert_eq!(
            fs::metadata(new).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(new).unwrap();
    }
    layout.prepare(&f.0.join("a")).unwrap();
    assert!(!layout.global.exists());
    assert!(!layout.project.exists());
    assert_eq!(
        fs::read_to_string(layout.legacy_global).unwrap(),
        "global fact"
    );
}

#[test]
fn existing_destination_wins_and_blocked_paths_preserve_legacy_data() {
    let f = Fixture::new();
    let layout = f.layout("a");
    fs::write(&layout.legacy_global, "old fact").unwrap();
    fs::create_dir_all(layout.global.parent().unwrap()).unwrap();
    fs::write(&layout.global, "new fact").unwrap();
    layout.prepare(&f.0.join("a")).unwrap();
    assert_eq!(fs::read_to_string(&layout.global).unwrap(), "new fact");
    assert_eq!(
        fs::read_to_string(&layout.legacy_global).unwrap(),
        "old fact"
    );
    let other = Fixture::new();
    let layout = other.layout("a");
    fs::write(&layout.legacy_global, "keep me").unwrap();
    fs::write(other.0.join("memory"), "blocking file").unwrap();
    assert!(layout.prepare(&other.0.join("a")).is_err());
    assert_eq!(fs::read_to_string(layout.legacy_global).unwrap(), "keep me");
}

#[test]
fn migration_rejects_symlinks_and_oversized_sources() {
    let f = Fixture::new();
    let layout = f.layout("a");
    let external = f.0.join("external");
    fs::write(&external, "untouched").unwrap();
    symlink(&external, &layout.legacy_global).unwrap();
    assert!(layout.prepare(&f.0.join("a")).is_err());
    fs::remove_file(&layout.legacy_global).unwrap();
    let file = fs::File::create(&layout.legacy_global).unwrap();
    file.set_len(MAX_FILE + 1).unwrap();
    assert!(layout.prepare(&f.0.join("a")).is_err());
    assert!(!layout.global.exists());
    assert!(!layout.global.with_file_name(".migration-complete").exists());
    assert_eq!(fs::read_to_string(external).unwrap(), "untouched");
}

#[test]
fn publish_never_replaces_existing_data() {
    let f = Fixture::new();
    let path = f.0.join("MEMORY.md");
    publish(&path, b"first").unwrap();
    publish(&path, b"second").unwrap();
    assert_eq!(fs::read(path).unwrap(), b"first");
}

#[test]
fn revisions_are_bounded_and_only_apply_to_active_memory() {
    let f = Fixture::new();
    let layout = f.layout("a");
    layout.prepare(&f.0.join("a")).unwrap();
    let dir = layout.project.with_file_name("revisions");
    fs::write(dir.join("notes.md"), "keep user file").unwrap();
    for n in 0..25 {
        backup(&f.0, &layout.project, n.to_string().as_bytes()).unwrap();
    }
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap() != "notes.md")
        .collect();
    files.sort();
    assert_eq!(files.len(), REVISIONS);
    assert_eq!(fs::read_to_string(&files[0]).unwrap(), "5");
    assert_eq!(fs::read_to_string(files.last().unwrap()).unwrap(), "24");
    assert_eq!(
        fs::read_to_string(dir.join("notes.md")).unwrap(),
        "keep user file"
    );
    backup(&f.0, &f.0.join("ordinary.txt"), b"ignored").unwrap();
    assert!(!f.0.join("revisions").exists());
    backup(&f.0, &layout.global, b"global previous").unwrap();
    assert_eq!(
        fs::read_dir(layout.global.with_file_name("revisions"))
            .unwrap()
            .count(),
        1
    );
}
