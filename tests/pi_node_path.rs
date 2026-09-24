use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn standalone_node_from_pi_installer_is_available_to_the_pi_child() {
    let root = std::env::temp_dir().join(format!(
        "hibiscus-standalone-node-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let bin = root.join("data/pi-node/current/bin");
    fs::create_dir_all(&bin).unwrap();
    let node = bin.join("node");
    fs::write(&node, "#!/bin/sh\necho fake-node\n").unwrap();
    fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
    let pi = root.join("pi");
    fs::write(
        &pi,
        r#"#!/bin/sh
[ "$(command -v node)" = "$HIBISCUS_TEST_NODE" ] || exit 22
while IFS= read -r line; do
    printf '{"type":"response","id":"hibiscus-1","success":true}\n'
    printf '{"type":"agent_settled"}\n'
done
"#,
    )
    .unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    // The parent PATH has no node, as in a shell that has not sourced Pi's
    // standalone Node PATH after installation.
    let result = Command::new(env!("CARGO_BIN_EXE_hibiscus"))
        .arg("hello")
        .env("PATH", root.join("empty-bin"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("HIBISCUS_PI", &pi)
        .env("HIBISCUS_TEST_NODE", &node)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}
