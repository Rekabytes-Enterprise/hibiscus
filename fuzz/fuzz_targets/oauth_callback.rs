#![no_main]
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| hibiscus::fuzzing::oauth_callback(bytes));
