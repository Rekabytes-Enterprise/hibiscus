mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

#[test]
fn approval_picker_changes_setting_and_escape_preserves_it_without_prompting_pi() {
    let root = unique_temp_dir("hibiscus-approval-picker");
    let pi = root.join("pi");
    fs::write(
        &pi,
        r#"#!/usr/bin/env python3
import json,sys
for line in sys.stdin:
 c=json.loads(line)
 if c['type'] not in ['get_state','get_session_stats']: sys.exit(13)
 print(json.dumps({'type':'response','id':c['id'],'success':True,'data':{}}),flush=True)
"#,
    )
    .unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hibiscus"));
    command
        .env("HIBISCUS_PI", &pi)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HIBISCUS_HOME", root.join("hibiscus"))
        .env("HIBISCUS_NO_UPDATE_CHECK", "1");
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("Enter send");
    tty.send(b"/approval\r");
    tty.wait_text("Shell approvals");
    tty.send(b"\x1b[B\r");
    tty.wait_text("Approvals disabled:");
    tty.send(b"/approval\r");
    tty.wait_text("Shell approvals");
    tty.send(b"\x1b");
    tty.wait_text("Enter send");
    tty.send(b"/approval\r");
    tty.wait_text("Shell approvals");
    // Disabled remains selected after cancellation; Up chooses Enabled.
    tty.send(b"\x1b[A\r");
    tty.wait_text("Approvals enabled:");
    tty.send(b"/quit\r");
    tty.finish();
    fs::remove_dir_all(root).unwrap();
}
