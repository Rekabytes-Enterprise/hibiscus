//! Allocation-call counts, separate from timing (instrumentation has overhead).
#![allow(dead_code)]
#[path = "../src/tui/markdown.rs"]
mod markdown;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
};
static CALLS: AtomicUsize = AtomicUsize::new(0);
struct Counter;
// SAFETY: allocation ownership/layout is forwarded unchanged to System.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        System.realloc(ptr, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
fn main() {
    let line = "Some ordinary **bold** prose with `code` and words for the terminal.\n";
    for (name, source) in [
        ("prose", line.repeat(256 * 1024 / line.len())),
        ("single-line", "x".repeat(256 * 1024)),
        ("unmatched-brackets", "[".repeat(64 * 1024)),
    ] {
        CALLS.store(0, Ordering::Relaxed);
        let rows = markdown::format(black_box(&source), 73);
        black_box(&rows);
        let calls = CALLS.load(Ordering::Relaxed);
        println!(
            "{name} bytes={} allocation/reallocation_calls={calls}",
            source.len()
        );
        drop(rows);
    }
}
