# Memory profiling: synthetic Linux results

## Selective tool-update decoding follow-up (local, unreleased)

`src/pi/transport.rs` now validates but does not build a `serde_json::Value` tree for top-level `partialResult` on `tool_execution_update`. Hibiscus does not consume that intermediate result; it still fully decodes `tool_execution_start`, `tool_execution_end` (including goal details and edit diffs), responses, messages, queue updates, approval requests, settlement and unknown event types. Non-update events with a `partialResult` field retain it unchanged. A custom validation walker checks nested JSON and string escapes (including invalid surrogate escapes), even for repeated discarded fields; `RawValue` alone did **not** provide that validation. Byte/count inbox limits and strict LF framing are unchanged.

Against the same isolated Linux fixture, three fresh processes per case:

| Ignored incoming update | Baseline client observed VmHWM | Selective decoder VmHWM | Wire payload |
| --- | ---: | ---: | ---: |
| Numeric array | 37.09 MiB | **7.18 MiB** | ~2 MiB |
| Text string | 7.15 MiB | **7.22 MiB** | ~2 MiB |

Follow-up release binary SHA-256: `092e3916f3b4e8d49dc98d7e22e8f42d5656d9034e54cb8938b87ecaed714b7c`. All six follow-up experiments completed without receive failures; inbox peak allocated capacity was 4 MiB in each run. This is a **narrow optimization** for ignored tool updates, not general partial decoding of image echoes, session history or authoritative tool results. A `RawValue` copy and validation pass still cost time/memory; neither a total-process memory guarantee nor real Pi/provider event coverage follows from these fixtures.

## Shared-image follow-up (local, unreleased)

Staged attachments now use immutable `Arc<serde_json::Value>` handles across the composer, queued submission, and explicit `/restore` slot. Clipboard encoding wraps each value once; `serde`'s `rc` support serializes the referenced image into Pi's **unchanged JSONL wire format**. This removes the extra base64 allocation when a queued submission is rejected without deleting the recoverable image or overwriting newer typing. Incoming Pi user echoes are still parsed independently and can still cause high transient RSS.

On the same Linux fixture, three fresh runs for each scenario (client observed `VmHWM`, approximate):

| Scenario | Before (median) | Shared images (median) | Scope |
| --- | ---: | ---: | --- |
| Rejected queued four × 10 MiB images | 123.24 MiB | 69.64 MiB | The targeted duplicate recovery/composer copy |
| Rejected initial four × 10 MiB images | 69.75 MiB | 69.74 MiB | No duplicate expected |
| Successful four × 10 MiB images plus incoming echo | 195.24 MiB | 195.28 MiB | Incoming echo/serialization still dominate |

The isolated allocation benchmark reports **55,926,840 bytes** for cloning four owned image values versus **32 bytes** (one small vector allocation) for cloning four shared handles. These are synthetic measurements, not portable RSS guarantees or proof that real Pi uses the same event cadence. The initial full-matrix baseline below remains labelled with its own binary fingerprint; the follow-up measurements used rebuilt binary SHA-256 `f0fb1cef5720bc9456633679e862516ca49fa2f271bbf08a3bbe3da5162ceb86` and three repeats per listed scenario. Run `cargo build --release --locked`, then `python3 scripts/profile-memory.py --scenario queue-reject --repeats 3 --output /tmp/hibiscus-shared-images.json` to reproduce locally. Real macOS clipboard and provider behavior still need verification.

The full-matrix baseline below was measured on the 0.1.7 application source at `dbba12f` (same runtime source as `dc5212c`), using the optimized release binary. No production runtime changes were made in this profiling task.

## Method and boundaries

- Linux/WSL, Rust/Cargo 1.98.1, Python 3.12.3. `heaptrack`, `valgrind` and `perf` were unavailable; this is process-counter sampling plus a separate instrumented allocation benchmark, **not a call-stack heap profile**.
- `scripts/profile-memory.py` runs the actual binary in an 80×24 controlling PTY with a synthetic Python Pi replacement. It isolates HOME/Pi/cache settings, does not inherit credentials, disables update checks, and shadows all clipboard helpers. No real providers, saved conversations or desktop clipboard contents are used.
- Reads per-PID RSS from `/proc/<pid>/smaps_rollup` at approximately 10 ms intervals and observes `/proc/<pid>/status`'s `VmHWM` while each process is alive. Hibiscus and the mock backend are recorded separately; child totals are not attributed to Hibiscus. Birth identifiers protect against PID reuse, and pre-exec Python memory is excluded from Hibiscus samples.
- `VmHWM` is an **approximate Linux counter**. RSS sampling can miss short peaks; the counters are not read atomically and may disagree slightly. Do not interpret the figures as exact maximum allocation or a process-memory guarantee. The profiler also perturbs scheduling.
- Eleven scenarios, three fresh-process repeats each: **33 completed experiments**. File preparation is outside the measured child. Phase measurements wait for distinct rendered state markers, not a previously displayed idle footer.
- Queue counters come from `HIBISCUS_RPC_METRICS=1`. Reports contain numeric phases/counters and a binary SHA-256, not transcripts or credentials. Temporary fixture data/diagnostics are cleaned up. An incomplete run marks its report `complete: false` rather than preserving an older success report.

Profiled binary SHA-256: `fcf3ff78f7ab317f1d5fe0043091aea2ea208cec4794c3e9752fda3e44122472`.

## Results

Figures are MiB. Each column is the median of three per-run peaks; the bracketed range is for observed Hibiscus `VmHWM`. The Python backend is a measurement fixture, **not representative of real Pi/provider memory**. Do not add these RSS columns to infer unique physical memory; shared pages may be counted in both processes.

| Scenario | Hibiscus observed VmHWM [range] | Hibiscus sampled RSS peak | Mock backend observed VmHWM |
| --- | ---: | ---: | ---: |
| Idle | 3.06 [2.94–4.78] | 3.30 | 18.94 |
| 20 turns × about 256 KiB prose | 9.00 [8.99–9.07] | 9.33 | 19.57 |
| Send four 2 MiB images, including incoming user echo | 41.87 [41.83–42.57] | 31.52 | 63.33 |
| Send four 10 MiB images + echo, 128 MiB queue budget | 195.21 [195.08–195.25] | 187.88 | 245.75 |
| Same maximum images + echo, default 64 MiB budget | 195.24 [195.19–195.26] | 181.94 | 245.60 |
| Reject initial prompt with four 10 MiB images | 69.75 [69.73–69.78] | 70.03 | 179.08 |
| Reject queued prompt with four 10 MiB images | 123.24 [123.22–123.33] | 123.36 | 179.07 |
| Ignored update with about 2 MiB text | 7.15 [7.01–7.20] | 3.39 | 24.85 |
| Ignored update with about 2 MiB numeric array | 37.09 [37.04–37.13] | 33.92 | 24.91 |
| Paused consumer, 8 MiB queue budget | 11.24 [11.21–11.24] | 11.04 | 19.41 |
| Paused consumer, 32 MiB queue budget | 35.91 [35.87–36.00] | 35.60 | 19.39 |

The image fixtures use signature-valid synthetic PNG bytes, not decoded screenshots. They exercise capture, base64, serialization, incoming echo and recovery ownership; they do not test image decoding or provider resizing.

Both maximum-image send configurations completed in all three runs. The default-budget case reached **64 MiB queued allocated capacity**; this does not establish headroom under other event ordering/load. The paused-consumer bursts intentionally disconnected: queue high-water counters reached their 8/32 MiB budgets and did not exceed them. There were no unexpected receive failures in the other scenarios.

### Long-chat retention

Median steady Hibiscus RSS was about 6.20 MiB after turn 1, 9.18 after turn 5, 9.20 after turn 10 and 9.21 after turn 20. It remained about 9.21 after `/new`. No sustained growth was observed beyond the initial rise **in this prose-only 20-turn fixture**. This is not proof of leak freedom or of bounded memory in every tool/session workload.

### Image staging and rejection

Four 10 MiB raw attachments represent about **53.33 MiB of base64**. In the queued-rejection case, median steady RSS grew from 70.02 MiB after staging to 123.36 MiB after rejection. `Screen::rejected_queue` explicitly clones `submission.images` into the live composer while retaining the original for `/restore`.

Clearing/restoring did not immediately return RSS to idle: after clearing the final restored queued draft it remained around 70.15 MiB. Initial rejection ended around 19.49 MiB after clearing its restored draft. **Freed Rust allocations need not be returned to the OS immediately.** These RSS residuals alone do not establish a leak or prove which buffers remain live.

## Isolated logical allocation measurements

`cargo bench --locked --bench allocations` now reports counts, added live/peak requested layout bytes, and bytes remaining after dropping the result. Input buffers are allocated before each measurement and excluded from the added-byte figure. This counts Rust allocator requests, not allocator overhead, virtual-memory slack or OS RSS; realloc's internal temporary copying is not measured.

| Operation | Additional live allocation bytes | Interpretation |
| --- | ---: | --- |
| Clone the four maximum-size image values | 55,926,840 (~53.34 MiB) | An avoidable second owned image copy; 29 allocation/reallocation calls |
| Parse the ~2 MiB text-bearing JSON value | 2,098,461 (~2.00 MiB) | Input wire buffer excluded |
| Parse the similarly sized array-bearing JSON value | 33,555,741 (~32.00 MiB) | Same-order wire size, much larger JSON tree |

All measured operation results returned tracked live bytes to their pre-operation baseline when dropped. That applies to these isolated operations, not the complete application. Formatter measurements are also included in the benchmark.

## Recommended next changes

1. **Shared immutable image payloads:** implemented in the follow-up above. Preserve exact image serialization, newer live typing, explicit `/restore`, and the no-auto-replay rule during future changes.
2. **Avoid materializing unused incoming payloads.** The ignored tool-update `partialResult` case is implemented above. Incoming image echoes and other large records still construct JSON trees; any broader selective decoder must preserve validation, response IDs, errors, approvals and settlement. Raw frames must remain bounded and accounted for. A wire-byte budget is not a JSON-tree or process-RSS budget.
3. **Keep the event-loop redesign separate.** These results identify memory costs, not the latency benefit of replacing the 40 ms wait. Measure input wake latency independently before changing scheduling.
4. **Do not switch allocators or add forced trimming yet.** Obtain heap attribution/longer repeated ownership tests before calling residual RSS a leak or using allocator-specific workarounds.

The baseline profiling task did not implement these optimizations. The subsequent shared-image and narrow selective tool-update changes are described above; broader selective decoding remains pending.

## Reproduce

```sh
cargo build --release --locked
python3 tests/profile_memory.py
python3 scripts/profile-memory.py --repeats 3 --output /tmp/hibiscus-memory.json
cargo bench --locked --bench allocations
```

Use `--scenario queue-reject`, `--scenario incoming-array`, or another name shown by `--help` to isolate one workload. The default `--repeats 1` is a quick screening pass; `--repeats 3` runs the full 33-experiment matrix. Every scenario has bounded waits and cleanup; expected overloads are reported as `disconnected`, not hidden as successful model runs. `completed` means the profiling workflow completed, including intentional request-rejection cases.

Linux with readable `smaps_rollup` is required; no macOS estimate is substituted. Reports identify the binary by hash and reject binary changes between repeats. Do not run concurrent rebuilds while profiling. Put reports outside the repository and avoid comparing debug builds to these release results.

References: Linux [`proc_pid_status`](https://man7.org/linux/man-pages/man5/proc_pid_status.5.html) documents the limitations of VmHWM/VmRSS; [`proc_pid_smaps`](https://man7.org/linux/man-pages/man5/proc_pid_smaps.5.html) describes resident/proportional mapping accounting. See also [the transport budget/recovery design](rpc-transport.md) and [the earlier CPU/input performance review](performance-review.md).
