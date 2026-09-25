# Terminal UI

Start `hibiscus` in a terminal for an interactive chat. Hibiscus starts Pi with installed extensions disabled, one explicit approval gate, and only its built-in `read`, `bash`, `edit`, and `write` tools active, so Pi-installed MCP extensions do not start (also during auth handoffs). Running Pi directly still uses your normal configuration. `hibiscus --continue` resumes the latest Pi session for this directory; `hibiscus --sessions` opens the startup numbered picker. Inside the full-screen chat, `/sessions` and `/models` use docked pickers above the composer. `/models` lists Pi's configured models as `provider/model`, sorted by provider and model ID; selecting one switches providers and models in the current chat without a new login if Pi already has access. Pi remains responsible for credentials and reports any model-access errors.

A successful **`/new`** opens a blank full-screen chat: it clears the visible transcript, input/attachments, scroll position, and failed-prompt recovery buffer. The model stays selected and the header updates from Pi. Saved sessions are not deleted; use `/sessions` to return to them. If Pi rejects or cancels creation, the current conversation stays visible. Line mode retains its plain “Started new session” confirmation.

## Input and navigation

| Action | Key |
|---|---|
| Send a message | Enter |
| Insert a newline | Ctrl+Enter (Ctrl+J fallback) |
| Attach an image from the clipboard | Alt+V on WSL; Ctrl+V elsewhere |
| Remove staged images | Ctrl+X |
| Scroll a long draft | Up/Down, or mouse wheel over the input rows |
| Delete a character | Backspace |
| Stop a running Pi response | Esc |
| Exit from an empty composer | Ctrl+D (or `/quit`) |
| Scroll transcript | Mouse wheel, PageUp/PageDown |
| Navigate a model/session picker | Up/Down, wheel, PageUp/PageDown |
| Accept a picker choice | Enter |
| Cancel a picker | Esc |

Type `/` to see only Hibiscus's supported commands; a prefix such as `/m` filters the suggestions. Up/Down or the wheel changes the highlighted suggestion. **Tab** completes without running it, **Enter** runs it, and **Esc** dismisses the list without erasing the text. Type `/help` to see chat commands.

The full-screen composer grows with explicit newlines and wrapped text to **five visible rows**, then scrolls internally. Up/Down or the wheel over the input rows reviews a long draft; typing returns to its end. Editing is still end-of-draft only. Ctrl+Enter requires a terminal that sends a newline or distinct modified Enter sequence (Kitty/CSI-u or modifyOtherKeys); use Ctrl+J if your terminal sends it as ordinary Enter. Shift+Enter sequences are also accepted where the terminal distinguishes them, but some terminals send Shift+Enter as ordinary Enter. Full-screen menus are unavailable in line mode, where numbered selection remains available. For clipboard images, press **Alt+V on WSL** or **Ctrl+V elsewhere**, matching Pi's platform defaults. Hibiscus accepts both shortcuts, including CSI-u/modifyOtherKeys encodings. The border shows the attachment count (not an inline image preview). Add optional text and press Enter to send, or Ctrl+X to remove all staged images. Image-only prompts work too. Ctrl+C clears both text and attachments. Remove attachments before running slash commands.

Clipboard image paste supports macOS via AppKit/`osascript`, WSL via Windows `powershell.exe`, Wayland via `wl-paste` (`wl-clipboard`), and X11 via `xclip`. It reads the clipboard on the machine running Hibiscus, not an SSH client's clipboard. Windows Terminal can intercept Ctrl+V for terminal paste, which is why WSL uses Alt+V instead. The chosen shortcut must reach Hibiscus. Normal terminal text paste is separate from image paste. Up to four PNG/JPEG/GIF/WebP images, at most 10 MiB each, can be staged; macOS and WSL screenshots are converted to PNG in memory. Clipboard reads are asynchronous and bounded; failures leave the text draft intact. Images are sent as Pi RPC `images` content blocks, without temporary image files or base64 in the visible transcript. Pi owns persisted session content and provider handling; use an image-capable model.

## Status and transcript

The header shows the workspace, model, and session. While a prompt is active, the flower cycles and the pink footer shows a working, thinking, writing, retry, compaction, or tool phase with elapsed time. This is a heartbeat even when the provider has not sent text. A heartbeat is **not proof the provider is making progress**; it means Hibiscus is still awaiting Pi.

The transcript renders common Markdown for readability without changing saved Pi messages. It shows concise tool start/finish events with read/write/edit file paths and success or failure. Raw thinking text and full tool results are not displayed. For a successful Pi `edit`, Hibiscus can show up to 40 lines of Pi's returned diff (context, additions, deletions); an unsuccessful edit or missing diff does not get a preview. The preview is not a replacement for reviewing `git diff`.

Scrolling stops at the oldest complete viewport instead of leaving most of the screen empty. When new output arrives while you are scrolled up, the view remains anchored until you scroll back to the bottom.

## Dangerous-command approval

`read`, `edit`, `write`, and ordinary `bash` commands run without a prompt. Before Pi's **agent** executes a recognized dangerous `bash` tool call (such as `rm`, `git clean`/force push, `sudo`, permission changes, publishing, shell indirection, or output redirection), Hibiscus shows the reason, working directory, and exact command. Choose **1 Deny** (default), **2 Allow** (once), or **3 Always Allow**. Always Allow applies only to that exact command string and directory in the current chat; it resets on session switch/new session and restart. No wildcard or persistent approvals. Esc, Enter with no choice, timeout, error, and non-interactive use deny by default. In full-screen mode the confirmation is currently a numbered terminal prompt rather than a docked picker.

This is a **best-effort guard, not a sandbox**. Arbitrary scripts, indirect effects of seemingly safe commands, and `edit`/`write` can still change or remove files; command classification cannot prove a shell program is harmless. Pi remains responsible for tool execution and model behavior. Direct user shell commands and tools outside the four-tool allowlist are not covered by this approval gate. [Architecture →](architecture.md)

## Errors and recovery

A rate limit, subscription/quota problem, authentication failure, or rejected command no longer closes interactive chat. Hibiscus shows bounded error details and guidance while retaining the transcript. Pi controls automatic retries; the composer becomes available after Pi settles, not at the first failed attempt. Tool and extension warnings do not automatically fail the whole run.

- **`/restore`** brings the last failed prompt and images back into the full-screen composer for review. Nothing is sent until you press Enter; earlier tool actions may already have happened.
- **`/reconnect`** recovers a disconnected Pi backend using the last confirmed session path. It never replays a prompt. If no path was confirmed, it explains that a new session will be opened.
- While disconnected, `/help`, `/restore`, and `/quit` remain available; other input is not sent.
- One-shot/piped failures still exit nonzero. Unusable terminal I/O and initial startup failures can still exit.

See [error handling and recovery](error-handling.md) for the event mapping, classification, limits, and regression tests.

## Updates

Prebuilt installs in full-screen mode check for a newer public GitHub release in the background at most once per day. When idle, the picker offers **Later** (the safe default) or **Update now**. A check never interrupts a draft in progress, and offline failures do not block chat. To check or update on demand, exit and run `hibiscus update`; see [Installation and releases](installation.md). Set `HIBISCUS_NO_UPDATE_CHECK=1` to disable automatic checks.

## Authentication handoff

In full-screen mode, `/login` offers **OpenAI Codex** or **another provider** regardless of the active model (which may be absent after logout). With Node.js and the matching Pi SDK available, choosing Codex stays in Hibiscus: select browser or device-code sign-in and wait for confirmation. On macOS, Hibiscus opens the validated browser sign-in URL with the system `open` command after you choose browser login, independent of terminal hyperlink support. The screen still shows **Open Codex sign-in ↗** as a short OSC 8 hyperlink where supported rather than a URL broken across rows. Press **Ctrl+Y** to ask an OSC 52-capable terminal to copy the complete URL. If the browser could not open and neither terminal feature works, cancel and retry using device-code sign-in. A browser callback can finish automatically; if it cannot reach WSL, paste the redirect URL into the masked composer instead. Press Esc to cancel. Once Pi's SDK reports login and credential storage complete, Hibiscus ends the dedicated helper itself; it does not wait for the browser tab or callback connection to close. The URL stays out of transcript text but still contains sensitive OAuth state; the device code is also short-lived and sensitive. Do not share screenshots of either. Pi handles credential storage and token refresh. On success Hibiscus reconnects its idle RPC child to the same saved session (or starts a fresh session if the chat is still empty). Ctrl+C is read as an input key while the terminal is in raw mode; use Esc to cancel the sign-in flow rather than expecting the shell's usual interrupt signal.

Non-Codex provider login, line-mode login, and login with an unavailable Node/SDK helper continue to use the **Pi TUI handoff**. No prior message is needed. Hibiscus closes its idle RPC child before Pi opens the saved session; for an empty chat Pi runs with `--no-session`. In Pi, run `/login`, then `/quit` to return. Hibiscus reconnects its RPC child afterward. Hibiscus never stores credentials itself.

### Logout inside Hibiscus

In full-screen mode, **`/logout`** lists providers with stored Pi credentials (OAuth or API keys), regardless of the active model. Choose a provider, then select **Remove stored credential** in the confirmation picker; **Cancel is the default**, and Esc cancels before removal. No saved chat is required.

Hibiscus uses Pi SDK `ModelRuntime.listCredentials()` and `logout()`—it does not edit `auth.json` itself. Only provider IDs and credential types cross the helper protocol, never tokens or API keys. After confirmation, Hibiscus closes the idle RPC backend before authorizing removal, then reopens the same saved chat (or a new empty session) so cached credentials are discarded. It also reopens after a helper error that might have occurred after mutation. Once removal starts, it waits for the bounded result rather than pretending cancellation can undo it.

This removes **one stored credential shared by Pi and Hibiscus**, not the conversation. It does **not** unset environment variables, change `models.json`, or revoke tokens at the provider. Those other credential sources can still provide access. If Pi removes the credential but reports a synchronization failure, Hibiscus reports that distinction and refreshes the backend.

With no stored credentials, nothing is changed. If Node/the compatible SDK is missing, or full-screen input is unavailable, Hibiscus explains the limitation without opening Pi automatically; run `pi` and `/logout` manually if needed. Piped input never authorizes logout. Real-provider logout is not exercised by the automated tests; they use an isolated fake credential store.

## Terminal compatibility

- `NO_COLOR=1` disables ANSI colors.
- `TERM=dumb` or a terminal smaller than the full-screen minimum uses line mode.
- Piped input produces plain output; it does not support Esc or interactive approvals.
- Pi extension dialog confirmations require an explicit `y` or `yes`. Non-interactive dialogs cancel safely.
- If a terminal's mouse or key escape sequences behave differently, use PageUp/PageDown and report the terminal name, WSL version, and a minimal reproduction. Do not attach private chat transcripts or credentials to a bug report.
