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

- `Engine::start(start, workspace)`: `Start::New` (Chat), `Start::Canvas`, `Start::Latest` (newest session for the workspace, else new) or `Start::Selected(path)`. The session and every tool work in `workspace`. Several engines can run in one process, each in its own workspace.
- `send(command)` sets the command's `id` and returns it. Fails if `type` is missing or the engine is shut down.
- `next_event(timeout)` returns `Ok(None)` on timeout and an error once the engine has stopped.
- `shutdown()` (or dropping the engine) stops the run and waits for its tools to stop.
- `desktop::save_api_key(provider, key)` stores a key in `auth.json` (`0600`) after the same checks as `/login`. Start a new engine afterwards, as the TUI reconnects after `/login`.
- `desktop::key_page(provider)` gives the page where a built-in provider's key is created.
- `desktop::sign_out(provider)` removes a stored API key, the ChatGPT plan login for `openai-chatgpt`, or the Reka Bytes session for `rooxcode` (also revoked on the gateway when it can be reached).
- `desktop::RekaSignIn::start().await` starts Reka Bytes sign-in for Rooxcode models: it asks the gateway for the issuer and listens on `127.0.0.1` port 14565, 14566 or 14567. Open `url()`, then await `finish()`; `paste()` takes the redirect URL by hand. It times out after 10 minutes and saves `rooxcode-auth.json`, which the terminal app shares.
- `desktop::reka_account()` gives the signed-in account's name, email and avatar (the pixel character picked on the Rooxcode account page), never its tokens. `desktop::reka_profile()` reloads them from the gateway's `/me` first and keeps the saved ones when it can't be reached. `desktop::reka_usage()` loads the Settings → Account page from the same `/me`: each budget window as a percent with its refresh time, the wallet credits and this device's name. Budget amounts stay in the engine. Engines read the session on every Rooxcode request, so no restart is needed after sign-in.
- `desktop::wsl::detect()` lists the WSL 2 distros on a Windows computer (from `wsl.exe --list --verbose`, or the registry when that fails), without Docker's internal ones; it is empty elsewhere. `desktop::wsl::connect(folder, distro)` makes the shell tool and dev server for that folder run inside the distro, and maps Linux paths in tool calls to Windows ones; `None` disconnects. Stopping a command kills its Linux process group too. `desktop::wsl::home(distro)` gives the distro's home as a `\\wsl.localhost` path for the folder picker. The app remembers each folder's choice and connects again at startup (#201). A folder inside a distro (`\\wsl.localhost\<distro>\...` or `\\wsl$\<distro>\...`) always runs in that distro without connecting, and the open-folder and add-folder pickers start in the distro's home while WSL is on (#206).
- `desktop::REKA_SIGN_IN` is the error a Rooxcode `prompt` gets without a session. The prompt was not added to the conversation, so it can be sent again after sign-in.
- `desktop::training_denied()` / `desktop::set_training_denied(deny)` read and save the "don't let providers train on my prompts" choice in `privacy.json` in the Hibiscus home. While it is on, OpenRouter requests carry `"provider": {"data_collection": "deny"}`. Engines read it on every request, in the terminal app too, so no restart is needed.
- `desktop::ChatGptSignIn::start()` chooses an available loopback port and builds the ChatGPT sign-in link (`url()`). Open it in the browser, then await `finish()`. `paste()` takes the redirect URL by hand when the browser cannot reach the app. Dropping `finish()` cancels and frees the port; a failed or cancelled sign-in keeps the previous login. It times out after 10 minutes.
- `desktop::sessions(workspace)` lists saved sessions (path, title, kind, modified time), newest first.

The transport keeps the TUI's limits: `HIBISCUS_RPC_BUFFER_MIB` (default 64) and `HIBISCUS_RPC_MAX_RECORDS` (default 4096) bound queued events, and writes time out after 30 seconds.

## Commands

Every command gets exactly one `response` with the same `id`:
`{"type":"response","id":…,"success":true,"data":{…}}` or `{"type":"response","id":…,"success":false,"error":"…"}`.

| `type` | Fields | Notes |
|---|---|---|
| `prompt` | `message`, optional `images: [{mimeType, data}]` (base64), optional `streamingBehavior` | While a run is active, needs `streamingBehavior: "steer"` or `"followUp"`; otherwise fails with "Agent is busy". At most 16 queued. |
| `abort` | | Cancels the active run. The run still ends with `agent_settled`. |
| `clear_queue` | | Drops queued prompts. |
| `get_state` | | Session state, including `sessionKind` (`chat` or `canvas`) and extension commands. |
| `get_messages` | optional `limit`, `before` | Without `limit`, `data.messages` is the whole conversation. With `limit` (at most 1000), it's one page for the desktop chat: up to `limit` messages ending before position `before` (default: the end), moved back so it never starts with a tool result. `data.start` is the page's first position, `data.total` the conversation's length, and `data.edited` the files written or edited without error, newest first. In a page, tool output over 2 KB is cut (`trimmed: true`), edit diffs over 16 KB are left out, tool images are left out (`omittedImages: n`), and prompt images keep only their type. |
| `get_tool_result` | `toolCallId` | `data`: that tool result in full, as stored. For rows a page trimmed. |
| `get_tool_outputs` / `get_tool_output` | `index` (1-based, optional) | Stored tool output. |
| `get_session_stats` | | Token usage and context. |
| `get_available_models` | | `data.models: [{provider, id, contextWindow, reasoning, images}]` for providers the user can use; `data.catalogError` if a catalog refresh failed. Refreshes catalogs when idle. |
| `get_available_thinking_levels` | | Levels for the current model. |
| `set_model` | `provider`, `modelId` | Not during a run. |
| `set_thinking_level` | `level` | Not during a run. |
| `new_session` | optional `kind` | Defaults to `chat`. Desktop can pass `canvas`. Same workspace. Unavailable during a run. |
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
| `queue_rejected` | `message`, `images`, `steer`, `error` | A queued prompt was held back after a failure or while waiting for an answer. Show it so the user can review and resend it. |
| `auto_retry_start` / `auto_retry_end` | `success` | The engine is retrying a provider error. |
| `compaction_start` / `compaction_end` | `result` or `errorMessage` | Context compaction. |
| `provider_status` | `provider`, `waiting` | The Rooxcode Gateway is queueing the request. |
| `provider_error` | `error` | A provider or catalog problem, safe to show as is. |
| `extension_error` | `error` | An extension failed to load. |
| `extension_command_output` | `text` | Output of `extension_command`. |
| `agent_settled` | | The run and its queued prompts are finished. Commands marked "not during a run" work again. |

The native engine does not send `extension_ui_request` dialogs today; the TUI bridge still handles them for compatibility. A desktop app does not need them until the engine sends them.

Error texts are already redacted by the engine (`error::safe_message`) and can be shown directly.

## Canvas

### Engine tools

The desktop app sets `HIBISCUS_CANVAS`. With it, the engine adds canvas instructions to its system prompt and offers these tools. None of them runs anything except `analyze_video`; the app opens cards from the tool call events.

- `text_to_speech` opens a Video card and a joined Text to speech card holding the script.
- `storyboard` opens the storyboard. Beats can arrive in batches (`from`), and each beat can name its style `component`. The card flags a component used by more than one scene.
- `ask_user` opens a Questions card with suggested answers. The answers come back as the user's next message.
- `video_preview` opens or updates the preview card. With the HyperFrames `project` folder, it returns the timeline's length and scene count in `details`. Caption tracks don't count as scenes.
- `analyze_video` cuts evenly spaced stills from a video with ffmpeg: 12 by default, at most 24, 640 px wide. It returns them as JPEG images, and `details.frames` lists their times in seconds. The app opens a Frames card when the call starts and fills it from the result. The terminal app offers this tool when `ffmpeg` is on PATH.

The app bundles ffmpeg 6.1.1 from ffmpeg-static as a separate program, with its GPL-3.0 licence in `licenses/ffmpeg.txt`, and points the engine at it with `HIBISCUS_FFMPEG`.

### ElevenLabs

"Sign in with ElevenLabs" uses OAuth 2.1 with PKCE (S256) against ElevenLabs' hosted MCP server, `resource=https://api.elevenlabs.io/v1/mcp`, as a public client with no secret.

- Hibiscus's client ID is the address of its Client ID Metadata Document, `https://rekabytes-enterprise.github.io/hibiscus/oauth/elevenlabs.json`. GitHub Pages serves it from the `gh-pages` branch. It allows `http://127.0.0.1/callback` on any port.
- `desktop::elevenlabs::ElevenLabsSignIn` listens on a free loopback port and waits up to ten minutes for the browser or a pasted redirect URL. It stores the token in `connectors.json` and refreshes it before it expires.
- Signed in, the canvas lists voices with the `creative_list_voices` tool and makes speech with `creative_generate_speech`. It polls `creative_get_flow_run_status` and downloads the result from `creative_get_generations`.

A pasted API key, when there is one, comes first and uses the REST API:

- `connect(key)` checks the key against `GET /v2/voices`, then stores it in `connectors.json`. That file is private (mode 0600) and separate from the model providers' `auth.json`.
- `models()` lists the `GET /v1/models` entries that can do text to speech. `voices(search)` lists up to 100 voices from `GET /v2/voices`.
- `speak(text, voice, model)` calls `POST /v1/text-to-speech/{voice}` with `output_format=mp3_44100_128` and returns the MP3 as base64.
- Every request sends the key in `xi-api-key`.

### Cards and sessions

- Desktop groups saved Chat and Canvas sessions by folder and opens each in its own view. The kind is stored in the session header and stays fixed. Legacy sessions with a saved board open in Canvas. Other legacy sessions open in Chat.
- Both Desktop session types support `ask_user`. A successful call pauses the run. Chat shows an inline form for suggested or typed answers, and Canvas shows a question card. The answers arrive as the next user message.
- Storyboard, video preview and text-to-speech cards are Canvas tools. `analyze_video` can return frames to the agent in Chat too, without opening a board.

- A card's Send runs the agent in the open folder's session. The card's model and reasoning level become the session's. Cards the agent's tools open while it works join the card that was sent.
- A card locks the moment it's sent, so it can't be sent twice. A failed model reply shows on the card as "The model failed: …".
- `desktop::save_canvas_reference` saves each reference when the user adds it, under `~/.hibiscus/canvas/references`. The message lists references by path, and attaches images too. `desktop::read_canvas_reference` reads only from that folder, so previews come back after a restart.
- Each session keeps its own canvas. `desktop::save_canvas(session, json)` writes it atomically to `~/.hibiscus/canvas/boards/<hash of the session path>.json`, at most 64 MiB, and removes the file when the canvas is empty. `desktop::load_canvas(session)` reads it back when the session opens.
- While the agent works, that video's cards show what it's doing and for how long.

### Video engine

Download engine, shown in place of New until it's done, installs the video engine into `~/.hibiscus/runtime`:

- Node.js 22.23.5, checked against nodejs.org's SHA-256.
- `hyperframes@0.9.59` (Apache-2.0, the CLI and its skills), installed with that Node's npm.
- HyperFrames' pinned Chrome, from `hyperframes browser ensure`.

Once it's installed, the agent's shell finds Node.js, `hyperframes` and the bundled ffmpeg first on `PATH`, with `HYPERFRAMES_NO_TELEMETRY=1`. HyperFrames' skills load alongside the project's in Canvas sessions.

### Styles

- The app ships its skills in `desktop/src-tauri/skills/` as a Tauri resource and passes their folder to the engine at startup (`desktop::set_bundled_skills`). The first is the `tech-explainer-short` video style.
- Personal skills in `~/.hibiscus/skills` load in every project. One with the same name replaces the bundled skill.
- A skill whose frontmatter has `hibiscus-style: true` is a video style (`desktop::styles()`). The canvas lists it on Video cards and names it, with the path to its SKILL.md, in every step's request.
- With a style picked, the script uses `## N. Scene name` headings, the storyboard has one scene per heading drawn to the style's layout, the canvas makes one ElevenLabs clip per scene, and the agent builds with the style's generator. Without headings, blank lines split the script into scenes.

### Voiceover and follow-ups

- Every build uses the voiceover made on the script card in this session. There's no default voice. An existing voiceover is reused, and a styled build won't start without one. The agent never makes silent or placeholder audio.
- A Video card remembers whether its last build had the voiceover (`voicedBuild`). When it didn't, the preview card offers Add the voiceover.
- A finished video's preview card has a Follow up box, hidden while the agent works, with three modes:
  - Visuals only keeps the script and attaches this session's voiceover, which replaces any silent or placeholder track.
  - Change the words asks for a new script and storyboard. When the agent stops, the canvas makes the new voiceover and rebuilds.
  - Rebuild makes a new storyboard from the confirmed script, then rebuilds with this session's voiceover.
- Build requests ask the agent to write the generator a few scenes at a time. `desktop/src/lib/prompts.ts` holds every prompt the canvas sends.
- Rooxcode replies may stream for up to an hour, but fail after two minutes with nothing arriving.

### Run checks

- The bash tool adds a warning when a script exits 0 but its output shows a Python or Node.js crash, because the exit code is only the last command's.
- The preview card compares `video_preview`'s length and scene count with this session's voiceover and the storyboard. When they differ, it shows the mismatch and a Fix the timing button.
- A run that ends right after a tool result, with no reply from the model, marks its card "Stopped before finishing" with the last step and a Continue button.

## Request size

These apply to every session, not only the canvas.

- Anthropic requests mark cache points on the system prompt, the tools and the latest message.
- Tool results older than the latest 10, and everything before the latest canvas prompt, go to the model as their first 400 characters and a note once they pass 2 KB. Long strings in older tool-call arguments get the same treatment. The cut moves in steps of 10 results, so provider caches keep hitting. The saved session keeps everything.
- Every tool's output except `read`, `grep` and `glob` drops colour codes and redrawn progress lines. Past 30,000 characters it keeps the first 2 KB and the last 8 KB, and the full output goes to `~/.hibiscus/tool-output/` for 7 days.
- `read` numbers its lines and returns at most 2,000 lines or 50 KB. It cuts lines over 2,000 characters, reads from the end with a negative offset, and says which lines it returned.
- `grep` and `glob` search contents and paths with ripgrep's walker. They skip ignored, hidden and binary files and return at most 200 results.
