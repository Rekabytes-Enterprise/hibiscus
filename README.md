<div align="center">
<img src="docs/assets/hibiscus.svg" alt="Hibiscus" width="96" />

# Hibiscus

A standalone Rust coding agent with a terminal UI, direct provider connections and native tools.
</div>

## Native migration

Versions 0.3.5 and later run without the Pi SDK, Pi runtime or Node.js. Releases before 0.3.5 use the previous runtime. Build the source with Rust/Cargo, or use the versioned native installer in the installation section below.

```sh
cargo install --path . --locked
export OPENAI_API_KEY='your-provider-key'
hibiscus
```

You can also use `cargo run --locked --` without replacing your installed binary. API-key providers use API billing. Native Codex browser login uses your ChatGPT account and its limits. Existing Pi/Codex/Claude subscription credentials are not imported or reused.

The default catalog contains OpenAI `gpt-4.1`, Anthropic `claude-sonnet-4-5` and Codex `gpt-6.1-sol`. These are configurable model IDs, not a promise of account access or a list of the newest models. Use `ANTHROPIC_API_KEY` for Anthropic, select the model with `/models`, or configure another model/provider in `~/.hibiscus/config.json`. [Configuration and migration](docs/native-runtime.md).

## What it does

- Streams responses directly through OpenAI Responses, OpenAI-compatible Chat Completions or Anthropic Messages.
- Executes native `read`, `write`, `edit`, `bash` and `goal` tools. Covered file edits have per-request checkpoints, recorded diffs and selective restore. [Undo file changes](docs/checkpoints.md).
- Runs validated native tools directly, including shell commands.
- Saves conversations locally, supports resume, and preserves interrupted tool calls without replaying them.
- Keeps a searchable model picker, images, terminal selection/copy, Markdown tables, queued steering/followups and recoverable errors.
- Compacts context on request, or near a configured context limit when the provider supplies usage. Original history is archived locally.

## Local extensions

Version 0.3.6 creates an empty `~/.hibiscus/extensions/` directory. Ask Hibi to create an extension with the tools or slash commands you want, such as “Create a project review extension with `/review`.” The agent writes and registers its manifest and source through native extension tools; you can use the new command in the same session. Extensions can use any installed executable through the versioned JSON protocol. Native update, history, diff and rollback tools keep validated versions while preserving the original source. [Extension authoring](docs/extensions.md).

## Commands

| Command | Action |
| --- | --- |
| `/cli` | Switch to inline chat in terminal scrollback |
| `/tui` | Return to the default fullscreen view |
| `/models` | Search the configured provider/model catalog |
| `/login` | Browser Login → Codex, or API Key → configured provider |
| Custom commands, e.g. `/review [input]` | Run an installed extension command locally |
| `/logout` | Confirm removal of a locally stored API key or Codex credential |
| `/thinking [level]` | Select a supported Responses reasoning level |
| `/new` | Start another conversation |
| `/sessions` | Select a native session for this workspace |
| `/continue` | Resume the latest native session |
| `/compact [instructions]` | Summarize active context and archive the original history |
| `/restore` | Review a failed prompt without resending it |
| `/reconnect` | Restart the local runtime without replaying work |
| `/output [list\|number]` | Inspect saved tool results locally without rerunning them |
| `/quit` | Exit |

`hibiscus` starts in TUI by default. Use `/cli` for inline terminal chat and `/tui` to return, keeping the same session. CLI shows a small text heading and command hints; the HIBI logo and bordered welcome panel belong to TUI. Start directly in CLI with `hibiscus --cli`, or resume with `hibiscus --cli --continue`. The welcome tips offer the other view and hide `/login` when any configured provider has credentials. You can still type `/login` to connect another provider.

Start with `hibiscus --continue` to resume, `hibiscus --sessions` to choose a session, or `hibiscus 'your prompt'` for one-shot output. Piped prompts produce plain output. Login/logout from CLI temporarily open the native fullscreen dialogs and return to CLI afterward.

Enter sends and Esc cancels an active run in either view. In CLI, Alt+Enter or Ctrl+J inserts a newline, arrows edit the draft, and multiline paste waits for Enter. In TUI, Ctrl+Enter or Ctrl+J inserts a newline. TUI supports steering at tool boundaries and Ctrl+Q followups during a run. Image paste uses Alt+V on WSL and Ctrl+V elsewhere. Clipboard support depends on your desktop and terminal. [Terminal guide](docs/terminal.md).

## Data and credentials

Hibiscus uses `~/.hibiscus`, or the absolute directory in `HIBISCUS_HOME`:

- `config.json` contains providers, models and run limits.
- `auth.json` contains API keys entered through `/login`, stored with mode 0600.
- `codex-auth.json` contains native Codex access/refresh tokens, expiry and account ID, also mode 0600.
- `sessions/` holds versioned native conversations.
- `archives/` preserves history before compaction.
- `cache/` holds disposable update metadata.

An explicitly configured environment variable takes precedence over a stored key. Logout removes the selected stored credential; it does not unset environment variables or revoke provider access. Tools execute on your machine and are not sandboxed. Shell commands execute directly in interactive and piped modes.

## Installation and updates

The bundled installer downloads a checksum-verified Hibiscus binary and creates its data directory. It no longer installs Pi or Node. The npm wrapper still requires Node/npm to run the installer; the installed agent does not.

```sh
# Native v0.3.6 installer, pinned to the matching GitHub release:
curl -fsSL https://raw.githubusercontent.com/Rekabytes-Enterprise/hibiscus/v0.3.6/install.sh | HIBISCUS_VERSION=v0.3.6 sh

# npm installer versions are published separately:
npx @rekabytes/hibiscus@latest
```

`hibiscus update` updates Hibiscus only for supported prebuilt installations. Source installs should use `cargo install --path . --locked` after updating the source. [Installation and releases](docs/installation.md).

## Development

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
sh tests/install.sh
sh tests/bump-version.sh
node tests/npm-install.mjs
```

Provider integration tests use local HTTP servers and synthetic keys, not real provider accounts. See [architecture](docs/architecture.md), [development](docs/development.md) and [contribution instructions](CONTRIBUTING.md). Pull requests target `dev`.

The development-only [coding evaluation suite](docs/evaluations.md) compares baseline and candidate instructions or extension implementations in fresh workspaces, with independent file/test checks and versioned JSON reports. Its default provider is an offline fixture; live evaluations require explicit configuration and `--live`.
