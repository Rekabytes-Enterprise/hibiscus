# Architecture

Hibiscus is a Rust CLI and terminal frontend for a **Pi RPC subprocess**. It does not implement its own agent, model catalog, credential manager, tool runtime, or session store.

## Source layout

```text
src/
  main.rs            CLI entry point, flags, one-shot/piped dispatch
  chat/
    mod.rs           Interactive chat loop, session switching, auth routing
    commands.rs      Supported slash-command suggestions
    models.rs        Model selection using Pi RPC
    sessions.rs      Pi session discovery, picker fallback, recent-history display
  pi/
    mod.rs           Pi integration modules
    rpc.rs           Child process, JSONL transport, events, interrupts, progress
    auth.rs          Codex SDK helper discovery and OAuth UI bridge
    codex-auth.mjs   Embedded Node helper calling Pi ModelRuntime.login()
    dialog.rs        Pi extension UI requests and responses
  tui/
    mod.rs           Terminal UI modules
    screen.rs        Alternate-screen renderer, composer, menus, activity
    terminal.rs      /dev/tty raw mode and input reader
    picker.rs        Shared bounded picker/scroll state
    markdown.rs      Display-only Markdown and edit-diff formatting
    ui.rs            Labels/help and line-mode presentation
```

`tests/` contains mock Pi subprocess and PTY integration tests; unit tests live alongside the modules they cover. The split keeps chat workflows separate from Pi protocol handling and terminal rendering.

## RPC data flow

```text
terminal input → chat commands → Pi RPC stdin (JSONL)
                               ← Pi RPC stdout (JSONL responses/events)
                               → terminal UI or plain stdout
Pi stderr → diagnostics (never parsed as protocol records)
```

`pi/rpc.rs` assigns IDs to commands and matches their `response` records. A successful `prompt` response means **accepted**, not finished. It continues reading events through `agent_settled`; `agent_end` alone is not sufficient because Pi can retry or perform follow-up work. Records are framed by LF, not Unicode line separators. During a run, Esc sends `clear_queue` followed by `abort` and waits for their responses and settlement without discarding the RPC child.

Pi events supply progress and tool activity. Hibiscus correlates `tool_execution_start` and `tool_execution_end` by `toolCallId`. Successful built-in `edit` results may include `result.details.diff`; the UI displays a bounded preview of that authoritative diff, not a re-created diff or an inferred write. The display never prints raw reasoning deltas or full tool results by default.

## Sessions, models, and credentials

Pi persists sessions in its JSONL store. `chat/sessions.rs` discovers files for the current workspace, respecting Pi's session directory settings. Resume/switch obtains messages with `get_messages` and displays only the latest five user turns and visible assistant replies; Pi still retains full history. In-chat switches use `switch_session` on the existing RPC child.

`chat/models.rs` obtains the active model and available models from Pi, then sends `set_model` for a changed selection. Hibiscus does not maintain a second model catalog. Slash-command suggestions contain only commands implemented by Hibiscus, not arbitrary Pi commands.

Pi RPC has no login/logout command. After an explicit Codex choice in the full-screen `/login` provider picker (independent of the active model), Hibiscus launches a small Node helper using the **installed Pi SDK's `ModelRuntime.login("openai-codex", "oauth", interaction)`**. It relays UI events over JSONL; Pi performs OAuth and writes its own credential store. Hibiscus never serializes returned credentials. Manual authorization responses are masked and excluded from chat history. The helper's `done` event is sent only after SDK login and credential synchronization finish; Rust treats that event—not natural Node process exit—as completion. It then terminates and reaps the dedicated helper rather than waiting for its HTTP callback sockets/browser connection to close, which can otherwise leave Node alive after the listener is closed. After successful login, Hibiscus gracefully closes the idle RPC child before opening the same saved session in a fresh child; for an unsaved/empty chat it starts a fresh session. This avoids concurrent session writers and stale in-process auth state.

For other providers, `/logout`, line mode, or an unavailable Node/SDK helper, the Pi TUI handoff remains. Hibiscus first closes the idle RPC child, stops its terminal reader, and restores terminal mode and screen. Pi opens the saved session if one exists, or uses `--no-session` for an empty chat. On Pi exit Hibiscus restores the terminal and reopens its RPC child. No saved message is required, and the same JSONL session is never opened by two Pi children simultaneously. See [Terminal UI](terminal.md#authentication-handoff).

## Terminal modes

Interactive chat with compatible stdin/stdout uses a full-screen renderer with a fixed composer, transcript, and inline pickers. Pi still provides agent/tool behavior; Hibiscus formats the display only. Small or `TERM=dumb` terminals fall back to line mode. Piped input stays plain and script-friendly. The alternate screen and mouse reporting are disabled on exit and during the Pi TUI authentication fallback.

Read [Development](development.md) before modifying the RPC protocol or terminal lifecycle; both have mock subprocess and PTY regression tests.
