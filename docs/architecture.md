# Architecture

Hibiscus is a Rust CLI and terminal frontend for a **Pi RPC subprocess**. It does not implement its own agent, model catalog, credential manager, tool runtime, or session store.

## What runs where

Hibiscus owns the terminal experience: CLI flags, chat commands, full-screen rendering, local image capture, update prompts, and recovery UI. Pi owns the agent: model access, authentication storage, tool execution, reasoning, retries, and saved session files. The main boundary is a long-lived `pi --mode rpc` child process using LF-delimited JSON records on stdin/stdout.

That boundary is intentional. When adding behavior, prefer asking Pi through RPC or its SDK helpers over duplicating Pi's agent, credential, model, tool, or session logic in Rust.

## Codebase map

```text
src/
  main.rs            CLI entry point, flags, one-shot/piped dispatch
  chat/
    mod.rs           Interactive chat loop, session switching, auth routing
    commands.rs      Supported slash-command suggestions
    models.rs        Model selection using Pi RPC
    sessions.rs      Pi session discovery, picker fallback, recent-history display
  pi/
    mod.rs           Pi integration and explicit approval-extension staging
    approval.mjs     Pre-execution bash approval and explicit goal checklist tool
    rpc.rs           Child process, JSONL transport, events, interrupts, progress
    error.rs         Typed errors, recovery disposition, safe presentation
    run.rs           Final run outcome across Pi retries and compaction
    auth.rs          Codex SDK helper discovery and OAuth UI bridge
    codex-auth.mjs    Embedded Node helper calling Pi ModelRuntime.login()
    logout.rs        In-chat provider picker, confirmation, helper lifecycle
    logout.mjs       Pi SDK credential metadata and logout protocol
    dialog.rs        Pi extension UI requests and responses
  tui/
    mod.rs           Terminal UI modules
    screen.rs        Alternate-screen view, composer, menus, frame scheduling
    paint.rs         Cached row-diff output, synchronized frames, cursor state
    activity.rs      Mutable display-only grouped tool/think timeline
    clipboard.rs     Bounded, cancellable local clipboard image reads
    terminal.rs      /dev/tty raw mode and input reader
    picker.rs        Shared bounded picker/scroll state
    modal.rs         Shared panel chrome, dialog focus/input/details and deadlines
    markdown.rs      Display-only Markdown and edit-diff formatting
    ui.rs            Labels/help and line-mode presentation
```

`tests/` contains mock Pi subprocess and PTY integration tests; unit tests live alongside the modules they cover. `benches/` contains repeatable formatter, session-list, and allocation benchmarks. Top-level scripts and docs cover installation, version bumps, development, release notes, and public contributor guidance.

The split keeps chat workflows separate from Pi protocol handling and terminal rendering. `modal.rs` renders the common panel used by model/session pickers and Pi dialogs. `Dialogs` asks the `ScrollDisplay` interface for a full-screen dialog response before falling back to line-mode terminal prompts; Screen owns modal focus and never mixes raw approval output with its composer.

## Runtime flow

A typical full-screen chat follows this path:

1. `main.rs` parses CLI flags and stages the bundled approval extension before launching Pi.
2. `chat::start_chat` opens a Pi RPC child, prepares terminal raw mode when available, creates a `Screen`, optionally restores session status/history, and enters the chat loop.
3. User input is handled as a local slash command or sent to Pi as a `prompt` command with optional image parts.
4. `pi/rpc.rs` writes JSONL commands to Pi, correlates responses by ID, and keeps reading events until the run settles.
5. TUI modules render assistant text, tool activity, approvals, model/session pickers, queued follow-ups, images, and recovery notices.
6. On exit or handoff to Pi's TUI, Hibiscus restores terminal state and closes/reopens RPC children deliberately to avoid concurrent session writers.

One-shot and piped modes use the same Pi RPC boundary but avoid the full-screen UI so stdout remains script-friendly.

## RPC data flow

```text
terminal input → chat commands → Pi RPC stdin (JSONL)
                               ← Pi RPC stdout (JSONL responses/events)
                               → terminal UI or plain stdout
Pi stderr → diagnostics (never parsed as protocol records)
```

`pi/rpc.rs` assigns IDs to commands and matches their `response` records. During full-screen runs it polls composer input alongside Pi stdout, sending additional prompts with explicit `streamingBehavior: "steer"` or `"followUp"`, distinct IDs, and optional images. Their responses acknowledge acceptance only. Pi's `queue_update` supplies queue counts and pending-message text, which Hibiscus docks above the composer (Waiting, then Sending if dequeued before delivery). The accepted message is not appended to chat until Pi emits `message_start` with `role: user`; skip the initial prompt's already-rendered user event. Since Pi queue events have no per-message IDs, match equal text by queue order and retain in-transit entries until delivery, cancellation, or settlement. Unsent or rejected drafts are preserved without overriding newer typing, and local slash commands are blocked during runs. A successful `prompt` response means **accepted**, not finished. It continues reading events through `agent_settled`; `agent_end` alone is not sufficient because Pi can retry or perform follow-up work. Records are framed by LF, not Unicode line separators. During a run, Esc sends `clear_queue` followed by `abort` and waits for their responses and settlement without discarding the RPC child.

Pi events supply progress and tool activity. Hibiscus correlates `tool_execution_start` and `tool_execution_end` by `toolCallId`. Successful built-in `edit` results may include `result.details.diff`; the UI displays a bounded preview of that authoritative diff, not a re-created diff or an inferred write. The display never prints raw reasoning deltas or full tool results by default.

## Tool approvals

Hibiscus stages its bundled `approval.mjs` in a private temporary directory and launches Pi with `--no-extensions --extension <trusted-file> --tools read,bash,edit,write,goal`. Pi's installed extensions are still disabled. Pi's `tool_call` event blocks a classified dangerous `bash` command until RPC `ctx.ui.select()` returns an explicit Allow or Always Allow. Approval is exact `(cwd, command)` and session-scoped. The existing extension UI subprotocol correlates responses by ID; empty/noninteractive/timeout/Esc deny. This is a best-effort policy for common commands, not a sandbox or guarantee that apparently safe shell commands cannot have side effects.

## Failure boundaries

Provider failures and command rejections are typed separately from transport/protocol failures. The chat loop catches recoverable errors per interaction; one-shot and piped runs retain failing exit statuses. A failed assistant attempt can be superseded by successful Pi retry/compaction recovery. The final outcome is interpreted only at settlement, and Hibiscus never runs its own automatic retry loop.

`/restore` stages a failed prompt with its images for review. `/reconnect` is explicit, stops/reaps the old Pi process group, and uses the last confirmed session path without replaying requests. Broken transports leave a disconnected composer available for local commands. See [Error handling and recovery](error-handling.md) for the full event matrix and timeout behavior.

## Sessions, models, and credentials

Pi persists sessions in its JSONL store. `chat/sessions.rs` discovers files for the current workspace, respecting Pi's session directory settings. Resume/switch obtains messages with `get_messages` and displays only the latest five user turns and visible assistant replies; Pi still retains full history. In-chat switches use `switch_session` on the existing RPC child.

`chat/models.rs` obtains the active model and all configured models from Pi, sorts them by provider then ID for display, and sends `set_model` with both provider and ID for a changed selection. Hibiscus does not maintain a second model catalog. Slash-command suggestions contain only commands implemented by Hibiscus, not arbitrary Pi commands.

Pi RPC has no login/logout command. After an explicit Codex choice in the full-screen `/login` provider picker (independent of the active model), Hibiscus launches a small Node helper using the **installed Pi SDK's `ModelRuntime.login("openai-codex", "oauth", interaction)`**. It relays UI events over JSONL; Pi performs OAuth and writes its own credential store. Hibiscus never serializes returned credentials. Manual authorization responses are masked and excluded from chat history. The helper's `done` event is sent only after SDK login and credential synchronization finish; Rust treats that event—not natural Node process exit—as completion. It then terminates and reaps the dedicated helper rather than waiting for its HTTP callback sockets/browser connection to close, which can otherwise leave Node alive after the listener is closed. After successful login, Hibiscus gracefully closes the idle RPC child before opening the same saved session in a fresh child; for an unsaved/empty chat it starts a fresh session. This avoids concurrent session writers and stale in-process auth state.

Full-screen `/logout` uses a separate SDK helper to list stored credential metadata, then accepts an explicit provider selection and removal confirmation. The old RPC child is closed before the helper calls `ModelRuntime.logout`; the session is reopened afterward even if credential mutation cannot be confirmed. Cancellation before authorization leaves both credentials and the running backend alone. No credential values are serialized or edited by Rust, and unavailable SDK/line mode explains the limitation instead of automatically handing off.

For other-provider login, line-mode login, or login with an unavailable Node/SDK helper, the Pi TUI handoff remains. Hibiscus first closes the idle RPC child, stops its terminal reader, and restores terminal mode and screen. Pi opens the saved session if one exists, or uses `--no-session` for an empty chat. On Pi exit Hibiscus restores the terminal and reopens its RPC child. No saved message is required, and the same JSONL session is never opened by two Pi children simultaneously. See [Terminal UI](terminal.md#authentication-handoff).

## Terminal modes

Interactive chat with compatible stdin/stdout uses a full-screen renderer with a fixed composer, transcript, and inline pickers. Pi still provides agent/tool behavior; Hibiscus formats the display only. Small or `TERM=dumb` terminals fall back to line mode. Piped input stays plain and script-friendly. The alternate screen and mouse reporting are disabled on exit and during the Pi TUI authentication fallback.

## Where to make changes

| If you are changing... | Start in... |
| --- | --- |
| CLI flags, one-shot prompts, top-level modes | `src/main.rs` |
| Interactive chat flow or slash-command behavior | `src/chat/mod.rs`, `src/chat/commands.rs` |
| Model selection | `src/chat/models.rs` |
| Session discovery, resume, or recent-history display | `src/chat/sessions.rs` |
| Pi RPC transport, event handling, steering, aborts, settlement | `src/pi/rpc.rs`, `src/pi/run.rs` |
| Error classification and recovery hints | `src/pi/error.rs`, `docs/error-handling.md` |
| Tool approval policy or Pi dialog bridging | `src/pi/approval.mjs`, `src/pi/dialog.rs` |
| Codex login or provider logout | `src/pi/auth.rs`, `src/pi/logout.rs`, `src/pi/*.mjs` helpers |
| Full-screen rendering, composer, queues, modals | `src/tui/screen.rs`, `src/tui/modal.rs`, `src/tui/paint.rs` |
| Tool/activity display | `src/tui/activity.rs` |
| Markdown/diff formatting | `src/tui/markdown.rs` |
| Clipboard image paste | `src/tui/clipboard.rs` |
| Raw terminal lifecycle and input reader | `src/tui/terminal.rs` |
| Self-update behavior | `src/update.rs`, `install.sh` |
| Installer/version bump/release process | `install.sh`, `scripts/bump-version.sh`, `.github/workflows/` |

Read [Development](development.md) before modifying the RPC protocol or terminal lifecycle; both have mock subprocess and PTY regression tests. Add or update targeted tests when changing protocol, recovery, session, approval, or terminal behavior.
