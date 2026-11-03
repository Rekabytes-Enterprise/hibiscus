# Current state

## Native Rooxcode v0.3.21 release in preparation

Built-in public Rooxcode provider discovers GET /models and reuses the native Chat Completions stream/tool/cancellation adapter. No account, API key, enrollment or backend change. New conversations without an explicit model select the first discovered Rooxcode model even when unrelated credentials exist. Explicit config/environment selections and saved-session selections stay unchanged. No provider/model fallback or generation retry.

Each generation includes exactly one client, actual application version and non-nil hyphenated conversation UUID header. The UUID persists in the session-v1 header through tool continuations, resume and compaction; legacy sessions migrate before generation, and /new rotates it. Dedicated no-key authentication cannot use stored/environment credentials, OAuth tokens or authenticated redirects. Catalog refresh replaces only effective Rooxcode models without rewriting user config.

Bounded discovery, empty/malformed catalogs, removed models, metadata403, numeric Retry-After429, disabled inference, upstream and midstream errors have fixed diagnostics. Login tips recognize anonymous access. The ID-only catalog leaves context unknown and images/reasoning disabled; requests use the gateway output default.

Verification: cargo test --locked passes213unit+81native+1evaluationlauncher21Python. Ten Rooxcode integration fixtures exercise discovery/no-key CLI/TUI, streaming, real native tools/continuation, metadata uniqueness/version, stored/environment credential isolation, UUID persistence/rotation/legacy migration/compaction, cancellation and error/no-fallback behavior. Existing providers pass regressions. Strict Clippy all-targets with fuzzing, fmt, curated fuzz replay, installer/version/offline npm and diff checks pass. All inference uses local fixtures; production/staging gateway UAT and real upstream behavior remain unverified. No dependencies changed. Owner subsequently requested a new release; synchronized package and installer references now use0.3.21.

Source and verification notes: src/runtime/rooxcode.rs, tests/native_runtime/rooxcode.rs and docs/development/rooxcode.md. Exact adjacent gateway docs/hibiscus-client-access.md and catalog/error source were inspected. Gateway source remains unchanged. The implementation task remained local. The owner subsequently requested a new release, authorizing the existing Hibiscus dev-to-main PR and verified native GitHub Release flow. Publication and hosted checks remain pending; no registry publication or gateway deployment is included.

## Released baseline and remaining work

Local main remains b68f76be887745b7573b4faf5f5cc5577bcc72c4, published v0.3.20. Issue33 and its Mokara task are complete; PR93/94/95 merged and released main was synchronized into dev820be7e098e7a2145bde4be98cdfa4b107599ef6. Release432060087 and tagged run41540970412 were independently verified, including checksums, signed provenance, public installer and loopback schema fixtures. Full historical evidence remains in Omni-graph and release records. Prior final progress notes were local outside the immutable tag and must not be republished using old consent.

Issues31,32and34 remain separately tracked; no implementation started here. Owner deferred score chasing and repository-age eligibility. No badge submission or new security-score measurement performed. Best Practices owner facts, Keychain and SBOM remain separate unimplemented recommendations.
