# Terminal UI

Start `hibiscus` in an interactive terminal. The fullscreen UI renders the native agent's progress, tool outcomes and errors. Small terminals and `TERM=dumb` use line mode; piped input produces plain output. [Native runtime configuration](native-runtime.md) explains providers and credentials.

## Input and navigation

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

Dialogs use the shared docked modal, preserve the composer draft and default to cancellation or denial. Approval details wrap and can be scrolled. Viewing a detail page is not proof the user read it. `/approval` changes the persistent shell gate; native Always Allow grants apply to the exact displayed command for the current run. File tools are not sandboxed or approval-gated.

`/login` offers Browser Login → Codex or API Key → configured provider. Credentials and manual redirect URLs are masked. Codex browser login uses a native loopback callback and supports Ctrl+Y to copy the link; Esc cancels. `/logout` selects a stored credential and requires confirmation. These commands require fullscreen input. Credentials live only in Hibiscus's private stores; environment keys take precedence for API-key providers and survive logout. Saving credentials does not prove model access. External-agent credential stores are not read. See [native authentication](native-runtime.md#authentication).

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

Fullscreen color mode uses the aubergine background and raspberry highlights. `NO_COLOR=1` preserves glyphs/layout without ANSI colors. Synchronized output and cached row painting reduce redraw work. Narrow layouts reduce margins and omit optional fields instead of clipping them. Resize invalidates the cached view without changing the draft.

Native runtime diagnostics are sanitized and bounded. Provider bodies and credentials are not printed into the transcript. A diagnostic does not imply successful authentication or run completion. Initial startup or terminal I/O errors may exit when a usable UI cannot be established.

Automated PTY tests check bytes, keyboard behavior and reconstructed views. They do not prove flicker-free rendering, clipboard success or perfect Unicode alignment in every physical terminal. Report the terminal, OS and minimal reproduction without sharing private conversations or credentials.
