# hibiscus

[![CI](https://github.com/Rekabytes-Enterprise/hibiscus/actions/workflows/ci.yml/badge.svg)](https://github.com/Rekabytes-Enterprise/hibiscus/actions/workflows/ci.yml)

Hibiscus is a small Rust terminal client for the [Pi](https://pi.dev) agent. It talks to Pi through its JSONL RPC interface: Pi owns models, authentication, tools, agent runs, and persisted sessions; Hibiscus provides the CLI and terminal UI.

## Requirements

- Pi installed, configured, and available as `pi` on `PATH`
- Node.js 22.19+ for the inline Codex sign-in flow (without Node, Hibiscus falls back to Pi's TUI)
- Rust, Cargo, and a C linker only when building from source (Ubuntu/WSL: `sudo apt update && sudo apt install -y build-essential`)

Set `HIBISCUS_PI` if Pi is installed under a different executable name or path.

## Install

Prebuilt releases support Linux and macOS on x86-64 and ARM64:

```sh
curl -fsSL https://raw.githubusercontent.com/Rekabytes-Enterprise/hibiscus/main/install.sh | sh
```

The installer verifies the release checksum and writes to `~/.local/bin` without `sudo`. Review-first installation, private-repository authentication, version pinning, and release details are covered in [Installation and releases](docs/installation.md).

To build from source:

```sh
cargo test
cargo install --path .
```

Run `cargo install --path .` again after changing the source.

## Usage

```sh
hibiscus                                  # start a new chat
hibiscus --continue                       # resume the latest session here
hibiscus --sessions                       # choose a saved session
hibiscus --version                        # print installed version
hibiscus "Explain this repository"        # one-shot prompt
printf 'Hello\nFollow up\n' | hibiscus   # piped multi-turn chat
```

Interactive chat commands:

- `/new` — start a new Pi session
- `/continue` — resume the latest saved session
- `/sessions` — choose and switch to a saved session
- `/models` — choose an available model for the current provider
- `/login` — choose OpenAI Codex sign-in in Hibiscus, or open Pi for another provider (independent of the currently selected model)
- `/logout` — hand the terminal to Pi for native logout
- `/help`
- `/quit` or `/exit`

One-shot prompts are saved as their own Pi sessions. Hibiscus does not maintain a separate conversation database. Session lists are read from Pi's JSONL files and filtered to the current working directory. Pi's session directory settings are respected, including `PI_CODING_AGENT_SESSION_DIR`, `PI_CODING_AGENT_DIR`, and `sessionDir` in Pi settings.

When resuming or switching sessions, Hibiscus displays up to the five most recent user turns and their visible assistant text. Pi retains the complete history.

## Terminal behavior

Interactive terminals use an alternate-screen interface with:

- a fixed composer and short model/session status
- docked five-row `/models` and `/sessions` pickers with active-item markers, arrow/wheel navigation, PageUp/PageDown movement, a scrollbar, Enter selection, and Esc cancellation
- live `/` command suggestions with prefix filtering; use Up/Down or the wheel to navigate, Tab to complete, Enter to run, and Esc to dismiss
- raspberry-pink accents and a cycling flower heartbeat while Pi is working, even before it streams text
- a raspberry-pink footer showing working, thinking, writing, and tool phases with elapsed time
- themed reasoning and tool start/finish lines, including reasoning duration, read/write/edit paths, and success/failure
- bounded, color-coded +/- diff previews with context and line numbers after successful Pi `edit` results (when Pi supplies `result.details.diff`); failed edits show no diff. Raw reasoning and full tool output remain hidden
- lightweight Markdown formatting for assistant output
- mouse-wheel and PageUp/PageDown transcript scrolling during idle or streaming responses; scrolling stops at the oldest complete page
- Esc to clear queued Pi messages and abort a running response

`NO_COLOR=1` disables colors. `TERM=dumb`, small terminals, and piped input use line-mode output instead. Piped mode writes assistant text to stdout and Pi tools/diagnostics to stderr. Full-screen picker navigation is unavailable in line mode, where `/models` and `/sessions` use numbered input instead.

Pi extension UI requests for `confirm`, `select`, `input`, and `editor` are handled in an interactive terminal. Confirmations require explicit `y` or `yes`, and selections require a valid number. Dialogs are cancelled in non-interactive mode. Terminal editing remains intentionally basic: single-line input with end-of-line editing.

In a full-screen terminal, `/login` first lets you choose **OpenAI Codex** or **another provider**. This is independent of the currently selected model: Pi may have no selected Codex model after logout. For Codex, Hibiscus launches a small Node helper using the installed **Pi SDK's** OAuth implementation. Hibiscus shows a short, clickable browser sign-in label (Ctrl+click) instead of a broken multi-line URL. Ctrl+Y asks compatible terminals to copy the complete URL via OSC 52; if hyperlinks/clipboard controls are unavailable, try the device-code option. Hibiscus accepts a manually pasted redirect URL without echoing it, then waits for Pi SDK login/storage completion, forcibly cleans up the dedicated helper (without waiting for the browser tab or callback socket to close), and reconnects the idle RPC child on the same saved session. You can sign in before sending a chat message. Esc cancels the flow; the helper's explicit completion event ends login, so a still-open browser success tab cannot keep Hibiscus waiting. Pi owns the credential file and refresh; Hibiscus does not copy OAuth tokens into its session or logs. **Do not share screenshots of authorization URLs or device codes.** If Node or the matching Pi SDK is unavailable, Codex login offers Pi's TUI fallback. `/logout` and login for other providers also use that TUI handoff. No prior chat message is needed: Hibiscus closes its RPC child before opening Pi on the saved session if one exists, or with `--no-session` otherwise. Run the auth command in Pi, then `/quit` to return; Hibiscus reopens its RPC child afterward.

## Project layout

- `src/main.rs` — CLI arguments and entry point
- `src/chat/` — chat commands, model selection, session discovery, and auth handoff
- `src/pi/` — Pi RPC subprocess, protocol events, Codex SDK auth bridge, and extension dialogs
- `src/tui/` — terminal input, screen rendering, pickers, and Markdown display
- `tests/` — mock Pi and PTY integration tests

See [docs/](docs/README.md) for architecture, terminal behavior, and development instructions, or [CONTRIBUTING.md](CONTRIBUTING.md) to contribute. `AGENTS.md` contains repository-specific development guidance; `.pi/STATE.md` tracks current work and `.pi/MEMORY.md` stores durable project knowledge.
