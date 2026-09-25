//! Run with `cargo bench --bench formatter`. Timings are diagnostic, not CI gates.
#![allow(dead_code)]
#[path = "../src/tui/markdown.rs"]
mod markdown;
use std::{hint::black_box, time::Instant};

fn measure(label: &str, samples: usize, mut run: impl FnMut()) {
    run();
    let mut times = Vec::new();
    for _ in 0..samples {
        let start = Instant::now();
        run();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "{label}: median_ms={:.4} samples={samples}",
        times[samples / 2]
    );
}

fn main() {
    let line = "Some ordinary **bold** prose with `code` and words for the terminal.\n";
    for bytes in [32 * 1024, 128 * 1024, 256 * 1024] {
        for (name, text) in [
            ("prose", line.repeat(bytes / line.len())),
            ("single-line", "x".repeat(bytes)),
            ("unmatched-brackets", "[".repeat(bytes)),
        ] {
            measure(&format!("{name} bytes={}", text.len()), 15, || {
                black_box(markdown::format(black_box(&text), 73));
            });
        }
    }
    let mut layout = markdown::Layout::default();
    let history = format!(
        "you › question\nhibi › {}\nyou › next\nhibi › ",
        line.repeat(3900)
    );
    layout.update(&history, 73);
    measure("cached rows (unchanged transcript)", 1000, || {
        black_box(layout.rows());
    });
    let mut tail = String::new();
    measure(
        "append to final message (completed blocks cached)",
        50,
        || {
            tail.push('x');
            layout.update(&format!("{history}{tail}"), 73);
            black_box(layout.rows());
        },
    );
}
