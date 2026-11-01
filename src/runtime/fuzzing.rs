pub(crate) fn rpc(bytes: &[u8]) {
    super::transport::fuzz_record(bytes);
}
pub(crate) fn provider(bytes: &[u8]) {
    super::provider::fuzz_stream(bytes);
}
pub(crate) fn oauth(bytes: &[u8]) {
    super::oauth::fuzz_callback(bytes);
}
