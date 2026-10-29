# Durable project memory

## Architecture and ownership

The owner explicitly approved removing both the Pi SDK and Pi runtime on 2026-10-29. Hibiscus is a standalone Rust coding agent. It owns providers, authentication, tools, sessions, context management and terminal presentation. Do not restore another agent dependency, read another agent's credentials implicitly, or treat an external SDK as authoritative for native behavior. Source and tests, not historical records, establish current implementation.

`src/runtime/` owns native configuration, provider adapters, typed messages/events, tools and the agent loop. `src/chat/` owns slash workflows; `src/tui/` owns presentation. The existing bounded UI adapter uses a private in-process socket. External provider wire formats belong only in the provider adapter; native lifecycle events are encoded separately for the UI.

## Execution and recovery

- Execute tool calls only after authoritative provider completion and complete validated argument objects. Never act on partial SSE arguments or an incomplete/truncated response. Tool results retain call IDs. Unknown tools and ambiguous edits fail before mutation. Evidence: tests/native_runtime.rs.
- Treat authoritative completion as sufficient; do not wait for natural socket closure. Responses can keep HTTP connections open after completion. Regression: authoritative_completion_does_not_wait_for_http_socket_close.
- Prompt acceptance is not completion. Settle after tool results and queued delivery. Steering enters at a completed tool boundary; followups wait for the turn. Esc clears queues and cancels HTTP/tool work without replay. Failed queued delivery restores the newest undelivered input for review. Never resend implicitly.
- A resumed session with an unmatched tool call records unknown outcome instead of executing it. Files may already have changed before interruption; inspect before retrying. Compaction archives original history and changes active context only after a usable summary. Failed/cancelled summaries preserve history.
- Cloned Unix sockets share O_NONBLOCK. The native writer needs nonblocking I/O; its dedicated reader must poll readiness rather than interpret WouldBlock as EOF. Preserve the wake-generation mechanism against lost notifications. Tests exercise the actual binary without external agent/runtime programs.
- Keep terminal I/O failures distinct from recoverable provider/helper errors. Interactive errors keep chat usable when possible; one-shot/piped failures exit nonzero. /restore is review, not resubmission; /reconnect restarts local runtime state without replay.

## Credentials and data

All app-owned data lives under ~/.hibiscus or an absolute HIBISCUS_HOME. Native config defines provider endpoints, models, capabilities and iteration limits. Stored keys are masked at entry and written atomically with mode 0600. Auth files must be regular/private. Configured environment keys take precedence; logout removes only the stored key and does not revoke provider access or unset environment variables.

Do not silently reuse Pi/Codex/Claude token stores. Native Codex browser login lives in runtime/oauth and follows the legacy public-client Codex authorization-code flow. It needs no personal client ID: the built-in public identifier names the OAuth client, while the refresh token renews access. Installed Pi 1.1.0 has a separate newer openai-chatgpt flow with dynamic client registration; do not confuse its endpoints, scopes or token audience with legacy openai-codex. Hibiscus owns callback handling, PKCE/state validation and private codex-auth.json storage. Require the full matching redirect URL for manual input; never accept a bare code without state. Serialize rotating refreshes across processes and preserve the old credential on errors or account changes. Subscription tokens are pinned to the Codex Responses endpoint; never send them to arbitrary configured providers. Logout is local removal, not remote revocation. Tests in runtime/oauth use synthetic tokens and local HTTP, not real-account acceptance. Existing data is not imported automatically. Legacy config must receive explicit migration guidance. Native session formats are versioned and workspace-scoped.

HTTPS is required except for loopback providers/tests; redirects are disabled. Provider error bodies and keys never enter user-visible errors. Missing usage/context/pricing stays unknown. Anthropic's context includes cache-read/creation input in addition to uncached input; final receipts override provisional counts. Do not infer provider limits or quotas from a model name.

## Tools, policy and UI

The native tools are read, write, edit, bash and goal. Read is bounded UTF-8 regular-file access. Edit requires one exact match and reports a diff after successful writing. Shell output/time are bounded and cancellation kills/reaps its process group. Goal percentages come from an explicit checklist, not tool counts. /loop remains removed.

Shell approvals default to Deny. With approvals enabled, every bash call requires authorization; Always Allow covers only the exact displayed command for that run. The persistent enable/disable preference belongs to Hibiscus. It is not a sandbox; file tools are not approval-gated. Writing guidance and project instructions remain independent of approval settings. Project skill catalogs point to instructions that must be read before applying them.

Preserve the user's aubergine UI, visible steady composer cursor, separate activity flower, docked shared modals and idle usage footer with Goal at the right edge. Slash commands and model-selection confirmations stay out of chat. Keep NO_COLOR and plain piped output.

Preserve bounded bracketed paste, literal @path insertion without silent file upload, staged image ownership, tool-call correlation and real edit diffs. Transcript copy uses desktop helpers or bounded OSC 52 without unverified success claims. Unicode cell-width and off-screen selection limits remain documented. Supported native Mermaid graphs render only after settlement; unknown graphs remain source.

PTY tests must set terminal dimensions, drain concurrently, reconstruct row-diff views and bound cleanup. Prefer tests/support to fixed sleeps. Mock clipboard programs on every relevant platform; never use a real clipboard or credential store just to run CI. Local HTTP fixtures use synthetic keys and temporary workspaces, not billable providers. Historical Pi tests/profiling results are not native acceptance evidence.

## Evidence and consent

Check current source before trusting a technical memory. Record test outcomes/limits in STATE, durable root causes here, release history in Git/changelog. A mock pass is not real-account or physical-terminal verification. Do not commit credentials, private conversations or machine-specific paths.

Work locally by default. Implementing/testing does not authorize a push, remote PR, release, registry publication or deployment. Those require an explicit action and destination. Git tags, GitHub Releases and npm publication are separate events.
