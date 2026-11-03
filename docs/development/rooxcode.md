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

Local fixtures exercise catalog discovery, metadata/header uniqueness, no-key
CLI/TUI access, real native tool continuation, session resume/migration,
compaction, cancellation, rejected catalogs, removed models, HTTP errors and
credential isolation. Other provider requests retain their key and carry no
Rooxcode headers. The fixture URL is configured on loopback; a unit check binds
the production default URL and verifies that it needs neither keys nor IDs.
No live production inference or deployed-gateway compatibility is claimed.
The ID-only catalog cannot describe context/image/reasoning capabilities, so
Hibiscus leaves them unknown or disabled and uses the gateway's output default.
Gateway quotas and context/tool limits remain authoritative.
