# Terminal UI

Start `hibiscus` in an interactive terminal. TUI is the default view and renders the native agent's progress, tool outcomes and errors. Small terminals and `TERM=dumb` use line mode; piped input produces plain output. [Native runtime configuration](native-runtime.md) explains providers and credentials.

## Switching views

Type `/cli` to use inline chat in the terminal's normal scrollback, then `/tui` to return to fullscreen. Switching keeps the current runtime session and conversation without resending prompts. TUI requires a capable terminal at least 40 columns by 14 rows. Send or remove staged images before switching.

Use `hibiscus --cli` to start directly in CLI, or `hibiscus --cli --continue` to resume the latest session there. `hibiscus --tui` explicitly selects the default view. The view is not saved as a startup preference; the next ordinary `hibiscus` launch starts in TUI.

Both welcome layouts show the command for the other view. The `/login` tip and TUI command suggestion disappear when any configured provider has credentials. You can still type `/login` to add or replace credentials. Credential presence does not verify remote access.

CLI starts with a plain text name/version, model, workspace and command hints. The large HIBI logo and bordered welcome panel appear only in TUI. CLI edits multiline drafts in normal scrollback, streams responses and supports Esc to stop an active run. `/models` uses a numbered list in CLI. The docked composer, staged images, transcript selection and live steering/followup controls belong to TUI. `/login` and `/logout` temporarily open the native fullscreen dialogs, then return to CLI.

The CLI welcome uses two lines. Each user turn has one raspberry `hibi` label; assistant updates and tool results follow beneath it. A running tool occupies one temporary row, replaced on completion by a green success row or a red failure row. Consecutive successful reads share a row. Plain `NO_COLOR` output prints completed rows without cursor rewriting.

Shell rows show the actual command, with OS exit code and measured execution duration aligned to the right. Edit rows put addition/removal counts in the same metadata column. Long commands clip to preserve metadata; narrow terminals shorten the row. Tool output is indented beneath it, limited to twelve lines with the beginning and end retained and an omission count. Repeated blank lines and decorative separator rules are compacted, while patch whitespace and dash-only changes are preserved. Failure text stays visible. Edit previews show the changed region with one surrounding context line, muted line numbers and red/green code. Previews are bounded and come from the successful native write. Display compaction does not change saved output or model context.

The runtime adapter decodes tool events into display data. The inline renderer owns grouping, clipping and the temporary row because it knows terminal width and color support. Keeping cursor control out of the runtime adapter preserves the separate fullscreen timeline and plain piped output. There is no CLI animation loop or fullscreen repaint.

Before the first response, CLI prints one static `Waiting for response · Esc to stop` row. Color mode clears it when text, a tool or an error arrives; `NO_COLOR` prints it as an ordinary line. Esc cancels and clears the waiting state. Long shell commands keep a short status row with right-aligned metadata and wrap beneath it. The preview shows at most eight command lines; saved-output inspection shows the complete stored command.

## Saved tool output

`/output` shows the latest completed tool result in the current active session history. `/output list` lists the latest fifty results with their numbers and exit status. `/output <number>` shows that result's complete saved text, command and recorded edit diff when available. These are local read-only queries: they do not call the provider, rerun a tool, change model context or save the slash command as a user message. Existing runtime output bounds still apply. Inspection preserves text omitted or compacted by the normal preview, wraps long lines and strips terminal controls. Staged composer images remain staged.

Results remain available after reopening the same session. Numbers refer to its active history; starting a new session or compacting context changes that history. Archived compaction history is not searched. Empty history and unavailable numbers produce a local message without sending a prompt. Omitted previews offer `/output list` so an earlier result can be selected after later tools finish.

## Input and navigation

In CLI, Enter sends the draft. Alt+Enter or Ctrl+J inserts a newline; arrows move through it, Home/End move within a line and Backspace/Delete edit across line breaks. Ctrl+C clears a nonempty draft and exits when empty. Ctrl+D deletes at the cursor, or exits an empty draft. Bracketed paste keeps multiline text in one draft without submitting it, normalizes CRLF and expands tabs to four spaces. Leading/trailing whitespace is retained in ordinary CLI prompts. A pasted `/quit` on an internal line is text, not a command.

The CLI draft is limited to 64 KiB. Invalid UTF-8 and oversized paste are rejected without replacing the existing draft. The editor repaints at most six draft rows plus a quiet overflow/error notice; sending prints the whole prompt. It uses the existing terminal reader, with no idle repaint loop. A terminal resize is handled on the next key by starting a fresh draft block. `TERM=dumb` and nonterminal input keep the simple single-line reader. Alt+Enter depends on the terminal forwarding a distinct key sequence; Ctrl+J is the newline fallback with ordinary carriage-return Enter.

The following table describes the fullscreen TUI:

| Input | Action |
| --- | --- |
| Enter | Send a prompt; during a run, submit steering for a tool boundary |
| Ctrl+Enter or Ctrl+J | Insert a newline where the terminal distinguishes it |
| Ctrl+Q during a run | Queue a followup after the current turn |
| Esc during a run | Cancel HTTP/tool work and wait for settlement |
| Up/Down, Home/End | Move through the draft, with picker navigation taking priority |
| PageUp/PageDown or wheel | Scroll the transcript; wheel over the composer scrolls the draft |
| `@path` | Pick and insert a literal path without reading/uploading the file |
| `/` | Show implemented slash commands |
| Alt+V on WSL, Ctrl+V elsewhere | Stage a clipboard image |
| Ctrl+X | Remove staged images |
| Ctrl+C with a selection | Request clipboard copy |
| Ctrl+C otherwise while idle | Clear the draft; press again within two seconds to exit |
| Ctrl+D or `/quit` | Exit |

The composer grows to five visible rows and then scrolls internally. Bracketed paste inserts literal text, normalizes newlines and never submits embedded Enter characters. Input is bounded; invalid/oversized paste is rejected. A terminal that does not forward bracketed-paste markers cannot distinguish pasted newlines from Enter. Slash suggestions support Tab completion and Enter execution; typed slash commands do not become user chat rows.

The cursor stays visible while editing and while the agent works. Hibiscus requests a steady block cursor and restores terminal defaults on exit. Terminal settings may still affect cursor appearance. A separate flower/activity indicator shows waiting, tools and response progress.

## Models and dialogs

`/models` searches the configured `provider/model` catalog with a labeled input and divider. Type/paste terms, use arrows/PageUp/PageDown, Enter to select and Esc to cancel. The current-model marker follows filtering; no matches leaves the picker open. The selected model appears in the header, without a selection-confirmation message in chat.

Login/logout and selection dialogs use the shared docked modal and preserve the composer draft. Native tools execute directly without a permission dialog; tools are not sandboxed.

`/login` offers Browser Login → Codex or API Key → configured provider. Credentials and manual redirect URLs are masked. Codex browser login uses a native loopback callback and supports Ctrl+Y to copy the link; Esc cancels. `/logout` selects a stored credential and requires confirmation. These dialogs require fullscreen input, including when opened temporarily from CLI. Credentials live only in Hibiscus's private stores; environment keys take precedence for API-key providers and survive logout. Saving credentials does not prove model access. External-agent credential stores are not read. See [native authentication](native-runtime.md#authentication).

## Progress, usage and recovery

Tool activity uses call IDs. A successful edit displays the recorded diff; failed edits do not claim a successful change. Goal progress comes only from an explicit checklist, not tool-call counts. Plain assistant text and Markdown are visible; private model reasoning is not displayed.

Usage/context values come from provider receipts and configured capabilities. Unknown values stay unknown. Dollar cost is unavailable without a pricing source. The footer updates at idle boundaries rather than repeatedly querying metadata during a run. Prompt acceptance does not mean completion; settlement follows tool results and queue delivery.

Steering enters context between completed tool batches. Followups wait until the current turn completes. Their user rows appear when the runtime actually delivers them, not just when the UI queues them. Esc clears queued messages and cancels the current operation. It never automatically replays interrupted work.

`/restore` restores a failed prompt and attachments for review. `/reconnect` restarts the local runtime from its confirmed session without resending. `/compact` summarizes active context and privately archives the original history. Automatic compaction needs a configured context window and confirmed usage. See [error handling](error-handling.md).

## Markdown, images and copying

Markdown tables wrap cells within shared column widths. Completed supported Mermaid flowcharts can render as terminal diagrams after settlement; unsupported or oversized graphs remain source. One-shot/piped output preserves original Markdown.

Up to four PNG/JPEG/GIF/WebP clipboard images, each at most 10 MiB before encoding, can be staged. The composer shows an attachment count. Clipboard reads are bounded and asynchronous, and failure keeps the text draft. The selected model must support images. Text paste is separate from image attachment.

Transcript drag selection covers visible rows. Release or Ctrl+C requests a copy through desktop helpers for small selections, or a bounded OSC 52 request. Terminal/multiplexer support determines whether OSC 52 works, so Hibiscus does not claim an unverified copy succeeded. Selection clears when the view changes. Off-screen selection and Unicode cell alignment have limits. macOS uses its clipboard helpers; Linux uses Wayland/X11 helpers when available; WSL image paste uses Windows clipboard access. Verify physical clipboard behavior in your own terminal.

## Rendering and compatibility

Fullscreen color mode uses the aubergine background and raspberry highlights. CLI uses a solid dark background with green added diff lines and red removed lines, including streamed Git diffs and native edit previews. It applies local SGR styling and restores it when leaving CLI without changing the terminal palette. `NO_COLOR=1` disables colors and background changes; interactive CLI editing still uses cursor controls and bracketed-paste mode. These modes are restored when leaving input. Piped output stays plain. Synchronized output and cached row painting reduce fullscreen redraw work. Narrow layouts reduce margins and omit optional fields. Fullscreen resize invalidates the cached view without changing the draft.

Native runtime diagnostics are sanitized and bounded. Provider bodies and credentials are not printed into the transcript. A diagnostic does not imply successful authentication or run completion. Initial startup or terminal I/O errors may exit when a usable UI cannot be established.

Automated PTY tests check bytes, keyboard behavior and reconstructed views. They do not prove flicker-free rendering, clipboard success or perfect Unicode alignment in every physical terminal. Report the terminal, OS and minimal reproduction without sharing private conversations or credentials.
