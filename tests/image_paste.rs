mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

fn run(empty: bool, wsl: bool) {
    let root = unique_temp_dir("hibiscus-image-paste");
    let pi = root.join("pi");
    let prompts = root.join("prompts");
    let image = root.join("image.png");
    // A synthetic 1x1 PNG; never read the user's real clipboard in tests.
    let bytes: Vec<u8> = "89504e470d0a1a0a0000000d4948445200000001000000010804000000b51c0c020000000b4944415478da63fcff1f0003030200efa63dff0000000049454e44ae426082"
        .as_bytes().chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect();
    fs::write(&image, if empty { b"".as_slice() } else { &bytes }).unwrap();
    for name in ["wl-paste", "xclip", "osascript", "powershell.exe"] {
        let path = root.join(name);
        fs::write(
            &path,
            r#"#!/bin/sh
if [ "$1" = '--list-types' ]; then
 if [ -s "$HIBISCUS_TEST_IMAGE" ]; then printf 'image/png\n'; else printf 'text/plain\n'; fi
else
 cat "$HIBISCUS_TEST_IMAGE"
fi
"#,
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(&pi, r#"#!/bin/sh
count=0
while IFS= read -r line; do
 id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
 case "$line" in
  *'"type":"get_state"'*) printf '{"type":"response","id":"%s","success":true,"data":{}}\n' "$id" ;;
  *'"type":"prompt"'*)
   count=$((count+1))
   printf '%s\n' "$line" >> "$HIBISCUS_TEST_PROMPTS"
   printf '{"type":"response","id":"%s","success":true}\n' "$id"
   printf '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"IMAGE_REPLY_%s"}}\n' "$count"
   sleep 0.05
   printf '{"type":"agent_settled"}\n' ;;
  *) exit 13 ;;
 esac
done
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_TEST_IMAGE", &image)
        .env("HIBISCUS_TEST_PROMPTS", &prompts)
        .env(
            "PATH",
            format!("{}:{}", root.display(), std::env::var("PATH").unwrap()),
        )
        .env("WAYLAND_DISPLAY", "test")
        .env_remove("WSL_INTEROP")
        .env_remove("WSL_DISTRO_NAME");
    if wsl {
        command.env("WSL_DISTRO_NAME", "test");
    }
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send · Ctrl+Enter");
    if empty {
        tty.send(b"keep my draft");
    } else if wsl {
        tty.send(b"describe this");
    }
    if wsl && cfg!(target_os = "linux") {
        tty.wait_text("Alt+V image");
    }
    tty.send(if wsl { b"\x1bv" } else { b"\x16" }); // Pi's WSL Alt+V, otherwise Ctrl+V
    if empty {
        tty.wait_text(if cfg!(target_os = "macos") {
            "No image in clipboard"
        } else {
            "No supported image in clipboard"
        });
    } else {
        tty.wait_text("1 image(s) attached");
        tty.send(if wsl { b"\x1b[118;3u" } else { b"\x1b[118;5u" });
        tty.wait_text("2 image(s) attached");
        // Remove both, then test the modifyOtherKeys encoding.
        tty.send(if wsl {
            b"\x18\x1b[27;3;118~"
        } else {
            b"\x18\x1b[27;5;118~"
        });
        tty.wait_text("Reading clipboard");
        tty.wait_text("1 image(s) attached");
    }
    assert!(!prompts.exists(), "pasting must not send anything");
    tty.send(b"\r"); // image-only is allowed
    tty.wait_text("IMAGE_REPLY_1");
    tty.wait_text("Enter send · Ctrl+Enter");
    tty.send(b"next text only\r");
    tty.wait_text("IMAGE_REPLY_2");
    tty.wait_text("Enter send · Ctrl+Enter");
    tty.send(b"/quit\r");
    let shown = tty.finish();
    let recorded = fs::read_to_string(&prompts).unwrap();
    let records: Vec<serde_json::Value> = recorded
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 2);
    assert!(records[1].get("images").is_none());
    if empty {
        assert_eq!(records[0]["message"], "keep my draft");
        assert!(records[0].get("images").is_none());
    } else {
        assert_eq!(
            records[0]["message"],
            if wsl { "describe this" } else { "" }
        );
        assert_eq!(records[0]["images"].as_array().unwrap().len(), 1);
        assert_eq!(records[0]["images"][0]["type"], "image");
        assert_eq!(records[0]["images"][0]["mimeType"], "image/png");
        assert!(records[0]["images"][0]["data"]
            .as_str()
            .unwrap()
            .starts_with("iVBORw0KGgo"));
        assert!(!shown.contains("iVBORw0KGgo"));
    }
    drop(tty);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn clipboard_images_are_staged_removed_and_sent_once() {
    run(false, false);
}

#[test]
fn wsl_clipboard_image_and_text_share_one_prompt() {
    run(false, true);
}

#[test]
fn empty_clipboard_keeps_text_draft_and_does_not_send_an_attachment() {
    run(true, false);
}
