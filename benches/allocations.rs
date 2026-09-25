//! Logical Rust allocation counts/bytes, not RSS or allocator-internal overhead.
//! Instrumentation is separate from timing and never used in the CLI binary.
#![allow(dead_code)]
#[path = "../src/tui/markdown.rs"]
mod markdown;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
};
static CALLS: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn acquired(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}
struct Counter;
// SAFETY: allocation ownership/layout is forwarded unchanged to System. Counts
// change only after successful allocation/reallocation; failed realloc retains
// the original block. Counters themselves do not allocate.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        let pointer = System.alloc(layout);
        if !pointer.is_null() {
            acquired(layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        let pointer = System.realloc(ptr, layout, size);
        if !pointer.is_null() {
            if size >= layout.size() {
                acquired(size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - size, Ordering::Relaxed);
            }
        }
        pointer
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;

fn measure<T>(name: &str, source_bytes: usize, operation: impl FnOnce() -> T) {
    let baseline = LIVE.load(Ordering::Relaxed);
    CALLS.store(0, Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let value = operation();
    black_box(&value);
    let calls = CALLS.load(Ordering::Relaxed);
    let peak = PEAK.load(Ordering::Relaxed) - baseline;
    let retained = LIVE.load(Ordering::Relaxed) - baseline;
    drop(value);
    let after_drop = LIVE.load(Ordering::Relaxed);
    assert_eq!(
        after_drop, baseline,
        "operation must release its tracked allocations"
    );
    println!("{name} source_bytes={source_bytes} calls={calls} peak_added_bytes={peak} retained_added_bytes={retained} after_drop_added_bytes=0");
}

fn main() {
    let line = "Some ordinary **bold** prose with `code` and words for the terminal.\n";
    for (name, source) in [
        ("prose", line.repeat(256 * 1024 / line.len())),
        ("single-line", "x".repeat(256 * 1024)),
        ("unmatched-brackets", "[".repeat(64 * 1024)),
    ] {
        measure(name, source.len(), || {
            markdown::format(black_box(&source), 73)
        });
    }
    // The actual clone operation used by Screen::rejected_queue. Sources are
    // allocated before measurement; only the additional copy is counted.
    let encoded = 4 * (10 * 1024 * 1024usize).div_ceil(3);
    let images = (0..4)
        .map(|_| {
            let mut image = serde_json::json!({"type":"image","mimeType":"image/png"});
            image["data"] = serde_json::Value::String("A".repeat(encoded));
            image
        })
        .collect::<Vec<_>>();
    measure("four-image-value-clone-baseline", encoded * 4, || {
        black_box(&images).clone()
    });
    let shared: Vec<std::sync::Arc<serde_json::Value>> =
        images.into_iter().map(std::sync::Arc::new).collect();
    measure("four-image-shared-clone", encoded * 4, || {
        black_box(&shared).clone()
    });

    // Equivalent ignored payloads: the input buffer is outside the counted
    // region. Transport queues raw bytes, but the consumer still builds Value.
    let size = 2 * 1024 * 1024;
    for (name, payload) in [
        ("json-text", format!("\"{}\"", "x".repeat(size))),
        ("json-array", format!("[{}0]", "0,".repeat(size / 2 - 1))),
    ] {
        let source = format!(
            "{{\"type\":\"tool_execution_update\",\"partialResult\":{{\"details\":{payload}}}}}"
        );
        measure(name, source.len(), || {
            serde_json::from_str::<serde_json::Value>(black_box(&source)).unwrap()
        });
    }
}
