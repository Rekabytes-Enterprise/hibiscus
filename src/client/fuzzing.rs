pub(crate) fn rpc(bytes: &[u8]) {
    super::transport::fuzz_record(bytes);
}
