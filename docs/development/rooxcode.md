# Native Rooxcode provider

## Usage and contract

Start Hibiscus without credentials. When there is no explicit model selection, the runtime discovers the built-in Rooxcode URL,
selects the first available ID and uses Chat Completions. `/models` refreshes
that catalog without changing an existing selection. No model IDs are bundled.
The contract was read from the adjacent gateway repository's
`docs/hibiscus-client-access.md` and checked against its public catalog/error
implementation. No gateway source is changed.

## Shape

```text
Authentication::Rooxcode
rooxcode::install(&mut Config)
rooxcode::refresh(&mut Config) -> async Result<()>
rooxcode::SessionId(String) // validated non-nil hyphenated UUID
rooxcode::chat_request(&Provider, &Value, &SessionId) -> RequestBuilder
Session { rooxcode_session: SessionId, ... }
provider::request(&Provider, &Model, &Session, ...) -> async Result<Reply>
```

The Rooxcode module owns bounded catalog parsing, UUID generation, fixed error
messages, no-retry/no-redirect HTTP policy and the credential-free request path.
Configuration installs its default without rewriting user files. Runtime
snapshots own the effective catalog; idle refresh replaces only Rooxcode models.
Session owns identity and persistence, so generation, tools and compaction use
the same UUID. Existing hexadecimal session IDs and filenames stay compatible. An unrelated
API key never turns a new default conversation into a paid-provider request.
The UI uses `hasAccess`, separate from `hasCredentials`, to hide unnecessary
login hints. Provider login choices remain limited to credentialed methods.

## Synthesis decision

Two shapes were compared sequentially. A general anonymous authentication mode
with provider-supplied arbitrary headers would expose metadata and credential
policy across configuration, sessions and transport. It would also permit a key
to be attached accidentally to the public service. The selected dedicated
Rooxcode mode owns these decisions while reusing the existing Chat Completions
encoder, streaming parser, tool lifecycle and cancellation path.

Discovery is lazy for explicitly selected existing providers to avoid adding a
network prerequisite to their startup. Fresh no-key startup discovers once;
`/models` refreshes explicitly. Failed discovery clears stale Rooxcode entries,
reports the failure and leaves explicit model selection untouched. There is no
generation retry, fallback model, enrollment or infrastructure component.

## Verification and limits

### Request waiting

The adjacent gateway's `docs/request-queue.md` defines the v0.1.9 contract.
The shared SSE decoder separates comment frames from data frames. Only the
Rooxcode adapter interprets `rooxcode waiting` as capacity status. Other comments
remain keepalives. Role-only deltas do not clear waiting; the first nonempty
text/reasoning/refusal or tool-call delta does. A native `provider_status` event
updates the existing work phase in both views, never session/model messages.
In TUI, `Rooxcode is busy. Waiting…` replaces Working below the composer and
retains the existing flower animation and elapsed timer without a new tick loop.
Each tool continuation can enter waiting independently in the same conversation.

Queue errors can arrive as ordinary HTTP errors or inside HTTP200 SSE streams.
The Rooxcode boundary allowlists error codes, accepts integer embedded statuses
from 400 through 599, and bounds numeric retry delays to 0 through 86400 seconds.
Invalid fields use a fixed fallback diagnostic; raw service messages are never
echoed. Queue capacity failures remain separate from terminal daily quotas and
work budgets. Retry delays are advice for manual recovery, never a retry timer.

Timeout audit: Rooxcode discovery remains five seconds with a three-second
connect timeout. Generation retains its 600-second total deadline, enough for
the server's maximum 300-second queue wait plus its current 120-second generation
deadline. Comments do not reset it. Active prompt exchange has no shorter idle
deadline. Manual compaction retains its 600-second idle deadline; the 30-second
control-write deadline bounds delivery only. Cancellation retains the existing
10-second settlement safety ceiling; the queued TCP fixture settles and closes
the connection in under two seconds. Other providers are unchanged.

`tests/native_runtime/rooxcode/queue.rs` runs the real binary against local SSE
fixtures: waiting then text in both views, waiting then a native tool and its
continuation, Esc and observed TCP disconnect, all four queue errors and four
quota/budget errors over HTTP and SSE, failed-draft restore without replaying a
completed shell command, interrupted tool streams, and existing-provider comment
handling. Unit tests cover arbitrary chunk boundaries and invalid error fields.
These tests use short controlled waits rather than spending 300 seconds in a
queue or calling the deployed service. Full deadline coverage is a source audit,
not a wall-clock load test.

### Existing access checks

Local fixtures exercise catalog discovery, metadata/header uniqueness, no-key
CLI/TUI access, real native tool continuation, session resume/migration,
compaction, cancellation, rejected catalogs, removed models, HTTP errors and
credential isolation. Other provider requests retain their key and carry no
Rooxcode headers. The fixture URL is configured on loopback; a unit check binds
the production default URL and verifies that it needs neither keys nor IDs.
No live production inference or deployed-gateway compatibility is claimed.
An ID-only catalog leaves context unknown; optional verified context metadata
can supply a limit. Image/reasoning capabilities remain disabled. Hibiscus uses
the gateway's output default while reserving output space during budgeting.
Gateway quotas and context/tool limits remain authoritative.


## Request budgeting and compaction

Model discovery accepts optional context_window, max_request_bytes and max_messages.
Missing context stays unknown. Missing byte/message metadata uses the Rooxcode
10 MiB/256-message compatibility defaults, not a guessed model context window.
Hibiscus measures the exact Chat Completions body including system instructions
and extension tools before every Rooxcode generation, after steering inputs.
It compacts at 80 percent of either byte/message capacity or the configured
context window after reserving output. Without a tokenizer, serialized UTF-8
bytes are a conservative token estimate, so compaction can happen earlier than
an exact token count would require. Other providers retain their existing policy.

Compaction summarizes canonical history in bounded text chunks, carrying each
summary into the next request. The recent tail retains complete assistant/tool
result groups when it fits. Intermediate summaries never replace active history.
Only a successful final summary archives and replaces it; failure or cancellation
keeps the original conversation. Completed tools are never replayed. Oversized
fixed instructions/tools still fail clearly instead of looping or switching models.
Gateway request_too_large errors receive a fixed, sanitized diagnostic.
