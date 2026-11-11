# Engine protocol

The records exchanged with `hibiscus::desktop::Engine`. They are the same JSON records the TUI exchanges with the engine (`src/engine/agent.rs`, `src/engine/event.rs`). This page describes the current behaviour; the code is the authority.

Back to the [desktop plan](README.md).

## Rust API

```rust
use hibiscus::desktop::{self, Engine, Start};

let mut engine = Engine::start(Start::New, Path::new("/path/to/project"))?;
let id = engine.send(json!({"type": "prompt", "message": "hello"}))?;
while let Some(event) = engine.next_event(Duration::from_millis(100))? { /* … */ }
engine.shutdown();
```

- `Engine::start(start, workspace)`: `Start::New`, `Start::Latest` (newest session for the workspace, else new) or `Start::Selected(path)`. The session and every tool work in `workspace`. Several engines can run in one process, each in its own workspace.
- `send(command)` sets the command's `id` and returns it. Fails if `type` is missing or the engine is shut down.
- `next_event(timeout)` returns `Ok(None)` on timeout and an error once the engine has stopped.
- `shutdown()` (or dropping the engine) stops the run and waits for its tools to stop.
- `desktop::save_api_key(provider, key)` stores a key in `auth.json` (`0600`) after the same checks as `/login`. Start a new engine afterwards, as the TUI reconnects after `/login`.
- `desktop::key_page(provider)` gives the page where a built-in provider's key is created.
- `desktop::sign_out(provider)` removes a stored API key, or the Codex login for `openai-codex`.
- `desktop::training_denied()` / `desktop::set_training_denied(deny)` read and save the "don't let providers train on my prompts" choice in `privacy.json` in the Hibiscus home. While it is on, OpenRouter requests carry `"provider": {"data_collection": "deny"}`. Engines read it on every request, in the terminal app too, so no restart is needed.
- `desktop::CodexSignIn::start()` listens on `localhost:1455` and builds the Codex sign-in link (`url()`). Open it in the browser, then await `finish()`. `paste()` takes the redirect URL by hand when the browser cannot reach the app. Dropping `finish()` cancels and frees the port; a failed or cancelled sign-in keeps the previous login. It times out after 10 minutes.
- `desktop::sessions(workspace)` lists saved sessions (path, title, modified time), newest first.

The transport keeps the TUI's limits: `HIBISCUS_RPC_BUFFER_MIB` (default 64) and `HIBISCUS_RPC_MAX_RECORDS` (default 4096) bound queued events, and writes time out after 30 seconds.

## Commands

Every command gets exactly one `response` with the same `id`:
`{"type":"response","id":…,"success":true,"data":{…}}` or `{"type":"response","id":…,"success":false,"error":"…"}`.

| `type` | Fields | Notes |
|---|---|---|
| `prompt` | `message`, optional `images: [{mimeType, data}]` (base64), optional `streamingBehavior` | While a run is active, needs `streamingBehavior: "steer"` or `"followUp"`; otherwise fails with "Agent is busy". At most 16 queued. |
| `abort` | | Cancels the active run. The run still ends with `agent_settled`. |
| `clear_queue` | | Drops queued prompts. |
| `get_state` | | Session state, including extension commands. |
| `get_messages` | optional `limit`, `before` | Without `limit`, `data.messages` is the whole conversation. With `limit` (at most 1000), it's one page for the desktop chat: up to `limit` messages ending before position `before` (default: the end), moved back so it never starts with a tool result. `data.start` is the page's first position, `data.total` the conversation's length, and `data.edited` the files written or edited without error, newest first. In a page, tool output over 2 KB is cut (`trimmed: true`), edit diffs over 16 KB are left out, tool images are left out (`omittedImages: n`), and prompt images keep only their type. |
| `get_tool_result` | `toolCallId` | `data`: that tool result in full, as stored. For rows a page trimmed. |
| `get_tool_outputs` / `get_tool_output` | `index` (1-based, optional) | Stored tool output. |
| `get_session_stats` | | Token usage and context. |
| `get_available_models` | | `data.models: [{provider, id, contextWindow, reasoning, images}]` for providers the user can use; `data.catalogError` if a catalog refresh failed. Refreshes catalogs when idle. |
| `get_available_thinking_levels` | | Levels for the current model. |
| `set_model` | `provider`, `modelId` | Not during a run. |
| `set_thinking_level` | `level` | Not during a run. |
| `new_session` | | Not during a run. Same workspace. |
| `switch_session` | `sessionPath` | Not during a run. Must belong to the same workspace. |
| `set_session_name` | `name` | Not during a run. Names the open session; an empty name restores the automatic title. |
| `compact` | optional `customInstructions` | Not during a run. |
| `extension_command` | `name`, `input` | Not during a run. Output arrives as `extension_command_output`. |

Commands marked "not during a run" fail with "Command unavailable while a run is active" instead of waiting.

## Events

| `type` | Fields | Meaning |
|---|---|---|
| `agent_start` | | A run started. |
| `message_start` / `message_end` | `message` | A message began / finished. A failed run ends with `message_end` whose message has `stopReason: "error"` and `errorMessage`. |
| `message_update` | `assistantMessageEvent: {type: "text_delta", delta}` | Streamed reply text. |
| `thinking_reset` | | Discard any shown thinking text. |
| `tool_execution_start` | `toolCallId`, `toolName`, `args` | A tool started. |
| `tool_execution_end` | `toolCallId`, `toolName`, `isError`, `result`, `durationMs` | A tool finished. |
| `queue_update` | `steering: [text]`, `followUp: [text]` | Queued prompts changed. |
| `queue_rejected` | `message`, `images`, `steer`, `error` | A queued prompt was not delivered because the run failed. Show it so the user can resend it. |
| `auto_retry_start` / `auto_retry_end` | `success` | The engine is retrying a provider error. |
| `compaction_start` / `compaction_end` | `result` or `errorMessage` | Context compaction. |
| `provider_status` | `provider`, `waiting` | The Rooxcode Gateway is queueing the request. |
| `provider_error` | `error` | A provider or catalog problem, safe to show as is. |
| `extension_error` | `error` | An extension failed to load. |
| `extension_command_output` | `text` | Output of `extension_command`. |
| `agent_settled` | | The run and its queued prompts are finished. Commands marked "not during a run" work again. |

The native engine does not send `extension_ui_request` dialogs today; the TUI bridge still handles them for compatibility. A desktop app does not need them until the engine sends them.

Error texts are already redacted by the engine (`error::safe_message`) and can be shown directly.
