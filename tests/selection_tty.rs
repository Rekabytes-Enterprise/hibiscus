mod support;
use std::{fs, process::Command};
use support::{unique_temp_dir, Pty};

fn fixture() -> (std::path::PathBuf, Pty) {
    let root = unique_temp_dir("hibiscus-selection");
    // The macOS backend tries pbcopy even without DISPLAY. Never allow PTY
    // fixtures to reach a runner's real clipboard; force the OSC 52 fallback.
    for name in ["pbcopy", "wl-copy", "xclip", "xsel"] {
        let helper = root.join(name);
        fs::write(
            &helper,
            format!("#!/bin/sh\nprintf '{name}\\n' >> \"$SELECTION_CLIPBOARD_LOG\"\nexit 1\n"),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let script = root.join("pi");
    fs::write(&script, r#"#!/usr/bin/env python3
import json,sys
for line in sys.stdin:
  cmd=json.loads(line); kind=cmd['type']; ident=cmd.get('id')
  if kind=='get_state': data={'sessionName':'selection','model':{'provider':'mock','id':'test'}}
  elif kind=='get_session_stats': data={}
  elif kind=='prompt':
    print(json.dumps({'type':'response','id':ident,'success':True}),flush=True)
    print(json.dumps({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'Copyable reply'}}),flush=True)
    print(json.dumps({'type':'agent_settled'}),flush=True)
    continue
  else: sys.exit(11)
  print(json.dumps({'type':'response','id':ident,'success':True,'data':data}),flush=True)
"#).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut tty = Pty::spawn(
        Command::new(env!("CARGO_BIN_EXE_hibiscus"))
            .env("HIBISCUS_PI", &script)
            .env("HIBISCUS_NO_UPDATE_CHECK", "1")
            .env("SELECTION_CLIPBOARD_LOG", root.join("clipboard-calls"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY"),
    );
    tty.wait_text("selection");
    (root, tty)
}

#[test]
fn drag_select_copies_visible_transcript_without_sending_mouse_bytes_to_pi() {
    let (root, mut tty) = fixture();
    tty.send(b"hello\r");
    tty.wait_text("Copyable reply");
    // The mock reply is on terminal row 8, starting at column 6.
    tty.send(b"\x1b[<0;6;8M\x1b[<32;14;8M\x1b[<0;14;8m");
    tty.wait_text("\x1b]52;c;Q29weWFibGU=\x07");
    // Ctrl+C with an active selection copies it again instead of exiting.
    tty.send(b"\x03");
    tty.wait_text("\x1b]52;c;Q29weWFibGU=\x07");
    tty.send(b"/quit\r");
    let output = tty.finish();
    assert!(
        output.contains("\x1b[7mC\x1b[27m"),
        "selection must be visible even without color"
    );
    assert!(
        output.contains("\x1b[?1002h"),
        "button-motion reporting must be enabled"
    );
    assert!(
        output.contains("\x1b[?1002l"),
        "button-motion reporting must be disabled"
    );
    assert_clipboard_isolated(&root, 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn narrow_transcript_selection_uses_reduced_margin() {
    let (root, mut tty) = fixture();
    tty.resize(48, 24);
    tty.wait_text("hibiscus ·");
    tty.send(b"hello\r");
    tty.wait_text("Copyable reply");
    // Narrow layout has a one-cell outer margin and one-cell marker gap.
    tty.send(b"\x1b[<0;4;8M\x1b[<32;12;8M\x1b[<0;12;8m");
    tty.wait_text("\x1b]52;c;Q29weWFibGU=\x07");
    tty.send(b"/quit\r");
    tty.finish();
    assert_clipboard_isolated(&root, 1);
    fs::remove_dir_all(root).unwrap();
}

fn assert_clipboard_isolated(root: &std::path::Path, calls: usize) {
    let log = root.join("clipboard-calls");
    if cfg!(target_os = "macos") {
        assert_eq!(
            fs::read_to_string(log).unwrap(),
            "pbcopy\n".repeat(calls),
            "must use only the fake clipboard helper"
        );
    } else {
        assert!(
            !log.exists(),
            "a headless Linux fixture must not use a desktop clipboard"
        );
    }
}

#[test]
fn idle_ctrl_c_requires_a_second_press_and_other_input_disarms_exit() {
    let (root, mut tty) = fixture();
    tty.send(b"\x03");
    tty.wait_text("Press Ctrl+C again within 2s to quit");
    // Ordinary typing must disarm the exit; clearing that draft must not exit.
    tty.send(b"draft");
    tty.wait_text("draft");
    tty.send(b"\x03");
    tty.wait_text("Press Ctrl+C again within 2s to quit");
    tty.send(b"\x03");
    let output = tty.finish();
    assert!(output.contains("\x1b[?1049l"), "terminal must be restored");
    fs::remove_dir_all(root).unwrap();
}
