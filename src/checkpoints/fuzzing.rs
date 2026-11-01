use super::Relative;

pub(crate) fn path(bytes: &[u8]) {
    if let Ok(value) = std::str::from_utf8(bytes) {
        if let Ok(relative) = Relative::try_from(value.to_owned()) {
            let root = std::path::Path::new("/fuzz-workspace");
            assert!(root.join(&relative.0).starts_with(root));
            assert!(!relative
                .0
                .split('/')
                .any(|p| p.eq_ignore_ascii_case(".git")));
            assert_eq!(Relative::try_from(relative.0.clone()).unwrap(), relative);
        }
    }
}
