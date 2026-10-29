# Local runtime transport

The TUI and native agent worker live in one process. A private Unix socket carries control and presentation records; no agent subprocess or external RPC server is started. `src/runtime/bridge.rs` translates native lifecycle events into the existing TUI behavior, while provider formats stay in `provider.rs`.

The ingress mailbox is bounded by bytes and record count. `HIBISCUS_RPC_BUFFER_MIB` and `HIBISCUS_RPC_MAX_RECORDS` retain their existing names for the local adapter. Oversize, malformed or overloaded input fails explicitly rather than blocking on an unbounded queue. Writes use deadlines and service cancellation while waiting for readiness.

Cloned Unix sockets share O_NONBLOCK. The dedicated read adapter polls readiness and treats WouldBlock as temporary unavailability, not EOF. Shutdown closes both socket directions, cancels the native worker and waits for its tools to stop. The wake-generation counter is captured before polling queues so activity cannot be lost between polling and waiting.

Responses retain request IDs. Prompt acceptance is distinct from settlement. Tool calls execute only after a complete provider response; settlement follows recorded tool results and queued-message delivery. A disconnected or cancelled operation is never blindly replayed. Session resume marks missing tool results as unknown outcomes.

Transport pressure, duplex/cancellation, bounded-frame parsing and event-order unit tests remain active. Native end-to-end tests also exercise cancellation, queued messages and a CLI run with external agent/runtime executables absent from PATH. Older Pi-process profiling results do not measure this implementation.
