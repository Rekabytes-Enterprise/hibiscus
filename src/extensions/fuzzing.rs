use super::{manifest, process, versions};

pub(crate) fn manifest(bytes: &[u8]) {
    if let Ok((manifest, validators)) = manifest::parse(bytes) {
        assert_eq!(manifest.api_version, 1);
        assert_eq!(manifest.tools.len(), validators.len());
        for validator in validators {
            // Exercise the compiled schema without running its tool.
            let _ = validator.is_valid(&serde_json::json!({}));
        }
    }
}
pub(crate) fn reply(bytes: &[u8]) {
    process::fuzz_reply(bytes);
}
pub(crate) fn path(bytes: &[u8]) {
    if let Ok(value) = std::str::from_utf8(bytes) {
        if let Ok(relative) = versions::relative(value) {
            let root = std::path::Path::new("/fuzz-workspace");
            assert!(root.join(relative).starts_with(root));
            assert!(relative
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))));
        }
    }
}
