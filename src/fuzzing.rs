//! Development-only bounded entry points. No provider, process, auth store or file I/O.
const MAX_INPUT: usize = 131_072;

macro_rules! target {
    ($name:ident, $owner:path) => {
        pub fn $name(bytes: &[u8]) {
            if bytes.len() <= MAX_INPUT {
                $owner(bytes);
            }
        }
    };
}
target!(extension_manifest, crate::extensions::fuzzing::manifest);
target!(extension_reply, crate::extensions::fuzzing::reply);
target!(rpc_record, crate::runtime::fuzzing::rpc);
target!(provider_stream, crate::runtime::fuzzing::provider);
target!(oauth_callback, crate::runtime::fuzzing::oauth);

pub fn relative_paths(bytes: &[u8]) {
    if bytes.len() <= MAX_INPUT {
        crate::extensions::fuzzing::path(bytes);
        crate::checkpoints::fuzzing::path(bytes);
    }
}
