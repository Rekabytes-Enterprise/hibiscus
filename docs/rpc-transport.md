# RPC transport limits and overload recovery

Hibiscus continues to use Pi's JSONL protocol and `agent_settled` completion boundary. The transport does not reorder/coalesce protocol events, schedule another agent, or automatically replay failed commands.

## Why fail-fast instead of a blocking inbox?

Pi's RPC documentation requires clients to continuously drain stdout and honor stdin backpressure. A blocking bounded receive channel alone is unsafe: Hibiscus can be sending a large image prompt while Pi is writing events before it reads more stdin. If the stdout reader waits for the UI to free a queue slot, both pipes can remain full indefinitely.

The current policy is **bounded overload protection**, not transparent flow control:

- An independent reader stores complete LF-delimited records as raw bytes, not fully allocated JSON trees. CRLF and Unicode separators inside strings remain supported.
- The mailbox bounds both queued record count and allocated byte capacity. The consumer parses each record only when it dequeues it. For `tool_execution_update`, only the unused `partialResult` subtree is omitted from the resulting JSON value. For user `message_start`, image-block `data` is omitted while text, image count and metadata remain available for delivery UI. Skipped content is still validated (including escaped strings); other event types, message ends, unknown blocks and protocol-critical fields retain normal decoding.
- Record assembly is separately bounded, including streams with no LF. Payloads are never included in transport/parse diagnostics.
- If either limit is exceeded, the connection becomes invalid. Queued records are discarded as part of that explicit failure; they are **not silently dropped while the run is reported successful**. Pi's stdout read end is closed and writes/receives report uncertainty. Normal recovery terminates the owned live Pi process group.
- A successful run still requires its correlated acceptance and settlement. An overload never substitutes for settlement.

Defaults and optional environment settings (read on connection creation):

| Setting | Default | Allowed range | Purpose |
| --- | ---: | ---: | --- |
| `HIBISCUS_RPC_BUFFER_MIB` | 64 | 1–512 | MiB budget for queued allocated frame capacity; also the individual wire-record limit |
| `HIBISCUS_RPC_MAX_RECORDS` | 4096 | 1–65536 | Maximum queued records |
| `HIBISCUS_RPC_METRICS` | off | `1` enables | Print peak queued bytes/count and receive-failure status to stderr when an inbox is dropped |

An image-heavy `get_messages` response can exceed the default record limit. Raise limits only for a trusted workload with enough memory; reconnect to apply them. Capacity accounting is conservative: allocation slack counts too. The queue budget is **not a total process-RSS limit**: one assembling record, a record being parsed, JSON-tree expansion, output/layout state, allocator overhead, and Pi's separate process use additional memory. No transcript is spooled to disk by this implementation.

Metrics contain counters only, but the same stderr stream also carries Pi diagnostics. Redirect it when profiling full-screen UI, and review/redact any captured diagnostics before sharing. No normal RPC record or metric is written to Pi's stdin or Hibiscus's human-output stdout as debug logging.

## Writing and stopping

Pi stdin is nonblocking. Writes retry readiness in short waits, send at most 16 KiB per write, and have a 30-second deadline from the first write through the command's flush. This is not a model-generation deadline. A receive failure/closure interrupts a blocked send without waiting for that write deadline.

Initial and queued prompt sends service terminal input and UI ticks while waiting. Printable input stays in the composer. Another submission during an incomplete send is held for explicit resubmission, not interleaved into the unfinished JSON record.

**Esc during a partial send disconnects**, rather than appending `clear_queue`/`abort` bytes to a partial JSON record. Delivery is uncertain: some or all of the record may have reached Pi. Review session history after reconnecting before resending. Once the command is fully sent and the normal run loop is active, ordinary Esc still sends `clear_queue` then `abort` and waits for their correlated responses/settlement.

Serialization preserves borrowed image payloads and disarms `BufWriter` on error so its destructor cannot retry buffered command bytes. A failed queued send preserves that draft/images for explicit review without overwriting newer live typing. On receive failure, the newest still-unacknowledged queued submission is retained in the single `/restore` slot; this is not complete recovery of every queued message. Already accepted queues remain Pi-owned.

After an overload while an approval modal is open, the next attempted approval response fails on the invalid connection. Failure transitions and individual nonblocking writes are serialized under the mailbox lock; callbacks/readiness waits never hold that lock. This prevents a new write after invalidation, but cannot retract bytes already sent before the failure. The modal itself is not asynchronously dismissed by the transport; it retains its normal user/timeout behavior.

Orderly shutdown closes stdin, observes receiver failure, and bounds waiting for child exit and stderr completion to 30 seconds. Error recovery never guesses success or retries the uncertain operation.

## Regression coverage

`src/pi/transport.rs` tests:

- Slow-consumer FIFO delivery, including responses, queue updates, approval requests, settlement, CRLF and Unicode separators.
- Byte/count limits and actual frame-capacity accounting, explicit failure, and payload-redacted diagnostics.
- Oversized/no-LF input, a blocked stdin deadline, cancellation callbacks, and a complete large send to a draining peer.
- A duplex peer that fills stdout before reading stdin: overload interrupts the blocked writer rather than deadlocking.

`tests/rpc_pressure.rs` uses only synthetic Node/clipboard fixtures:

- Overload while a modal blocks consumption; a subsequent Yes is not sent, and reconnect does not replay the prompt.
- Esc during a blocked large initial-image send; `/restore` retains the attachment without resending.
- Esc during a blocked queued-image send; the uncertain queued submission remains reviewable.

The existing steering, settlement, error-recovery, auth-handoff, image and terminal suites remain required. These checks establish their simulated scenarios, not every real provider/event ordering or macOS behavior. Synthetic per-process RSS and allocation measurements are now available in [Memory profiling](memory-profiling.md). Call-stack heap attribution, real-provider memory measurements and a shared wake-driven event loop remain follow-up work.

Reference: Pi 0.87.1 [RPC framing](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc.md#framing). The installed-version docs were checked; the linked upstream page can evolve.
