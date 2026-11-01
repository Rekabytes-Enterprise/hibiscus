#![cfg(feature = "fuzzing")]

type Boundary = (&'static str, fn(&[u8]));

#[test]
fn curated_native_boundary_inputs_replay_without_side_effectful_runtime_startup() {
    let targets: [Boundary; 6] = [
        ("extension_manifest", hibiscus::fuzzing::extension_manifest),
        ("extension_reply", hibiscus::fuzzing::extension_reply),
        ("rpc_record", hibiscus::fuzzing::rpc_record),
        ("provider_stream", hibiscus::fuzzing::provider_stream),
        ("oauth_callback", hibiscus::fuzzing::oauth_callback),
        ("relative_paths", hibiscus::fuzzing::relative_paths),
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/corpus");
    for (name, target) in targets {
        let mut count = 0;
        for entry in std::fs::read_dir(root.join(name)).unwrap() {
            let path = entry.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            assert!(bytes.len() <= 131072, "{}", path.display());
            target(&bytes);
            count += 1;
        }
        assert!(count > 0, "missing curated inputs for {name}");
    }
}
