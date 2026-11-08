# Native runtime migration

Version 0.3.5 replaces Pi's SDK and subprocess runtime with Hibiscus-owned Rust code. Earlier releases use the previous runtime. No Pi installation, Node runtime or Pi credential/session store is used by the agent. Node/npm are needed only if you choose the separately published npx installer.

## Provider configuration

Rooxcode is built in at `https://api-rooxcode.reka-bytes.my/v1`. No account,
login, key or config entry is needed. Hibiscus fetches `GET /models` when needed
at startup and when opening `/models`, then uses the existing streaming Chat
Completions adapter. With no explicit model, the first discovered Rooxcode model starts a new
conversation, even if another provider key exists in the environment. Explicit selections
and existing providers remain unchanged; failures never select another provider.

Rooxcode chat requests contain `x-rooxcode-client: hibiscus`, the actual app
version in `x-rooxcode-version`, and a non-nil conversation UUID in
`x-rooxcode-session`. This UUID is stored as `rooxcodeSession` in the existing
session header, survives resume and compaction, and changes for a new session.
Older sessions receive a UUID when loaded and persist it before generation.
These copyable headers are client metadata, not authentication or separate quota
buckets. No provider key, Codex token or shared product secret is attached.
Rooxcode does not appear in the API-key login choices.

The catalog supplies IDs and may advertise verified context, request-byte and
message limits. Missing context stays unknown; image and reasoning capabilities
are not inferred. Rooxcode applies its own default output limit.

When a Rooxcode model advertises `reasoning: true` and `reasoning_efforts`,
`/thinking` lists **Default** plus that model's advertised choices. Use
`/thinking high`, for example, only when `high` is listed. Default omits
`reasoning_effort` and leaves reasoning to the upstream model's default; it does
not promise reasoning is disabled. Explicit `none` is separate and is sent only
when advertised. Missing or empty metadata offers only Default. The choice
survives tools, compaction and session resume. A successful catalog refresh or
model change that removes the choice resets it to Default with a short notice;
failed discovery preserves the choice and blocks unavailable inference.

Discovery has a five-second timeout, a 1 MiB response bound and at most 1,000
unique model IDs. Catalog failures and empty catalogs are reported, and `/models`
can refresh them. A removed selected model requires an explicit new selection.
Generation has no automatic retries or fallback; HTTP403 metadata rejection,
HTTP429 with numeric `Retry-After`, disabled inference and upstream errors use
fixed diagnostics without echoing private upstream bodies. Esc cancels the active
request; partially streamed tools never execute. Existing gateway request,
context, tool-count and shared-capacity limits still apply.

Rooxcode v0.1.9 queues temporary rate/concurrency limits on the server. Its
`: rooxcode waiting` SSE comments show `Rooxcode is busy. Waiting…` through
the existing work status, without adding chat text or completing the turn. Model
text, reasoning or tool-call output clears that status. Esc cancels the waiting
connection. Queue-full, per-network queue-full, queue-timeout and queue-disabled
errors are distinct from exhausted daily quotas and input/output work budgets.
HTTP200 SSE errors use their validated embedded status and optional numeric
retry delay. `/restore` recovers a failed draft for review; it sends nothing.

The Rooxcode HTTP deadline remains bounded at 600 seconds for queueing plus
generation. This covers the server's configurable maximum queue wait of 300
seconds and current generation deadline of 120 seconds. Keepalives do not extend
the deadline. Other providers retain their existing timeouts.

Before each Rooxcode generation, Hibiscus measures the full encoded request,
including instructions and tool schemas. It compacts at 80% of the applicable
byte, message or context budget, reserving output space. Large histories are
summarized in bounded chunks; recent complete tool groups are retained when
they fit. Failed or cancelled summaries preserve the original history. Without
an exact tokenizer, the estimate is conservative and may compact early. Other
providers keep their existing compaction policy.

For other providers, optionally create `~/.hibiscus/config.json`, or the equivalent file under `HIBISCUS_HOME`:

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

Supported `api` values are `responses`, `chat_completions` and `anthropic`. A custom OpenAI-compatible provider uses its own ID, base URL, key environment variable and model IDs. These providers use explicit configuration; Rooxcode and the OpenRouter and OpenCode Go providers below discover their catalogs. Only configure endpoints you trust to receive your prompts and credential. HTTPS is required except on loopback for local providers and tests. Redirects are disabled.

`model` is `provider/model-id`; `HIBISCUS_MODEL` can override it at launch. Explicit model preferences and resumed-session selections are preserved. Otherwise a new session discovers and selects an available Rooxcode model, with no paid-provider fallback. The header shows no active model when neither provider credentials nor an available Rooxcode catalog provide access. Credential presence does not prove remote model access. Model IDs may contain slashes. `maxSteps` bounds provider/tool iterations per user turn, from 1 to 1024. `maxTokens` is the output limit. Optional `contextWindow` must exceed `maxTokens` and should match the provider's actual capability. Unknown context limits stay unknown. `reasoning: true` enables off/low/medium/high for the Responses adapter; it is not inferred from a model name. Only mark `images: true` when the model supports them.

Usage comes from provider receipts. Missing usage and unconfigured pricing remain unknown; Hibiscus does not invent context counts or dollar costs. Automatic compaction starts when confirmed usage exceeds 80% of a configured context window. This is a scheduling threshold, not an exact tokenizer or a guarantee against provider context errors. `/compact` is available explicitly.

OpenRouter (`openrouter`, `https://openrouter.ai/api/v1`, `OPENROUTER_API_KEY`) and OpenCode Go (`opencode-go`, `https://opencode.ai/inference/go/openai/v1`, `OPENCODE_API_KEY`) are built in as Chat Completions providers. Add a key with `/login` → API Key, which then opens the model picker. When a key exists, Hibiscus fetches each provider's own model list (`/api/v1/models` and `https://opencode.ai/inference/go/v1/models`) with that key at startup and when opening `/models`. Context window, image input and reasoning support are copied from the catalog; models without tool support are skipped. No `max_tokens` is sent, so each provider applies its own output limit. A failed refresh keeps the last list and shows the provider's error. Listing `models` for one of these providers in `config.json` turns discovery off for it and uses that configuration instead.

`/models` lists only providers with access: a key, a Codex sign-in, or Rooxcode's anonymous catalog. The TUI picker has one tab per provider plus All, opens on the current model's provider, and switches with Tab or ←/→; search covers the open tab, and All also matches provider names. Rows show the context window, image and reasoning support each catalog reports. In the CLI, more than 50 models print a provider summary instead of one numbered list. `/models <provider> [search]` narrows the list (or opens that tab in the TUI) and `/models <provider>/<model>` selects a model directly.

## Authentication

Welcome tips and TUI slash suggestions hide `/login` when a configured provider has credentials or an available Rooxcode model provides anonymous access. The command remains available to type for another provider. This distinguishes anonymous model access from credential presence; neither guarantees that a generation request will succeed. CLI opens login/logout in the native fullscreen dialogs and returns to inline chat afterward.

`/login` offers **Browser Login** and **API Key**. Browser Login → Codex opens OpenAI's browser authorization page. No personal OAuth client ID is needed. Hibiscus implements the Codex public-client authorization-code flow with S256 PKCE and a fresh random state. It listens on `127.0.0.1:1455` for `http://localhost:1455/auth/callback`. If that port is busy, login reports a recoverable error. If the browser cannot return automatically, paste the complete redirect URL into the masked dialog. The target and state must match the pending attempt. Ctrl+Y copies the sign-in link; Esc cancels without replacing a stored credential.

Codex tokens live in Hibiscus's own `codex-auth.json`, mode 0600, with expiry and account ID. Before requests, tokens near expiry are refreshed through OpenAI and replaced atomically. A separate private lock serializes refresh across Hibiscus processes. Failed refreshes preserve the previous file and report a recoverable error. Tokens are sent only to the fixed Codex subscription Responses endpoint. Successful login selects the first configured model for the provider you authenticated in the current chat. Login alone does not prove model access.

The protocol reference is the installed Pi 1.1.0 **legacy Codex** adapter, independently implemented in Rust. This uses the public Codex client ID and ChatGPT's Codex backend. It is distinct from the newer [Sign in with ChatGPT open-source flow](https://developers.openai.com/siwc/token-sharing-open-source/sign-in), which dynamically registers an app client and uses the public Responses API. Hibiscus does not currently implement that separate flow or Codex device-code login. No Pi code, Node process or SDK is loaded at runtime. Real-account login and inference still require manual verification.

The built-in `openai-codex` provider uses `authentication: "codex"`, `api: "responses"` and the fixed `baseUrl: "https://chatgpt.com/backend-api/codex"`. While signed in, its models come from the plan's own list (`GET /models` on that endpoint), fetched when the engine starts and when `/models` opens: the models Codex lists, with their context windows, image support and thinking levels (`/thinking` offers exactly those levels). Hidden models are left out. The endpoint lists a model only to clients at least as new as its minimum Codex version, so the request carries the current official Codex release, looked up from npm (`@openai/codex`) once each time the engine starts and reused for that run. The answer is saved in `codex-client-version` in the Hibiscus home only as a fallback: without npm, Hibiscus uses the last version seen, then one pinned in Hibiscus. If the fetch fails, the last list stays; before the first fetch, `gpt-6.1-sol` is the fallback. Writing an `openai-codex` entry with `models` in `config.json` turns discovery off and uses that list instead. The subscription endpoint does not accept `max_output_tokens`, so `maxTokens` is not sent as an output cap there.

API Key → provider masks key entry. Keys are stored in Hibiscus's own `auth.json`, mode 0600, using atomic replacement. A configured key environment variable takes precedence for API-key providers. Stored keys are not validated until a provider request. `/logout` requires selection and confirmation, removes the selected local key or Codex token file and reloads runtime state. It does not revoke the remote grant or unset environment variables.

From v0.3.19, API-key credential files are limited to 128 KiB. The reader opens
the final path component without following a symlink and without waiting for a
FIFO writer, validates the opened file's regular-file type and private permissions,
then reads from that same handle with a second byte limit. Replacing the path
after opening does not redirect that read. Parse/read failures use fixed messages
that do not include credential values or private paths. Saves enforce the same
size cap before atomic replacement; rejected files and writes remain unchanged.

The JSON format and normal login flow are unchanged. If an existing store is
oversized or malformed, keep any backup private and restore a valid, smaller
credential file before retrying `/login`; Hibiscus does not truncate or overwrite
the rejected store. A missing file still means no saved keys. These checks do not
freeze an opened file against another process that can modify its contents, nor
prevent programs running as the same user from reading accessible credentials.

Keys/tokens in `.pi`, Codex or Claude stores are never read. Claude browser login is not implemented. Anthropic directs third-party applications to API keys or a supported cloud provider: [authentication guidance](https://support.claude.com/en/articles/13189465-log-in-to-your-claude-account).

## Existing data

From v0.3.27, legacy configs containing `codexContextWindow` start automatically with native defaults for missing settings. Update Hibiscus and restart; no config rename, reset or manual migration is needed. The retired blanket context override is ignored. Existing native providers, model preferences and other valid settings still apply, and startup leaves the original config file intact. Invalid native settings or malformed JSON still report an error rather than resetting the configuration.

New sessions use a versioned Hibiscus JSONL format in `sessions/`. Resume and session selection stay scoped to the current canonical workspace. Interrupted native tool calls receive an explicit unknown-outcome result on resume; they are never re-executed automatically. An original Pi session is rejected rather than interpreted as native history. There is no automatic Pi session or credential importer in this version. Existing files outside Hibiscus remain untouched.

Compaction stores a private copy of the previous JSONL under `archives/`, then replaces active context with the summary. Archives do not appear as duplicate live sessions. If summarization fails or is cancelled, active history is unchanged. Archives may contain private conversations and file contents; do not publish them.

## Tools

The native registry owns `read`, `write`, `edit`, `bash` and `goal`. Tool arguments are validated before dispatch. Tools run only after an authoritative complete provider response, never from partial streamed arguments. Results retain tool-call IDs so continuation requests can pair each call/result.

Read accepts UTF-8 regular files up to 16 MiB and returns bounded line ranges. Write replaces file contents atomically. Edit requires exactly one occurrence of `oldText`, preserves file mode and returns a diff only after writing. Bash uses a process group, a maximum 600-second timeout and bounded stdout/stderr. Cancellation kills the group before reporting settlement. Goal tracks an explicit numbered checklist.

Native `write`/`edit` also capture covered workspace files before mutation. `checkpoint_history`, `checkpoint_diff`, `checkpoint_restore` and `checkpoint_prune` manage per-request recovery while preserving existing user work and Git state. Shell/extension changes and excluded paths are outside that guarantee. [Checkpoint coverage and recovery](checkpoints.md).

Native tools execute directly, including shell commands in interactive and piped modes. There is no approval prompt or enable/disable setting. Old `approval.json` files are ignored and left untouched. Argument validation, timeouts, cancellation and the complete-provider-response requirement still apply. Tools are not sandboxed. Provider/configuration errors remain recoverable in interactive chat; one-shot/piped failures exit nonzero.

## Implementation and verification

`src/engine/` owns configuration, credentials, sessions, providers, tools, typed lifecycle events and the agent loop, and never imports terminal code. `src/client/` is the terminal's side of the connection: the worker client, bounded transport, run state and user-facing errors. The existing terminal adapter consumes bounded presentation records over a local in-process socket. It does not start another executable. Queued steering is inserted at a tool boundary; followups wait for the current turn. Settlement follows tool results and queue delivery.

The former Pi-only integration fixtures were retired. Their history remains in Git. Current native integration tests use local HTTP fixtures to exercise the actual CLI, all three provider protocols, file/shell tools, direct shell execution, cancellation, sessions, compaction, queues and masked credentials. Existing TUI/transport unit coverage remains active. Mock success is not real-provider verification.

The bounded native Mermaid renderer handles simple linear top-to-bottom flowcharts. Unsupported graphs remain source rather than showing a misleading layout. The old optional JavaScript renderer is gone. Desktop clipboard helpers and shell commands can still require system executables; they are separate from the agent runtime.
