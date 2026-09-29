mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn at_file_picker_inserts_path_without_reading_file_or_sending_on_selection() {
    let root = unique_temp_dir("hibiscus-at-file");
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let fd = bin.join("fd");
    fs::write(
        &fd,
        "#!/bin/sh\nprintf './README.md\\0./src/\\0./my notes.md\\0'\n",
    )
    .unwrap();
    fs::set_permissions(&fd, fs::Permissions::from_mode(0o755)).unwrap();
    let pi = root.join("pi");
    fs::write(&pi, r#"#!/bin/sh
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_session_stats"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"prompt"'*)
   printf '%s\n' "$line" > "$MENTION_PROMPT"
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"MENTION_DONE"}}' '{"type":"agent_settled"}' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(root.join("README.md"), "PRIVATE_CONTENT_NOT_TO_SEND").unwrap();
    fs::write(root.join("my notes.md"), "ANOTHER_PRIVATE_FILE").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .current_dir(&root)
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_NO_UPDATE_CHECK", "1")
        .env("MENTION_PROMPT", root.join("prompt.json"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"check @rea");
    tty.wait_text("Files");
    tty.send(b"\r");
    tty.wait_text("@README.md");
    assert!(
        !root.join("prompt.json").exists(),
        "Enter on file picker must not submit"
    );
    tty.send(b"done\r");
    tty.wait_text("MENTION_DONE");
    tty.wait_text("Enter send");
    let prompt: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("prompt.json")).unwrap()).unwrap();
    assert_eq!(prompt["message"], "check @README.md done");
    assert!(!prompt.to_string().contains("PRIVATE_CONTENT_NOT_TO_SEND"));

    tty.send(b"check @\"my");
    tty.wait_text("Files");
    tty.send(b"\t");
    tty.wait_text("@\"my notes.md\"");
    tty.send(b"please\r");
    tty.wait_text("MENTION_DONE");
    tty.wait_text("Enter send");
    tty.send(b"/quit\r");
    tty.finish();
    let prompt: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("prompt.json")).unwrap()).unwrap();
    assert_eq!(prompt["message"], "check @\"my notes.md\" please");
    assert!(!prompt.to_string().contains("ANOTHER_PRIVATE_FILE"));
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}
