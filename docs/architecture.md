# Architecture

Hibiscus is a standalone Rust coding agent. It owns provider connections, tool execution, credentials, sessions and terminal presentation. See [native migration](native-runtime.md) for configuration and compatibility changes.

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | CLI modes and startup |
| `src/runtime/config.rs` | Validate provider/model configuration, private atomic storage |
| `src/runtime/provider.rs` | HTTPS/SSE adapters for Responses, Chat Completions and Anthropic |
| `src/runtime/session.rs` | Typed messages, versioned sessions, resume and interrupted-call recovery |
| `src/runtime/tools.rs` | Native file, shell and checklist tools |
| `src/runtime/engine.rs` | Model/tool loop, queueing, cancellation, approvals and compaction |
| `src/runtime/policy.rs` | Project/personal instructions, project skill catalog and writing guidance |
| `src/runtime/event.rs` | Typed lifecycle events encoded for presentation |
| `src/runtime/bridge.rs` | In-process worker lifecycle and the existing terminal adapter |
| `src/runtime/transport.rs` | Bounded local event ingress and controlled writes |
| `src/runtime/auth.rs`, `logout.rs`, `oauth/` | Masked API keys, native Codex browser login, locked token refresh and local logout |
| `src/chat/` | Slash-command workflows and workspace session selection |
| `src/tui/` | Composer, modal, activity, Markdown, clipboard and terminal rendering |
| `src/update.rs` | Hibiscus update checks and checksum-verified installation |

A provider response is not executable until its authoritative completion event arrives and tool arguments are complete valid objects. The runtime persists the assistant tool calls, performs each validated tool call, records its outcome, then requests continuation. Cancellation closes the HTTP request or kills the shell group, records cancelled tool results where needed, clears queued input and settles the run. An accepted prompt is not a completed run.

The worker and TUI live in the same process. A private Unix socket carries bounded presentation/control records so the existing event-driven terminal behavior can be retained. No Pi or other agent process is started. Provider wire formats are confined to the provider adapter; session/tool data use native Rust enums.

Sessions have one runtime writer. Atomic replacement protects committed snapshots. On resume, any call without a recorded result is marked as interrupted with an unknown outcome; no tool is automatically retried. Compaction archives original history before replacing active context. Credentials are never written into sessions or provider error records.

Approval is checked before bash execution and is not a sandbox. Exact-command grants last only for the run. The persistent enabled/disabled preference lives in `approval.json`. Project instructions and plain-writing guidance enter the native system prompt independently of the approval setting.

Historical performance/profiling documents describe earlier Pi-backed versions and are not measurements of this runtime. Current tests and their limits are in [Development](development.md).
