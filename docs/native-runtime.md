# Native runtime migration

Version 0.3.5 replaces Pi's SDK and subprocess runtime with Hibiscus-owned Rust code. Earlier releases use the previous runtime. No Pi installation, Node runtime or Pi credential/session store is used by the agent. Node/npm are needed only if you choose the separately published npx installer.

## Provider configuration

Create `~/.hibiscus/config.json`, or the equivalent file under `HIBISCUS_HOME`:

```json
{
  "model": "openai/gpt-4.1",
  "maxSteps": 64,
  "providers": [
    {
      "id": "openai",
      "baseUrl": "https://api.openai.com/v1",
      "api": "responses",
      "apiKeyEnv": "OPENAI_API_KEY",
      "models": [
        { "id": "gpt-4.1", "maxTokens": 4096, "images": true }
      ]
    },
    {
      "id": "anthropic",
      "baseUrl": "https://api.anthropic.com/v1",
      "api": "anthropic",
      "apiKeyEnv": "ANTHROPIC_API_KEY",
      "models": [
        { "id": "claude-sonnet-4-5", "maxTokens": 4096, "images": true }
      ]
    }
  ]
}
```

Supported `api` values are `responses`, `chat_completions` and `anthropic`. A custom OpenAI-compatible provider uses its own ID, base URL, key environment variable and model IDs. The catalog is explicit configuration, not discovery through another agent. Only configure endpoints you trust to receive your prompts and credential. HTTPS is required except on loopback for local providers and tests. Redirects are disabled.

`model` is `provider/model-id`; `HIBISCUS_MODEL` can override it at launch. Without an explicit preference, a new session selects the first configured model whose provider has a stored credential or configured environment key. The header shows no active model when that credential is absent. Credential presence does not prove remote model access. Model IDs may contain slashes. `maxSteps` bounds provider/tool iterations per user turn, from 1 to 1024. `maxTokens` is the output limit. Optional `contextWindow` must exceed `maxTokens` and should match the provider's actual capability. Unknown context limits stay unknown. `reasoning: true` enables off/low/medium/high for the Responses adapter; it is not inferred from a model name. Only mark `images: true` when the model supports them.

Usage comes from provider receipts. Missing usage and unconfigured pricing remain unknown; Hibiscus does not invent context counts or dollar costs. Automatic compaction starts when confirmed usage exceeds 80% of a configured context window. This is a scheduling threshold, not an exact tokenizer or a guarantee against provider context errors. `/compact` is available explicitly.

## Authentication

Welcome tips and TUI slash suggestions hide `/login` when any configured provider has a native credential or configured environment key. The command remains available to type for another provider. This checks local credential presence, not successful provider access. CLI opens login/logout in the native fullscreen dialogs and returns to inline chat afterward.

`/login` offers **Browser Login** and **API Key**. Browser Login → Codex opens OpenAI's browser authorization page. No personal OAuth client ID is needed. Hibiscus implements the Codex public-client authorization-code flow with S256 PKCE and a fresh random state. It listens on `127.0.0.1:1455` for `http://localhost:1455/auth/callback`. If that port is busy, login reports a recoverable error. If the browser cannot return automatically, paste the complete redirect URL into the masked dialog. The target and state must match the pending attempt. Ctrl+Y copies the sign-in link; Esc cancels without replacing a stored credential.

Codex tokens live in Hibiscus's own `codex-auth.json`, mode 0600, with expiry and account ID. Before requests, tokens near expiry are refreshed through OpenAI and replaced atomically. A separate private lock serializes refresh across Hibiscus processes. Failed refreshes preserve the previous file and report a recoverable error. Tokens are sent only to the fixed Codex subscription Responses endpoint. Successful login selects the first configured model for the provider you authenticated in the current chat. Login alone does not prove model access.

The protocol reference is the installed Pi 1.1.0 **legacy Codex** adapter, independently implemented in Rust. This uses the public Codex client ID and ChatGPT's Codex backend. It is distinct from the newer [Sign in with ChatGPT open-source flow](https://developers.openai.com/siwc/token-sharing-open-source/sign-in), which dynamically registers an app client and uses the public Responses API. Hibiscus does not currently implement that separate flow or Codex device-code login. No Pi code, Node process or SDK is loaded at runtime. Real-account login and inference still require manual verification.

The built-in `openai-codex` provider uses `authentication: "codex"`, `api: "responses"` and the fixed `baseUrl: "https://chatgpt.com/backend-api/codex"`. It defaults to `gpt-6.1-sol`; customize its `models` in `config.json` when needed. The native catalog is explicit configuration, not a live account catalog. The model ID does not guarantee subscription access. Context limits remain unknown unless configured. The subscription endpoint does not accept `max_output_tokens`, so `maxTokens` is not sent as an output cap there.

API Key → provider masks key entry. Keys are stored in Hibiscus's own `auth.json`, mode 0600, using atomic replacement. A configured key environment variable takes precedence for API-key providers. Stored keys are not validated until a provider request. `/logout` requires selection and confirmation, removes the selected local key or Codex token file and reloads runtime state. It does not revoke the remote grant or unset environment variables.

Keys/tokens in `.pi`, Codex or Claude stores are never read. Claude browser login is not implemented. Anthropic directs third-party applications to API keys or a supported cloud provider: [authentication guidance](https://support.claude.com/en/articles/13189465-log-in-to-your-claude-account).

## Existing data

A legacy config containing only `codexContextWindow` stops with migration guidance. Back it up and replace it with an explicit native provider/model configuration. The old blanket context override is not applied to direct API models.

New sessions use a versioned Hibiscus JSONL format in `sessions/`. Resume and session selection stay scoped to the current canonical workspace. Interrupted native tool calls receive an explicit unknown-outcome result on resume; they are never re-executed automatically. An original Pi session is rejected rather than interpreted as native history. There is no automatic Pi session or credential importer in this version. Existing files outside Hibiscus remain untouched.

Compaction stores a private copy of the previous JSONL under `archives/`, then replaces active context with the summary. Archives do not appear as duplicate live sessions. If summarization fails or is cancelled, active history is unchanged. Archives may contain private conversations and file contents; do not publish them.

## Tools

The native registry owns `read`, `write`, `edit`, `bash` and `goal`. Tool arguments are validated before dispatch. Tools run only after an authoritative complete provider response, never from partial streamed arguments. Results retain tool-call IDs so continuation requests can pair each call/result.

Read accepts UTF-8 regular files up to 16 MiB and returns bounded line ranges. Write replaces file contents atomically. Edit requires exactly one occurrence of `oldText`, preserves file mode and returns a diff only after writing. Bash uses a process group, a maximum 600-second timeout and bounded stdout/stderr. Cancellation kills the group before reporting settlement. Goal tracks an explicit numbered checklist.

Native `write`/`edit` also capture covered workspace files before mutation. `checkpoint_history`, `checkpoint_diff`, `checkpoint_restore` and `checkpoint_prune` manage per-request recovery while preserving existing user work and Git state. Shell/extension changes and excluded paths are outside that guarantee. [Checkpoint coverage and recovery](checkpoints.md).

Native tools execute directly, including shell commands in interactive and piped modes. There is no approval prompt or enable/disable setting. Old `approval.json` files are ignored and left untouched. Argument validation, timeouts, cancellation and the complete-provider-response requirement still apply. Tools are not sandboxed. Provider/configuration errors remain recoverable in interactive chat; one-shot/piped failures exit nonzero.

## Implementation and verification

`src/runtime/` owns configuration, credentials, sessions, providers, tools, typed lifecycle events and the agent loop. The existing terminal adapter consumes bounded presentation records over a local in-process socket. It does not start another executable. Queued steering is inserted at a tool boundary; followups wait for the current turn. Settlement follows tool results and queue delivery.

The former Pi-only integration fixtures were retired. Their history remains in Git. Current native integration tests use local HTTP fixtures to exercise the actual CLI, all three provider protocols, file/shell tools, direct shell execution, cancellation, sessions, compaction, queues and masked credentials. Existing TUI/transport unit coverage remains active. Mock success is not real-provider verification.

The bounded native Mermaid renderer handles simple linear top-to-bottom flowcharts. Unsupported graphs remain source rather than showing a misleading layout. The old optional JavaScript renderer is gone. Desktop clipboard helpers and shell commands can still require system executables; they are separate from the agent runtime.
