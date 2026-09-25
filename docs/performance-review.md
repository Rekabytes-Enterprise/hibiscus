# Performance review

Review and implementation follow-up, 2026-09-25. The original measurements below are the pre-optimization baseline; see the follow-up for implemented changes and remaining work.

## Implementation follow-up

Implemented locally (not yet verified in real macOS/Windows Terminal use):

- Direct character-to-cell insertion, slice-based wrapping, and a failed-link-search sentinel remove the measured allocation/quadratic paths.
- Transcript layout is cached at explicit role-reset boundaries. Completed blocks reuse rows; unchanged composer/footer frames reuse the whole layout. Scrolled row counting is deferred/coalesced until layout, not repeated per RPC delta. Width/content/timeline changes invalidate relevant state. Workspace text is cached too.
- Terminal reads use 4 KiB chunks. Printable input is consumed in batches of at most 128 already available bytes, stopping before controls; active input processes at most eight actions before polling Pi again. UTF-8 and existing Enter/Esc semantics remain unchanged.
- A bounded process-local session metadata cache validates device/inode/size/mtime/ctime, rescans after 30 seconds for coarse timestamps, and invalidates deletions, replacements and workspace changes. At most 1,024 titles of up to 8 KiB are retained. Cold listing remains synchronous.
- RPC responses move their data instead of cloning it. Ordinary read buffers are reused (oversized buffers are released on the next record). Initial/queued prompt serialization borrows images, buffers small writes, and does not retry failed writes on `BufWriter` drop. Clipboard encoding moves its base64 buffer into JSON rather than copying it again.

### Repeatable checks

```sh
cargo bench --locked --bench formatter --bench allocations --bench session_list
cargo test --locked --release --test performance_tty -- --nocapture
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
```

The PTY regression verifies exact submitted text and bounded frame counts, not a brittle absolute timing threshold. Its polling adds approximately 10 ms of measurement granularity. Unit regressions cover incremental layout equivalence (including partial role markers, fences, tables and resize), row reuse/scroll anchoring, bounded UTF-8 batching/control ordering, session cache invalidation/expiry, borrowed image framing, and no implicit retry after an output error.

### Observed after the first implementation

On the same local host, using the original ad hoc fixtures:

| Workload | Before | After |
| --- | ---: | ---: |
| 521-byte burst, 256 KiB prose history | 2,307.70 ms | 0.31 ms |
| 521-byte burst, 128 KiB single-line history | 8,057.38 ms | 0.61 ms |
| Format 256 KiB prose | 8.52 ms | 2.42 ms |
| Format 256 KiB single line | 123.72 ms | 2.00 ms |
| Format 64 KiB unmatched brackets | 808.86 ms | 0.25 ms |
| Allocation/reallocation calls for 256 KiB prose | 262,142 | 26,604 |

The committed session benchmark measured 200 unchanged files at **0.385 ms** on repeat listing; its initial scan took **87.459 ms**. Original repeated uncached scanning took 70.34 ms. This is a cache-hit improvement, not a faster cold scan. These are screening measurements, not promises: host scheduling varies, and the microbenchmarks do not include every application cost. The committed release PTY regression passed and reported approximately 10 ms including driver polling.

### Deliberately not changed yet

The 40 ms RPC polling wait, unbounded inbound event channel, full incoming JSON deserialization, and remaining rejected-draft image copies are unchanged. No peak-RSS/backlog profile or duplex-pipe stress proof was obtained. A blocking bounded queue or a new event-loop architecture should not be introduced solely to mark a checklist complete: they can obstruct cancellation or deadlock while sending a large prompt. Background session refresh and broader frame-buffer/LTO tuning also remain candidates, not completed optimizations. Pi/provider speed and real-terminal cursor presentation are not established by these results.

## Scope and evidence

Reviewed the main/startup path, chat/session workflows, RPC receive/send loops, terminal input, transcript/Markdown rendering, row-diff painting, activity display, clipboard helpers, and relevant auth/update code. Focused measurements used synthetic data and a mock Pi process, not real providers, credentials, or saved chats.

Baseline: `dev` at `8616a00` plus the local MIT manifest metadata change; version 0.1.7. Source/test fingerprint: `c84cc96a11f576c7c62d7c4bd025f176b7755043fc6a2aef1017b520c8792d4b` (definition in `.pi/STATE.md`). Linux/WSL, Intel Core Ultra 7 165H, Rust 1.98.1. `cargo build --release --locked` succeeded. Neither `perf` nor the `hyperfine` benchmark runner was available; these are standalone timing/allocation experiments, not a whole-process flame graph. No macOS or Windows Terminal visual validation was performed.

Temporary standalone harnesses compiled the existing `markdown.rs` and `sessions.rs` directly with `rustc -O`; no optimized substitutes were used. The baseline harnesses were ad hoc research artifacts. The follow-up adds repository benchmarks and a PTY regression as described above.

## Measurements

### Markdown formatting in isolation

Width: 73 cells (the transcript width at an 80-column terminal). Medians include formatting and dropping its result, with one warm-up; 15 samples for prose, seven for long lines, three for unmatched brackets. Timing used the normal allocator. A separate instrumented build counted allocation/reallocation calls.

| Synthetic input | Approximate size | Median time per format |
| --- | ---: | ---: |
| Repeated short prose lines with bold/code spans | 32 KiB | 0.96 ms |
| Same prose | 128 KiB | 4.20 ms |
| Same prose | 256 KiB | 8.52 ms |
| One line of `x` characters | 128 KiB | 29.19 ms |
| One line of `x` characters | 256 KiB | 123.72 ms |
| One line of unmatched `[` characters | 16 KiB | 50.42 ms |
| Same unmatched brackets | 64 KiB | 808.86 ms |

The 256 KiB prose case made 262,142 allocation/reallocation calls per format in the instrumented build. The repeated prose line was `Some ordinary **bold** prose with ` followed by a backtick-delimited `code`, then ` and words for the terminal.\n`. Long lines and brackets are stress cases, not evidence of their frequency in ordinary chats.

### Input responsiveness through a PTY

Used the actual release binary, an 80×24 controlling PTY with color enabled, a synthetic settled response from mock Pi, and continuously drained terminal output while awaiting each marker. No provider/model latency was included. Twenty keys were sent individually, followed by a 521-byte burst (512 `x` characters and `PASTE_DONE`). Latency measures receipt of the final rendered bytes, not physical terminal presentation.

| Retained synthetic response | Individual-key median, range across two runs | 521-byte burst completion, one run |
| --- | ---: | ---: |
| Empty | 0.20–0.41 ms | 21.64 ms |
| 128 KiB prose | 2.45–5.20 ms | 1,197.85 ms |
| 256 KiB prose | 4.64–9.34 ms | 2,307.70 ms |
| 128 KiB single line | 15.51–31.88 ms | 8,057.38 ms |

Timings varied substantially between runs on this host. They establish scaling problems in these fixtures, not portable latency guarantees. This was a plain byte burst, not a measurement of every terminal's bracketed-paste behavior or active-run input.

### Session listing

Called the existing `sessions::list` with an isolated session directory. Every file had a valid matching workspace header and 1,000 synthetic user-message records. Seven measured calls followed a warm-up; file creation was outside timing.

| Files | Total JSONL size | Median list time, warm filesystem cache |
| ---: | ---: | ---: |
| 50 | 8.25 MiB | 17.23 ms |
| 200 | 33.02 MiB | 70.34 ms |

Cold disks, network filesystems and image-heavy real sessions were not measured.

## Original ranked recommendations (implementation status above)

### 1. Cache transcript layout separately from frame painting — highest priority

`Screen::render` (`src/tui/screen.rs:1361`) constructs/sanitizes the transcript and calls `markdown::format` over all retained text before choosing visible rows. `Painter::paint` only eliminates unchanged **output** afterward. Typing and footer/spinner updates therefore repeat parsing even when transcript content is unchanged.

Cache parsed/layout blocks keyed by content revision and width. Separate transcript, composer, footer and modal invalidation. Reuse completed blocks and reformat the active block as it changes; preserve Markdown state across fences and table lookahead. Resize and transcript trimming must invalidate relevant caches. Keep rendering only visible styled rows.

Also address `Screen::write` (`screen.rs:2080`) and `update_timeline`: while scrolled up, these perform before/after full formatting to count rows. That work happens before the 33 ms streaming paint gate, potentially twice per delta. Maintain cached row counts/scroll anchors instead. Its additional streaming cost is code-observed, not separately timed here.

### 2. Remove avoidable allocation and quadratic work from Markdown

`inline` (`src/tui/markdown.rs:44`) turns ordinary characters into temporary `String`s before appending a `Cell`. Push `Cell { ch, tone }` directly. Reuse/preallocate suitable buffers after measuring allocation profiles.

`wrap_cells` (`markdown.rs:280`) repeatedly drains the front of a `Vec` and removes leading spaces with `remove(0)`, moving remaining elements. Consume by index or iterator instead of shifting the tail for every row.

Link parsing repeatedly searches the remaining suffix for `](` on each unmatched `[`. The 16-to-64 KiB stress test increased input 4× and formatting time roughly 16×. Use forward parsing or cached delimiter positions, with tests preserving escapes, unmatched markers, code spans and wrapping. Caching alone will not cure a slow first parse of a pathological line.

### 3. Batch pasted/queued input, not individual human keystrokes

`Terminal::events_from` reads one byte after each `select` (`src/tui/terminal.rs:60–96`). `read_prompt` and `active_key` update/re-layout/render on each complete character. `ensure_cursor_visible` and `render` also rescan the draft.

Read available input in chunks, process bounded batches, and paint once per batch/deadline. Preserve immediate feedback for isolated keys and priority handling for Esc, Enter, UTF-8 and escape sequences; never let a large paste starve RPC processing. Consider bracketed-paste handling as an explicit UX decision, not an unreviewed change to newline/submission semantics.

Active input is checked around an RPC receive timeout of 40 ms (`rpc.rs:411`). A shared wake-up mechanism or readiness/deadline-based wait can improve responsiveness while Pi is silent without busy polling. Preserve the independently opened terminal reader and stop/join behavior required for auth handoff. This scheduling improvement was not benchmarked here.

### 4. Avoid rescanning unchanged session files

`src/chat/sessions.rs:91` parses every record in each matching session file whenever listing sessions. This runs synchronously before the picker appears. The warm-cache experiment measured 70 ms for 200 files/33 MiB; repeated scans are avoidable.

Cache display metadata against canonical path and file metadata, invalidate changed/deleted files, and consider background refresh. It must remain a disposable UI cache, not a second session database. Keep Pi authoritative. Do not simply stop at the first user message: later `session_info` records may rename a session, and workspace/header filtering must remain correct. Handle replacements, truncations and timestamp limitations when designing invalidation.

### 5. Reduce large JSON/image copies and control backlog

These are code-observed candidates; no peak-RSS or provider-event throughput measurements were performed:

- `Rpc::request` returns `record["data"].clone()` (`src/pi/rpc.rs:235`). Moving/taking that value avoids copying potentially large `get_messages` responses.
- The RPC reader creates a fresh line and full `serde_json::Value` for every event. Reuse the line buffer; investigate typed/partial deserialization for fields that are actually consumed. Unknown-event tolerance and protocol checks must remain intact.
- RPC events use unbounded `mpsc::channel` (`rpc.rs:74`). Slow rendering/dialogs can let the reader accumulate parsed events. Measure queued bytes and high-water marks before selecting backpressure/coalescing policies. A bounded message count is not a byte limit, and blindly blocking the reader can obstruct cancellation or create pipe deadlocks. Never drop/reorder responses, approvals, queue delivery or settlement events.
- Images permit four 10 MiB raw attachments, roughly 53.3 MiB after base64 before JSON/object overhead. `json!` construction and rejected-draft preservation can retain additional copies. Use borrowed serialization or shared immutable payloads where ownership/retry semantics allow; keep failed-draft recovery and privacy intact.

### 6. Smaller work after the above

Cache stable workspace/header text instead of calling `current_dir` each render; reuse frame/row buffers and reduce intermediate `format!`/`replace` strings. These are lower priority than whole-history parsing. Measure release-profile/LTO changes rather than assuming smaller binaries or more compiler flags improve interactive latency.

## What to preserve

- Existing dirty-row output, synchronized terminal updates and a visible steady cursor.
- Existing 33 ms streaming coalescing and 256 KiB retained transcript bound; the issue is work outside/before those limits, not their absence.
- A long-lived Pi RPC child, separate stderr, strict JSONL handling and response correlation. Acceptance remains distinct from delivery and `agent_settled` completion.
- Background, cached startup update checks and bounded/cancellable clipboard reads.
- Pi's ownership of models, authentication, tools, retries and persistence. These UI optimizations do not increase model generation speed.

No evidence here justifies replacing the stack, introducing a second agent, parallelizing protocol mutation, switching allocators, or migrating everything to an async runtime.

## Original implementation order and verification goals

1. Add repeatable optimized-build formatter, allocation and PTY latency benchmarks covering these fixtures.
2. Make the parser's allocation/wrapping/search work linear where possible; add malformed-input regressions.
3. Separate layout caching from painting, including scroll anchoring and block invalidation tests.
4. Batch input and verify Esc/queue fairness under simultaneous paste and tool-event bursts.
5. Measure session and JSON/RSS paths before implementing their changes.

Track key-to-output p50/p95/p99, paste completion time, frame CPU time, allocations, terminal bytes/frame, queued-event bytes and peak RSS. Reasonable initial **targets**, not promised results: sub-16 ms ordinary key p95 and sub-100 ms 521-byte paste completion at the retained transcript limit on a fixed reference host. Compare fixed-workload before/after runs; avoid flaky absolute timing gates on shared CI runners. Retest real Windows Terminal/WSL and macOS after synthetic checks.

## Research sources

- [The Rust Performance Book — profiling](https://nnethercote.github.io/perf-book/profiling.html): optimized-build profiling, sampling tools and allocation profiling; supports measuring before broad rewrites.
- [The Rust Performance Book — heap allocations](https://nnethercote.github.io/perf-book/heap-allocations.html): allocation costs, buffer reuse and shared ownership trade-offs.
- [Rust `Vec::remove`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.remove): removing from the front shifts remaining elements and is O(n).
- [Rust `mpsc::channel`](https://doc.rust-lang.org/std/sync/mpsc/fn.channel.html): asynchronous channel sends do not block and the buffer is unbounded.
- [serde_json `RawValue`](https://docs.rs/serde_json/latest/serde_json/value/struct.RawValue.html): deferred/borrowed JSON payloads can avoid materializing values; ownership/lifetime constraints matter when crossing threads. It is a design option, not a drop-in fix or zero-copy claim for the current pipeline.
