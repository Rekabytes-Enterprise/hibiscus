# Extension process environment

## Problem and usage

Tools and slash commands converge in `extensions/process.rs`; explicit exec
checks start separately in `extensions/validation.rs`. Both previously inherited
the full parent environment. Shared `process.rs` also serves native shell tools,
whose environment behavior must remain unchanged.

An extension declares `"inheritEnv": ["GITHUB_TOKEN"]` in its existing versioned
manifest. Omission means no additional variables. Values are read only when a
process starts, never stored in its registry metadata. Missing declared names
produce a name-only error before execution.

## Shape

`extensions/environment.rs` owns `Environment(Vec<String>)`. Its private field
and `TryFrom<Vec<String>>` validate names once at the manifest boundary. Serde
deserialization uses that conversion. `Environment::command(program,
extension_dir, workspace) -> Result<tokio::process::Command>` clears inheritance,
copies a fixed baseline and declared values, then sets authoritative context
paths. Both extension process constructors use this factory. Argument building,
working directory, bounds, cancellation and process groups stay with their
existing owners.

The factory hides the complete inheritance policy behind one operation. There
is no generic subprocess-policy flag or second environment implementation.
Registry snapshots retain validated names, so managed updates, checks and
rollback naturally follow their selected manifest. The catalog exposes names
only. No resolved value enters the metadata or version store through this code.

## Synthesis and alternatives

Two independent read-only design candidates considered a policy applied to an
existing command and an extension-only command factory. The factory was chosen
because callers cannot construct an inheriting command and forget a later
sanitization step. Adding a policy mode to the generic subprocess runner was
rejected because native shell callers would have to understand extension policy.

The default includes PATH, HOME, TMPDIR, LANG, LC_ALL, LC_CTYPE, TZ and TERM when
set. Keeping HOME accepts ordinary user configuration discovery for compatibility;
excluding it would still not block filesystem access. Missing declared variables
fail before startup rather than silently disabling an integration. Existing
manifests remain valid, but programs relying on other variables need declarations.
There are no execution prompts or unrestricted-inheritance escape hatches.

## Verification and limits

The native integration tests start fresh Hibiscus subprocesses with synthetic
environment markers. They exercise all provider tool adapters, CLI/TUI slash
commands, exec/tool/command checks, missing names, and managed update/rollback.
They check actual child behavior without mutating the parallel test runner's
environment. Manifest tests cover bounds, malformed shapes, names and authoring
schema parity. Ordinary extension and process cancellation tests remain in place.

Extensions retain the user's OS access. HOME, installed programs and files can
still supply credentials, and explicitly declared secrets can be printed by an
extension. This change prevents implicit environment inheritance, not arbitrary
credential access or execution. No new sandbox boundary is claimed.
