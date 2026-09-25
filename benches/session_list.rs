//! Synthetic files only; compare a first scan with cached repeat listings.
// Source modules include #[cfg(test)] imports under Clippy's bench build,
// which has no test harness. They remain checked normally in the binary tests.
#![allow(dead_code, unused_imports)]
#[path = "../src/chat/sessions.rs"]
mod sessions;
#[path = "../src/tui/ui.rs"]
pub(crate) mod ui;
mod tui {
    pub(crate) use crate::ui;
}
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
use std::{
    fs,
    hint::black_box,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn main() {
    let root = std::env::temp_dir().join(format!(
        "hibiscus-session-bench-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let fixture = Fixture(root.canonicalize().unwrap());
    std::env::set_var("PI_CODING_AGENT_SESSION_DIR", &fixture.0);
    let header =
        serde_json::json!({"type":"session","id":"synthetic","cwd":fixture.0}).to_string() + "\n";
    let message = serde_json::json!({"type":"message","message":{"role":"user","content":"Synthetic test message ".repeat(5)}}).to_string() + "\n";
    let data = header + &message.repeat(1000);
    for count in [50, 200] {
        for i in 0..count {
            fs::write(fixture.0.join(format!("{i}.jsonl")), &data).unwrap();
        }
        let start = Instant::now();
        assert_eq!(sessions::list(&fixture.0).unwrap().len(), count);
        let first = start.elapsed().as_secs_f64() * 1000.0;
        let mut times = Vec::new();
        for _ in 0..15 {
            let start = Instant::now();
            black_box(sessions::list(&fixture.0).unwrap());
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "sessions={count} first_scan_ms={first:.3} cached_median_ms={:.3}",
            times[7]
        );
    }
}
