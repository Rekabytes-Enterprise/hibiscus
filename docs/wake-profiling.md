# Quiet-run input wake measurements

This is an optional local probe, not a real-provider latency guarantee. Hibiscus continues to use Pi as its only agent/backend.

## Method

`python3 scripts/profile-wake.py --samples 60 --output /tmp/hibiscus-wake.json` launches the release binary in an isolated 80×24 Linux controlling PTY. It uses a temporary mock Pi which accepts one prompt, emits a thinking event, then remains silent until the user presses Esc. The probe sends 60 individual printable keys with varied arrival intervals and times each key until the corresponding composer text appears on the PTY. It drains output concurrently and verifies the run stops and chat exits. No real Pi settings, credentials, session history or clipboard are accessed. The optional helper tests run with `python3 tests/profile_wake.py`.

The reported duration includes the terminal reader, Hibiscus scheduling/rendering and PTY delivery. It is **not** model response time, physical cursor presentation, or a universal terminal/input latency. The driver and host scheduler add noise. Timing is not asserted in CI. Report files belong outside the repository; no executable digest is committed.

## Observations on one Linux/WSL host

| Silent active run, 60 keys | Median | p95 | Range |
| --- | ---: | ---: | ---: |
| Before wake signal (40 ms RPC receive timeout) | 21.02 ms | 37.09 ms | 2.96–37.15 ms |
| With shared activity signal | 0.26 ms | 0.49 ms | 0.16–0.80 ms |

One repeat measured 0.25 ms median and 0.46 ms p95 after the change. These are synthetic numbers from this host; another terminal, OS, scheduler, or workload can behave differently.

## Design and safety boundaries

The terminal-reader and Pi-stdout reader signal one shared condition variable **after** enqueuing input or a complete RPC frame. The active RPC loop snapshots a generation counter before polling either queue. On no record, it waits for a changed generation or the existing 40 ms deadline. This avoids a missed notification between checking a queue and going to sleep. A notification is only a hint to recheck; the independently ordered queues, response IDs, dialog approvals, `clear_queue`/`abort` and `agent_settled` remain authoritative. The timeout fallback still drives the flower/Explore animation, elapsed footer, pending streaming frame and cancellation deadline. No extra reader steals terminal keys; the existing stoppable `/dev/tty` reader still stops/joins before Pi TUI handoff. Cross-session wakeups can be spurious but do not change protocol state.

Check the full test suite plus steering, rendering, working-cursor, approval, modal, image, error-recovery and RPC-pressure PTY regressions after scheduling changes. The automated checks cannot prove a cursor is steady in Windows Terminal or that real Pi/provider event ordering is complete. Retest those cases on the intended terminal before release.
