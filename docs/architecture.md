# Architecture

Hibiscus is a standalone Rust coding agent. It owns provider connections, tool execution, credentials, sessions and terminal presentation. See [native migration](native-runtime.md) for configuration and compatibility changes.

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Binary entry point and top-level error reporting |
| `src/lib.rs`, `src/args.rs` | Private module graph and shared startup; command-line arguments and `--help` text |
| `src/runtime/config.rs` | Validate provider/model configuration, private atomic storage |
| `src/runtime/provider/` | Provider requests for Responses, Chat Completions and Anthropic: request bodies (`wire.rs`), sending (`http.rs`), SSE framing (`sse.rs`) and stream parsing (`stream.rs`) |
| `src/runtime/session.rs` | Typed messages, versioned sessions, resume and interrupted-call recovery |
| `src/runtime/tools.rs` | Native file, shell and checklist tools |
| `src/runtime/engine.rs` | Model/tool loop, queueing, cancellation and compaction |
| `src/runtime/policy.rs` | Project/personal instructions, project skill catalog and writing guidance |
| `src/runtime/event.rs` | Typed lifecycle events encoded for presentation |
| `src/runtime/bridge/` | In-process worker lifecycle and the terminal adapter: `Client` (`mod.rs`), one turn (`turn.rs`), transcript rendering (`display.rs`), keys during a run (`live_input.rs`) and JSONL records (`wire.rs`) |
| `src/runtime/transport.rs` | Bounded local event ingress and controlled writes |
| `src/runtime/auth.rs`, `logout.rs`, `oauth/` | API-key storage, Codex browser sign-in steps and locked token refresh, credential removal. No terminal UI |
| `src/extensions/` | Agent creation/inspection/reload tools, shared catalog snapshots, manifest validation and executable protocol |
| `src/process.rs` | Bounded stdin/stdout/stderr, deadlines and process-group cancellation shared by shell tools and extensions |
| `src/chat/` | Slash-command workflows, the `/login` and `/logout` prompts, saved-session selection, and the native command catalog (`commands.rs`) shared by routing, completion and extension collision checks |
| `src/cli/` | Inline rendering, multiline editor, tool rows and CLI styling |
| `src/tui/` | Fullscreen composer, modal, activity, Markdown, clipboard and painting |
| `src/ui/` | Shared terminal services, display contracts, extension dialogs and frontend handover |
| `src/update.rs` | Hibiscus update checks and checksum-verified installation |

The [presentation module map](development/presentation-architecture.md) explains CLI/TUI ownership, the shared frontend and regression coverage.

A provider response is not executable until its authoritative completion event arrives and tool arguments are complete valid objects. The runtime persists the assistant tool calls, performs each validated tool call, records its outcome, then requests continuation. Cancellation closes the HTTP request or kills the shell group, records cancelled tool results where needed, clears queued input and settles the run. An accepted prompt is not a completed run.

The worker and both terminal views live in the same process. A private Unix socket carries bounded presentation/control records so the existing event-driven terminal behavior can be retained. No Pi or other agent process is started. Provider wire formats are confined to the provider adapter; session/tool data use native Rust enums.

Sessions have one runtime writer. Atomic replacement protects committed snapshots. On resume, any call without a recorded result is marked as interrupted with an unknown outcome; no tool is automatically retried. Compaction archives original history before replacing active context. Authentication handlers keep stored credential values and raw provider error bodies out of session messages. Tool output and files can still contain secrets and enter saved conversations, so keep sessions private and review them before sharing.

Native tools execute directly after argument validation and authoritative provider completion. Shell commands do not wait for a confirmation in either interactive or piped mode. Tools are not sandboxed. Project instructions and plain-writing guidance enter the native system prompt.

Historical performance/profiling documents describe earlier Pi-backed versions and are not measurements of this runtime. Current tests and their limits are in [Development](development.md).

The [extension API](extensions.md) registers tools into the same authoritative-completion execution path and provider catalog. Local extension commands use correlated cancellable runtime requests without creating model messages. Malformed manifests are reported without executing code.
