//! Shared OAuth loopback and credential storage for native browser sign-in.
pub(super) mod protocol;
pub(super) mod store;

#[cfg(feature = "fuzzing")]
pub(super) fn fuzz_callback(bytes: &[u8]) {
    protocol::fuzz_callback(bytes);
}
