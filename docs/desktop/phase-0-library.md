# Phase 0: library API

Status: **done** (`src/desktop.rs`, `src/runtime/embed.rs`, `tests/desktop_api.rs`).

Goal: let a program other than the terminal UI start the engine, send commands and read events. **No user-visible change.** The CLI and TUI keep their current behaviour and tests.

Back to the [desktop plan](README.md).

## Where things are today

- `src/lib.rs` exports only `hibiscus::run()`; every module is private.
- `runtime::bridge::Client::start_with_diagnostics` loads the config, opens the session, creates a `UnixStream::pair()` and runs `runtime::engine::serve(worker_socket, config, session)` on a thread. The engine side is already UI-independent: it reads JSON-lines commands and writes JSON-lines events.
- The client side (`Client::command`, `exchange`) is tied to the terminal: it takes a `ScrollDisplay`, `RawMode`, `Dialogs` and a key-event receiver.
- Login runs outside the engine. `runtime::auth::sign_in` takes the terminal `Frontend` and asks for the key itself; the `openai-codex` OAuth flow (`runtime::oauth::sign_in`) does the same.
- The saved-session list is `chat::sessions::list(cwd)`.

Unix sockets, `/bin/sh`, process groups and `0600` files all work on macOS and Linux, so **nothing needs porting** in this phase.

## Changes

1. **Public engine handle** in a new `src/desktop.rs` (only module made public):
   ```rust
   pub struct Engine { /* worker thread, socket, Inbox, PipeWriter */ }
   pub enum Start { New, Latest, Selected(PathBuf) }

   impl Engine {
       pub fn start(start: Start, workspace: PathBuf) -> Result<Engine>;
       pub fn send(&mut self, command: serde_json::Value) -> Result<String>; // returns the request id
       pub fn next_event(&mut self, timeout: Duration) -> Result<Option<serde_json::Value>>;
       pub fn shutdown(self) -> Result<()>;
   }
   ```
   It reuses the startup code from `Client::start_with_diagnostics` (move that into a shared function both call) and the existing `transport::Inbox` / `PipeWriter`, so the byte and record limits, write deadlines and shutdown behaviour stay the same.
2. **Login without a terminal.** Split `auth::sign_in` into the prompt (stays in the TUI) and a public `desktop::save_api_key(provider, key)` that keeps today's validation (non-empty, ≤ 8192 bytes, no control characters, known API-key provider) and the atomic `auth.json` write. Also expose `desktop::key_page(provider)` for the "create one at …" link.
   - `openai-codex` OAuth stays TUI-only for now; it is not needed for API-key providers.
3. **Project folder as a parameter.** The engine takes its working folder from the process (`std::env::current_dir()` in `session.rs`). A desktop app is launched from the Dock or a menu, so its current folder is meaningless, and it is shared by every window. `Engine::start` takes the folder explicitly (`start(start, workspace: PathBuf)`), and session open, tools and file paths use that value. The CLI keeps passing `current_dir()`, so its behaviour does not change. Check every `current_dir()` call under `src/runtime/` while doing this.
4. **Session list:** `desktop::sessions(cwd) -> Vec<SessionInfo>` wrapping `chat::sessions::list`, returning a public `SavedSession { path, title, modified }`. The desktop glue decides how to serialise it.
5. **Protocol doc:** a new `docs/desktop/protocol.md` listing the commands the engine accepts (`prompt`, `abort`, `get_available_models`, `set_model`, `set_thinking_level`, `new_session`, `switch_session`, `compact`, `get_messages`, `get_session_stats`, `extension_command`, …) and the events it sends (`message_start`/`message_update`/`message_end`, `tool_execution_start`/`end`, `queue_update`, `provider_error`, `extension_ui_request`, `compaction_start`/`end`, `agent_settled`, `response`). Written from `engine.rs`, not invented.

## Out of scope

- Any Tauri code, frontend or packaging (Phase 1).
- Windows support.
- Changing the protocol itself. If the desktop app needs a new command, add it in Phase 1 with a test.
- Anything in `runtime/rooxcode.rs` or the Rooxcode Gateway.

## Tests

- Integration test: two engines started with different `workspace` folders in one process each see only their own folder.
- Integration test: `Engine::start(Start::New, dir)` against the existing fake provider, send a `prompt`, read events until `agent_settled`, check the reply text, then `shutdown` with no thread left running.
- Integration test: `abort` during a streaming reply settles the run.
- Unit tests for `save_api_key`: rejects empty, oversized, control-character and unknown-provider input; writes `auth.json` with `0600`.
- The full existing suite and `cargo clippy --all-targets -- -D warnings` pass unchanged.

## Done when

- `hibiscus::desktop` compiles as a library API and the tests above pass.
- `hibiscus`, `hibiscus --cli` and `hibiscus --tui` behave exactly as before.
- `protocol.md` exists and matches `engine.rs`.

Estimated size: small to medium. Most of it moves existing code behind a public function; passing the project folder explicitly touches the most files.
