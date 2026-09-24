# Terminal UI

Start `hibiscus` in a terminal for an interactive chat. `hibiscus --continue` resumes the latest Pi session for this directory; `hibiscus --sessions` opens the startup numbered picker. Inside the full-screen chat, `/sessions` and `/models` use docked pickers above the composer.

## Input and navigation

| Action | Key |
|---|---|
| Send a message | Enter |
| Delete a character | Backspace |
| Stop a running Pi response | Esc |
| Exit from an empty composer | Ctrl+D (or `/quit`) |
| Scroll transcript | Mouse wheel, PageUp/PageDown |
| Navigate a model/session picker | Up/Down, wheel, PageUp/PageDown |
| Accept a picker choice | Enter |
| Cancel a picker | Esc |

Type `/` to see only Hibiscus's supported commands; a prefix such as `/m` filters the suggestions. Up/Down or the wheel changes the highlighted suggestion. **Tab** completes without running it, **Enter** runs it, and **Esc** dismisses the list without erasing the text. Type `/help` to see chat commands.

The composer currently supports only **single-line, end-of-line editing**. Full-screen menus are unavailable in line mode, where numbered selection remains available. Hibiscus chat input is text-only; images should be attached in Pi's own TUI instead.

## Status and transcript

The header shows the workspace, model, and session. While a prompt is active, the flower cycles and the pink footer shows a working, thinking, writing, retry, compaction, or tool phase with elapsed time. This is a heartbeat even when the provider has not sent text. A heartbeat is **not proof the provider is making progress**; it means Hibiscus is still awaiting Pi.

The transcript renders common Markdown for readability without changing saved Pi messages. It shows concise tool start/finish events with read/write/edit file paths and success or failure. Raw thinking text and full tool results are not displayed. For a successful Pi `edit`, Hibiscus can show up to 40 lines of Pi's returned diff (context, additions, deletions); an unsuccessful edit or missing diff does not get a preview. The preview is not a replacement for reviewing `git diff`.

Scrolling stops at the oldest complete viewport instead of leaving most of the screen empty. When new output arrives while you are scrolled up, the view remains anchored until you scroll back to the bottom.

## Authentication handoff

In full-screen mode, `/login` offers **OpenAI Codex** or **another provider** regardless of the active model (which may be absent after logout). With Node.js and the matching Pi SDK available, choosing Codex stays in Hibiscus: select browser or device-code sign-in and wait for confirmation. Browser sign-in appears as **Open Codex sign-in ↗** rather than a URL broken across rows: Ctrl+click the hyperlink, or press **Ctrl+Y** to ask an OSC 52-capable terminal to copy the complete URL. If neither terminal feature works, try device-code sign-in. A browser callback can finish automatically; if it cannot reach WSL, paste the redirect URL into the masked composer instead. Press Esc to cancel. Once Pi's SDK reports login and credential storage complete, Hibiscus ends the dedicated helper itself; it does not wait for the browser tab or callback connection to close. The URL stays out of transcript text but still contains sensitive OAuth state; the device code is also short-lived and sensitive. Do not share screenshots of either. Pi handles credential storage and token refresh. On success Hibiscus reconnects its idle RPC child to the same saved session (or starts a fresh session if the chat is still empty). Ctrl+C is read as an input key while the terminal is in raw mode; use Esc to cancel the sign-in flow rather than expecting the shell's usual interrupt signal.

`/logout`, non-Codex provider login, line mode, and an unavailable Node/SDK helper continue to use the **Pi TUI handoff**. No prior message is needed. Hibiscus closes its idle RPC child before Pi opens the saved session; for an empty chat Pi runs with `--no-session`. In Pi, run `/login` or `/logout`, then `/quit` to return. Hibiscus reconnects its RPC child afterward. Hibiscus never stores credentials itself.

## Terminal compatibility

- `NO_COLOR=1` disables ANSI colors.
- `TERM=dumb` or a terminal smaller than the full-screen minimum uses line mode.
- Piped input produces plain output; it does not support Esc or interactive approvals.
- Pi extension dialog confirmations require an explicit `y` or `yes`. Non-interactive dialogs cancel safely.
- If a terminal's mouse or key escape sequences behave differently, use PageUp/PageDown and report the terminal name, WSL version, and a minimal reproduction. Do not attach private chat transcripts or credentials to a bug report.
