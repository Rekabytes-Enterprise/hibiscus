# Development

Install Rust/Cargo and a C linker. Pi and Node are not agent dependencies. The npm installer test requires Node/npm, shell tests require standard Unix tools, and coding-evaluation development tests require Python 3.9 or newer. Python is not required by the shipped Hibiscus executable. Provider tests use local HTTP servers and synthetic keys. Extension tests use local `/bin/sh` fixtures; the optional Python example requires Python 3 only when invoked.

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
sh -n install.sh
sh tests/install.sh
sh tests/bump-version.sh
node tests/npm-install.mjs
```

Use `cargo run --locked --` to exercise the working tree. Reinstall with `cargo install --path . --locked` only when you want to replace your installed executable. Config and session formats are described in [Native migration](native-runtime.md).

`tests/native_runtime.rs` exercises the actual binary through local HTTP endpoints and controlling PTYs. Coverage includes native provider streaming/tool calls, real temporary file mutations, direct shell execution and process-group cancellation, stream interruption, session resume, compaction archives, queued messages, API-key masking and logout. It explicitly supplies nonexistent Pi/Node paths; one test removes all external programs from PATH. No real provider credential or billable request is used.

Unit tests retain the CLI, TUI, Markdown, transport pressure, queue ordering and recovery contracts. View-specific integration tests live under `tests/native_runtime/` and share the parent fixture. See the [presentation module map](development/presentation-architecture.md) before changing a frontend. Old Pi-only mock-process fixtures were retired because they no longer test the executable's runtime. Their source remains in Git history. The shared PTY harness drains concurrently, reconstructs row-diff output and bounds waits/cleanup; use it instead of fixed sleeps and unbounded child waits.

CI targets pushes and PRs to `dev`. Formatting, Clippy, shell/npm installer checks and Rust tests must pass. Release workflows publish only explicitly tagged revisions; local implementation does not authorize a push, release or deployment.

`cargo test --locked` also runs the [coding-evaluation runner tests](evaluations.md) against the freshly built binary through `tests/evaluation.rs`. The runner is standard-library Python under `tools/evaluation`, separate from runtime/provider code. See its [design and ownership](development/evaluations.md) before adding experiment options.

For manual acceptance, use an intentionally configured provider and review possible API charges first. Check streaming, a file edit, shell deny/allow, Esc during a request/tool, model selection, login/logout, `/continue`, `/sessions`, queued steering/followups and `/compact`. Verify the physical terminal's clipboard and rendering separately. Never publish credentials, authorization details, clipboard contents or private conversations in evidence.

Extension tests cover metadata-only loading, malformed manifests/schemas/replies, tool argument validation before execution, all provider adapters, local command completion/reload, agent-authored source creation, immediate registration, private file permissions, output/time limits and descendant cancellation. See [extension authoring](extensions.md).
