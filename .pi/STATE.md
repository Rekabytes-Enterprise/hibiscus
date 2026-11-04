# Current state

## v0.3.24 release preparation

The owner requested a new release after reviewing the TUI tool-heading fix and a real CLI screenshot. Prepare v0.3.24 through the existing Hibiscus dev → main PR flow, then create an immutable tag, independently verify the draft assets and publish the stable GitHub Release. No npm registry publication or deployment is requested.

The TUI now shows one hibi heading above the first genuine tool activity or visible assistant prose in each user turn. Later tools/prose reuse it; a new user message resets it. Empty provider chunks and ordinary notices create no orphan headings. Cache complete user turns so live streaming and replay match uncached formatting. Runtime/providers and CLI behavior are unchanged.

Pre-bump verification: 220 unit and 91 native fixture tests, strict all-target Clippy and fmt/diff checks passed. Real isolated TUI/CLI captures executed reads, a failing test, an edit, passing tests and Bash checks through eight loopback responses; images inspected. No live account/provider inference. Post-bump full Rust tests (220 unit, 91 native and the Python evaluation launcher), strict Clippy with fuzzing, curated fuzz replay, fmt/diff, installer/version-bump and offline npm checks pass. Secret scan and remote PR/release gates are pending.

## Published baseline

v0.3.23 is published latest/stable from main 3071c7549b3e1e888a98d115a3debea5706fe132. Dev 1b3b1cc8d20d9413df7147fa422be96779d49b1f has the same tree and includes main as an ancestor. Its release workflow, signed platform archives, anonymous public downloads and temporary-home installer were verified. Existing releases/tags must remain immutable.

The owner deferred security-score chasing and repository-age eligibility. Separate issue 31/32/34 work, Keychain/SBOM and the Best Practices assessment are unchanged.
