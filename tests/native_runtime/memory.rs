use super::*;

fn memory_file(f: &Fixture) -> PathBuf {
    f.root.join("app/memory/global/MEMORY.md")
}

#[test]
fn cli_memory_manager_edits_deletes_and_restores_without_model_calls() {
    let f = Fixture::new("responses", vec![]);
    fs::write(f.root.join("app/MEMORY.md"), "first\n").unwrap();
    let mut command = f.command();
    command.arg("--cli").env_remove("HIBISCUS_TEST_NATIVE_KEY");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("/tui");
    tty.send(b"/memory global\r");
    tty.wait_text("6. Back");
    tty.send(b"2\r");
    tty.wait_text("Enter save");
    tty.send("\x1b[200~second\n世界\x1b[201~\r".as_bytes());
    tty.wait_text("Memory saved · Global");
    tty.wait_text("6. Back");
    assert_eq!(
        fs::read_to_string(memory_file(&f)).unwrap(),
        "first\nsecond\n世界"
    );
    // Cancellation keeps the current file, and the next command stays local.
    tty.send(b"2\r");
    tty.wait_text("Enter save");
    tty.send(b"discard\x1b");
    tty.wait_text("6. Back");
    assert!(!fs::read_to_string(memory_file(&f))
        .unwrap()
        .contains("discard"));
    tty.send(b"3\r");
    tty.wait_text("2. Confirm");
    tty.send(b"2\r");
    tty.wait_text("Memory deleted · Global");
    tty.wait_text("6. Back");
    assert!(!memory_file(&f).exists());
    tty.send(b"4\r");
    tty.wait_text("newest first");
    tty.send(b"1\r");
    tty.wait_text("Memory revision preview");
    tty.wait_text("3. Previous page");
    tty.send(b"1\r");
    tty.wait_text("Restore Global memory?");
    tty.wait_text("2. Confirm");
    tty.send(b"2\r");
    tty.wait_text("Memory restored · Global");
    tty.wait_text("6. Back");
    tty.send(b"6\r");
    tty.wait_text("❯");
    tty.send(b"/memory project\r");
    tty.wait_text("0 bytes");
    tty.wait_text("6. Back");
    tty.send(b"\x1b");
    tty.wait_text("❯");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(shown.contains("first"));
    assert!(f.captured().is_empty());
    assert_eq!(
        fs::read_to_string(memory_file(&f)).unwrap(),
        "first\nsecond\n世界"
    );
}

#[test]
fn tui_memory_manager_edits_and_restores_with_local_dialogs() {
    let f = Fixture::new("responses", vec![]);
    fs::write(f.root.join("app/MEMORY.md"), "first\n").unwrap();
    let mut command = f.command();
    command.env_remove("HIBISCUS_TEST_NATIVE_KEY");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"/memory\r");
    tty.wait_text("Memory · local files");
    tty.send(b"\r");
    tty.wait_text("Recommended maximum");
    tty.send(b"\x1b[B\r");
    tty.wait_text("Edit memory · Global");
    tty.send("\x1b[200~second\n世界\x1b[201~\r".as_bytes());
    tty.wait_text("Memory saved · Global");
    tty.wait_text("Recommended maximum");
    assert_eq!(
        fs::read_to_string(memory_file(&f)).unwrap(),
        "first\nsecond\n世界"
    );
    tty.send(b"\x1b[B\x1b[B\r");
    tty.wait_text("Delete Global memory?");
    tty.send(b"\x1b[B\r");
    tty.wait_text("Memory deleted · Global");
    tty.wait_text("No saved memory");
    tty.send(b"\x1b[B\x1b[B\x1b[B\r");
    tty.wait_text("Memory revisions · newest first");
    tty.send(b"\r");
    tty.wait_text("Memory revision preview");
    tty.send(b"\r");
    tty.wait_text("Restore Global memory?");
    tty.send(b"\x1b[B\r");
    tty.wait_text("Memory restored · Global");
    tty.wait_text("Recommended maximum");
    tty.send(b"\x1b");
    tty.wait_text("Memory · local files");
    tty.send(b"\x1b");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    assert!(f.captured().is_empty());
    assert_eq!(
        fs::read_to_string(memory_file(&f)).unwrap(),
        "first\nsecond\n世界"
    );
    let screens = support::screen::snapshots(&shown);
    assert!(screens
        .iter()
        .any(|s| s.contains("Edit memory") && s.contains("first")));
}

#[test]
fn piped_memory_is_read_only_and_never_enters_model_history() {
    let f = Fixture::new("responses", vec![response("CHAT_ONLY")]);
    fs::write(f.root.join("app/MEMORY.md"), "- saved fact\n").unwrap();
    let output = f.run("/memory global\nhello\n/quit\n", &[]);
    good(&output);
    let shown = String::from_utf8_lossy(&output.stdout);
    assert!(shown.contains("- saved fact"));
    assert_eq!(f.captured().len(), 1);
    let invalid = f.run("/memory invalid\n/quit\n", &[]);
    assert!(!invalid.status.success());
    assert_eq!(f.captured().len(), 1);
    assert!(!f.captured()[0]["input"].to_string().contains("/memory"));
    assert_eq!(
        fs::read_to_string(memory_file(&f)).unwrap(),
        "- saved fact\n"
    );
}
