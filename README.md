<p align="center">
  <img src="docs/assets/hibiscus.svg" width="112" height="112" alt="Hibiscus flower with a terminal chevron">
</p>

<h1 align="center">hibiscus</h1>

<p align="center">
  <strong>Your terminal, in bloom.</strong><br>
  A standalone Rust coding agent with a terminal UI and native tools.
</p>

<p align="center">
  <a href="https://github.com/Rekabytes-Enterprise/hibiscus/releases/latest"><img src="https://img.shields.io/github/v/release/Rekabytes-Enterprise/hibiscus?style=flat-square&amp;color=ec4a7d&amp;label=release" alt="Latest release"></a>
  <a href="https://github.com/Rekabytes-Enterprise/hibiscus/actions/workflows/ci.yml?query=branch%3Adev"><img src="https://img.shields.io/github/actions/workflow/status/Rekabytes-Enterprise/hibiscus/ci.yml?branch=dev&amp;style=flat-square&amp;label=dev%20CI" alt="CI status on dev"></a>
  <a href="https://github.com/Rekabytes-Enterprise/hibiscus/releases"><img src="https://img.shields.io/github/downloads/Rekabytes-Enterprise/hibiscus/total?style=flat-square&amp;color=ec4a7d&amp;label=release%20downloads" alt="Total GitHub release asset downloads"></a>
  <a href="docs/installation.md"><img src="https://img.shields.io/badge/platforms-Linux%20%7C%20macOS-63304b?style=flat-square" alt="Platforms: Linux and macOS"></a>
  <!-- Enable after the first verified published scan; see docs/security-scorecard.md.
  <a href="https://scorecard.dev/viewer/?uri=github.com/Rekabytes-Enterprise/hibiscus"><img src="https://api.scorecard.dev/projects/github.com/Rekabytes-Enterprise/hibiscus/badge" alt="OpenSSF Scorecard"></a>
  -->
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#made-for-the-terminal">Features</a> ·
  <a href="#your-everyday-commands">Commands</a> ·
  <a href="docs/README.md">Documentation</a> ·
  <a href="CONTRIBUTING.md">Contribute</a> ·
  <a href="LICENSE">MIT license</a>
</p>

<p align="center">
  <img src="docs/assets/terminal-preview.svg" width="960" alt="Illustrated Hibiscus chat with raspberry accents, tool activity, an edit diff, and a multiline composer">
  <br><sub>Illustrated preview with sample content. Colors and key hints depend on your terminal.</sub>
</p>

Chat, switch models, pick up an earlier session, and share screenshots—without leaving your terminal. Hibiscus brings a calm, raspberry-pink interface; **Hibiscus owns the runtime**: provider connections, authentication, tools, agent runs, and local sessions.

<sub>The download badge updates automatically through Shields.io. It counts GitHub release asset downloads across releases, including checksum files—not unique users, source clones, or Cargo installations. Counts may be cached.</sub>

## Quick start

**Linux, WSL, and macOS · x86-64 and ARM64**

```sh
curl -fsSL https://raw.githubusercontent.com/Rekabytes-Enterprise/hibiscus/v0.3.7/install.sh | HIBISCUS_VERSION=v0.3.7 sh
```

Alternatively, install through **npx** (requires Node.js 22.19+ and npm):

```sh
npx @rekabytes/hibiscus@latest
```

The npm installer uses the same verified native release download as the curl installer. It installs Hibiscus permanently; after installation, start it with `hibiscus`. **Availability:** this command requires publication of `@rekabytes/hibiscus` and its matching GitHub release; adding the package to `dev` alone does not make it available on npm.

Then start a conversation:

```sh
hibiscus
```

Use `/login` to connect through native Codex browser login or a configured provider API key, then `/models` to search and choose a model. API-key providers use API billing; Codex browser login uses your ChatGPT account and its limits.

The installer verifies release checksums and places Hibiscus in `~/.local/bin`. It creates `~/.hibiscus/` for native configuration, private credentials, sessions, cache and user extensions. An absolute `HIBISCUS_HOME` selects another data directory. Provider/model IDs and supported context limits are explicit [configuration](docs/native-runtime.md#provider-configuration); existing credentials or sessions from other applications are not imported. The installed agent requires neither Node.js nor Rust. Node.js is needed only to run the npm installer.

Prefer to inspect the installer first, choose a version, or use a custom directory? See [installation options](docs/installation.md).

## Made for the terminal

| | What you get |
| :--- | :--- |
| **Clear conversations** | Aubergine TUI background, lighter user rows, visible `hibi` reply headings, and a scrolling transcript. A compact dark CLI is available through `/cli`. |
| **Readable Markdown** | Width-aware pipe tables with wrapped cells, per-row dividers and raspberry borders. In full-screen chat, completed assistant `mermaid` blocks become Unicode diagrams after the native runtime settles; the source stays visible while streaming or if rendering is unavailable. |
| **Room to think** | A movable insertion cursor in a multiline composer that grows to five rows, then scrolls within the input. |
| **Point to files** | Type `@` in the full-screen composer to search and insert a file path. Hibiscus can choose to read it with its native tools; Hibiscus does not silently send the file contents. |
| **Show, don't describe** | Paste clipboard images alongside your prompt, with a visible attachment count. |
| **Models at your fingertips** | A searchable keyboard-and-wheel picker of configured models, grouped by provider. |
| **Pick up where you left off** | Resume the latest session or choose another, with recent conversation context shown. |
| **Know what's happening** | A persistent HIBI header with model, thinking, workspace and session metadata; provider-reported context/input/output and cache usage beside the live footer phase and optional checklist Goal bar. Unknown context or cost stays unavailable. Tool activity, failures and actual edit diffs stay visible in chat. |
| **Stay in control** | Type while Hibiscus works: Enter steers and Ctrl+Q queues a follow-up. Waiting messages stay beside the composer until the runtime delivers them. Native tools execute directly after validated, completed provider responses. Esc clears the queue and stops a run. |

### Markdown at a glance

<p align="center">
  <img src="docs/assets/markdown-preview.svg" width="960" alt="Illustrated Hibiscus chat showing an aligned JavaScript versus TypeScript table and a three-step Unicode flowchart">
  <br><sub>Illustrated examples, not a captured session. Diagram layout depends on the supported graph and terminal width.</sub>
</p>

Tables are laid out within the transcript width, including model output that omits the usual Markdown separator row. For a fenced <code>mermaid</code> block, Hibiscus shows the source during streaming, then displays a diagram **after a successful settled reply**. Unsupported, too-wide or unavailable diagrams stay as source. This is a full-screen display transform only: Hibiscus keeps the original Markdown in the native saved session; CLI history and piped output remain plain text.

### A few keys to remember

| Action | Shortcut |
| :--- | :--- |
| Send / steer during a run | **Enter** |
| Queue a follow-up during a run | **Ctrl+Q** |
| Newline | **Ctrl+Enter** or **Ctrl+J** in TUI; **Alt+Enter** or **Ctrl+J** in CLI |
| Paste an image | **Alt+V on WSL** · **Ctrl+V elsewhere** |
| Remove staged images | **Ctrl+X** |
| Move/edit inside a draft | **← / → / ↑ / ↓**, Home/End, Backspace/Delete |
| Scroll a long draft | Wheel over the input |
| Scroll the conversation | **PgUp / PgDn** or mouse wheel |
| Stop the current run | **Esc** |

Image paste needs a supported local clipboard backend and an image-capable model. You can move the cursor to edit inside a draft. See [terminal behavior](docs/terminal.md) for clipboard requirements, limits, and terminal-specific shortcuts.

## Your everyday commands

```sh
hibiscus                              # open a new chat
hibiscus --continue                   # resume the latest session here
hibiscus --sessions                   # choose a saved session
hibiscus "Explain this repository"     # run a one-shot prompt
printf 'Hello\nFollow up\n' | hibiscus   # pipe a conversation
```

Inside a chat:

| Command | What it does |
| :--- | :--- |
| `/cli` | Switch to inline terminal chat |
| `/tui` | Return to full-screen chat |
| `/output [list\|number]` | Inspect saved tool output locally without rerunning tools |
| Custom commands, e.g. `/review [input]` | Run an installed extension command locally |
| `/new` | Open a blank chat while keeping previous sessions saved |
| `/continue` | Resume the latest saved session in this directory |
| `/sessions` | Choose and switch sessions |
| `/models` | Search and choose a model across configured native providers |
| `/thinking [level]` | Choose a supported Responses reasoning level for the active model |
| `/compact [instructions]` | Summarize older context with the configured provider (model call; original history remains archived) |
| `/login` | Native Codex browser login or an API key for a configured provider |
| `/logout` | Choose a provider and remove its stored credential inside Hibiscus |
| `/restore` | Restore a failed prompt and images for review, without sending |
| `/reconnect` | Restart the native runtime connection without replaying requests |
| `/help` | Show available commands |
| `/quit` or `/exit` | Leave Hibiscus |

Sessions belong to Hibiscus and are saved as workspace-scoped native JSONL under `~/.hibiscus/sessions/`. On resume, Hibiscus shows up to the **last five user turns** and their replies; the native runtime retains the full active history. One-shot prompts are saved as separate sessions.

Model errors should not end your chat. Hibiscus keeps the UI open after rate limits, authentication/subscription failures, and rejected commands. `/restore` lets you review a failed prompt, while `/reconnect` restarts the local runtime connection without automatically resending anything. Interrupted tools are never replayed automatically. [Recovery guide →](docs/error-handling.md)

## Keep it fresh

```sh
hibiscus update
hibiscus --version
```

For supported prebuilt installs, `hibiscus update` updates Hibiscus through GitHub Releases with checksum verification and atomic replacement. Restart after updating. Full-screen chat also checks for updates at most once a day and offers **Later / Update now**; offline checks don't block chat.

Set `HIBISCUS_NO_UPDATE_CHECK=1` to disable automatic checks. Custom install directories need `HIBISCUS_INSTALL_DIR`; source/Cargo installations should be updated with `cargo install --path . --locked` instead. [More about updates →](docs/installation.md)

## Hibiscus runtime

The native system prompt asks for direct, concrete prose. It preserves user-requested tone and code/quotations; this is prompt guidance, not a post-processing filter.

**A standalone Rust coding agent.** Hibiscus owns its agent loop, direct provider connections, authentication, native tools and session persistence. CLI and TUI share one in-process runtime and session.

- Hibiscus provides native **read, bash, edit, write and goal** tools. Covered write/edit operations create per-request checkpoints with recorded diffs and conflict-checked selective restore. [Undo file changes →](docs/checkpoints.md)
- Native Codex browser login, provider API keys and logout stay inside Hibiscus. API keys and Codex credentials are stored in private mode-0600 files. Authentication failures show recoverable errors. Logout asks for a provider and confirmation; it removes only that stored credential, not environment keys or provider-side access. [Authentication details →](docs/native-runtime.md#authentication)
- `NO_COLOR=1` disables colors. The TUI adapts to small terminals; `TERM=dumb` and piped input use plain input/output. `/cli` and `/tui` switch views while keeping the same runtime and session.
- Installation creates an empty `~/.hibiscus/extensions/`. Ask Hibi to create your own tools or slash commands, such as `/review`. Extensions can use any installed executable through the versioned JSON protocol. Native update/history/diff/rollback tools retain validated versions. [Extension authoring →](docs/extensions.md)

## Build & contribute

With Rust, Cargo, a C linker and Python 3.9+ for the development evaluation tests installed:

```sh
git clone --branch dev https://github.com/Rekabytes-Enterprise/hibiscus.git
cd hibiscus
cargo test --locked
cargo install --path . --locked
```

On Ubuntu/WSL, the linker prerequisite is `sudo apt install build-essential`. Node.js/npm are needed only for the npm installer tests. The shipped native executable has no Python or Node.js runtime dependency. To contribute, [fork the repository, branch from `dev`, and open a PR targeting `dev`](CONTRIBUTING.md#fork-branch-and-open-a-pull-request), not `main`.

| Explore | |
| :--- | :--- |
| [Changelog](CHANGELOG.md) | What's new in each version |
| [Installation & releases](docs/installation.md) | Platforms, installer options, updates, and publishing |
| [Terminal guide](docs/terminal.md) | Input, images, sessions, authentication, and compatibility |
| [Architecture](docs/architecture.md) | The Rust modules, native runtime and provider boundaries |
| [Development](docs/development.md) | Tests and manual verification |
| [Coding evaluations](docs/evaluations.md) | Isolated baseline/candidate comparisons with objective checks and explicit live-provider configuration |
| [Contributing](CONTRIBUTING.md) | Fork, branch, and submit a PR to `dev` |
| [MIT license](LICENSE) | Use, modify, redistribute, and sell copies under the MIT terms |

## Ownership and license

Hibiscus is a project of [Reka Bytes](https://reka-bytes.my/), the trading name of **Rekabytes Enterprise** (registration **202503277241 (IP0614333-M)**), based in Kuala Lumpur, Malaysia. Hibiscus is released under the [MIT license](LICENSE); this permits commercial use and resale when its notice is retained.

<p align="center">
  <img src="docs/assets/hibiscus.svg" width="32" height="32" alt=""><br>
  <sub>Built in Rust. Native tools. A little more bloom in your terminal.</sub>
</p>
