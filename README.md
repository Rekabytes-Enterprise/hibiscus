<div align="center">
<img src="docs/assets/hibiscus.svg" alt="Hibiscus" width="96" />

# Hibiscus

A standalone Rust coding agent with a terminal UI, direct provider connections and native tools.
</div>

## Native migration

The current source runs without the Pi SDK, Pi runtime or Node.js. Earlier published releases may still use Pi. Build this source to test the native runtime; a branch change does not publish a new binary or npm package.

```sh
cargo install --path . --locked
export OPENAI_API_KEY='your-provider-key'
hibiscus
```

You can also use `cargo run --locked --` without replacing your installed binary. API-key providers use API billing. Native Codex browser login uses your ChatGPT account and its limits. Existing Pi/Codex/Claude subscription credentials are not imported or reused.

The default catalog contains OpenAI `gpt-4.1`, Anthropic `claude-sonnet-4-5` and Codex `gpt-6.1-sol`. These are configurable model IDs, not a promise of account access or a list of the newest models. Use `ANTHROPIC_API_KEY` for Anthropic, select the model with `/models`, or configure another model/provider in `~/.hibiscus/config.json`. [Configuration and migration](docs/native-runtime.md).

## What it does

- Streams responses directly through OpenAI Responses, OpenAI-compatible Chat Completions or Anthropic Messages.
- Executes native `read`, `write`, `edit`, `bash` and `goal` tools.
- Shows approval dialogs before shell execution when approvals are enabled. Deny is the default.
- Saves conversations locally, supports resume, and preserves interrupted tool calls without replaying them.
- Keeps a searchable model picker, images, terminal selection/copy, Markdown tables, queued steering/followups and recoverable errors.
- Compacts context on request, or near a configured context limit when the provider supplies usage. Original history is archived locally.

## Commands

| Command | Action |
| --- | --- |
| `/models` | Search the configured provider/model catalog |
| `/login` | Browser Login → Codex, or API Key → configured provider |
| `/logout` | Confirm removal of a locally stored API key or Codex credential |
| `/approval [on\|off]` | Inspect or change the persistent shell approval setting |
| `/thinking [level]` | Select a supported Responses reasoning level |
| `/new` | Start another conversation |
| `/sessions` | Select a native session for this workspace |
| `/continue` | Resume the latest native session |
| `/compact [instructions]` | Summarize active context and archive the original history |
| `/restore` | Review a failed prompt without resending it |
| `/reconnect` | Restart the local runtime without replaying work |
| `/quit` | Exit |

Start with `hibiscus --continue` to resume, `hibiscus --sessions` to choose a session, or `hibiscus 'your prompt'` for one-shot output. Piped prompts produce plain output. Login/logout require fullscreen input.

Enter sends, Ctrl+Enter or Ctrl+J inserts a newline, and Esc cancels an active run. During a run, Enter submits steering for the next tool boundary; Ctrl+Q queues a followup. Image paste uses Alt+V on WSL and Ctrl+V elsewhere. Clipboard support depends on your desktop and terminal. [Terminal guide](docs/terminal.md).

## Data and credentials

Hibiscus uses `~/.hibiscus`, or the absolute directory in `HIBISCUS_HOME`:

- `config.json` contains providers, models and run limits.
- `auth.json` contains API keys entered through `/login`, stored with mode 0600.
- `codex-auth.json` contains native Codex access/refresh tokens, expiry and account ID, also mode 0600.
- `approval.json` stores the approval preference.
- `sessions/` holds versioned native conversations.
- `archives/` preserves history before compaction.
- `cache/` holds disposable update metadata.

An explicitly configured environment variable takes precedence over a stored key. Logout removes the selected stored credential; it does not unset environment variables or revoke provider access. Tools execute on your machine and are not sandboxed. Approval is a permission mechanism, not proof that a shell command is harmless.

## Installation and updates

The bundled installer downloads a checksum-verified Hibiscus binary and creates its data directory. It no longer installs Pi or Node. The npm wrapper still requires Node/npm to run the installer; the installed agent does not.

```sh
# Requires matching published native release assets and npm publication:
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
