# Current state

## v0.3.23 release in preparation

The owner now explicitly requests publication of the local context-budget work as a new Hibiscus patch. This supersedes the earlier local-only release-scope decision for these client changes. Preparing0.3.23through protected dev/main PRs, immutable tag and independently verified GitHub Release. No gateway deployment or npm publication is included. Rooxcode-only budget/compaction guards were checked in current source; Codex OAuth/Responses and existing compaction remain on their prior paths. Post-bump219unit+91native+evaluation launcher tests pass, including Codex transport/OAuth refresh/cancellation and other-provider regressions. Strict Clippy all targets/fuzzing, fmt/diff, curated fuzz replay, installer/version-bump and offline npm tests pass. Pinned working-tree Gitleaks reports zero findings. Hosted exact-head checks, immutable tag/build/provenance and public installer acceptance remain pending.

## v0.3.22 published and branches synchronized

The owner requested a release after the Rooxcode queue UX refinement. PR99 merged feature6c907a14494278790d78e03119beeb1e16ec05ed into dev973ca572e752f88f209a0a0c88af483dd77cf5a0. PR100 promoted the identical reviewed tree to main21c7caa77b4b6c21e5bc44149c385b100788d502. PR101 merged released main back into deveaf09f38a75918208422a8ac107decaad51eefb3. All three PRs passed exact-head checks, all required security/platform checks plus coverage/fuzzing, and review-thread gates without bypass. Main is an ancestor of dev, and their tree3ab0352640e20c863e739c81da7f937f4fb91cff is identical. Local main/dev refs are fast-forwarded; the checkout is main.

Annotated tagv0.3.22 objectbf2c16d8b8addb8c8975daaf6da20ed8c3ef901e resolves to exact main21c7caa. GitHub Release432919474 is published latest/stable at2026-11-03T16:10:47Z: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.22. No tag or old published asset was moved or replaced. Package/lock/npm/installer references are synchronized at0.3.22. This publication is a Git tag and GitHub Release, not npm/registry publication or deployment.

## Shipped behavior

Rooxcode v0.1.9 SSE comments remain transport keepalives. Its waiting marker replaces the existing work status below the TUI composer with `Rooxcode is busy. Waiting…`, retaining the flower animation and elapsed timer. CLI uses the same wording with its existing static presentation; NO_COLOR retains plain scrollback lines. Model text/reasoning/tool output clears waiting, while role-only deltas do not. Status never enters chat/session content. HTTP200 SSE errors validate embedded status/retry metadata and distinguish queue capacity from exhausted daily quotas/work budgets. Esc cancels the actual connection; /restore reviews the failed draft without automatic submission, provider switching or completed-tool replay. Anonymous headers and credential isolation remain intact.

The existing600-second Rooxcode total HTTP deadline covers maximum300-second server queue waiting plus current120-second generation. Keepalives do not extend it. No shorter active-prompt idle timer; existing compaction600, control delivery30 and cancellation safety ceiling10 remain unchanged, as do other providers. No gateway source changed.

Empty/whitespace-only interactive assistant chunks no longer create orphan hibi labels or fragment tool-only timelines. Bounded leading whitespace preserves indentation; piped/provider/session text remains intact. TUI replay suppresses old empty labels. Consecutive tools remain compact; green/red success/failure badges tint only their labels, with muted details and a gap before real assistant prose. Shell command bodies remain hidden in TUI summaries.

## Verification and limits

Post-bump cargo test --locked passes217unit+89native+1evaluation launcher21Python. Strict all-target Clippy with fuzzing, format/diff checks, curated fuzz replay, installer/version-bump and offline npm installer checks pass. Pinned Gitleaks8.30.1 working-tree and full committed-history scans report zero findings. All three PR coverage artifacts were independently matched to their tested source, with native executable coverage. Real local PTY captures verify below-composer status placement, multiple flower frames, clearing on output and one real shell execution in the same conversation. Screenshots are implementation observations, not live-service acceptance or approved visual baselines.

Tagged run41668594170 passed all nine source/test/compiler/transport/signing/publisher jobs. Independent source artifact13247790130 matches all274 Git blobs and executable modes, SHA256acf405b2fe1770e14268eb6f1e45c6886f4fb186fdffa3d6a5faaff950539d3e. All four archives match GitHub digests, SHA256SUMS, compiler receipts and cryptographic provenance binding exact source/tag/workflow/run. The verified macOS ARM64 binary passes version/signature checks and all18Rooxcode fixtures; the original local build was restored. Other platform binaries were inspected, not executed locally.

All six anonymous public downloads match the independently verified draft bytes. The public versioned installer matches tagged source and installs identical0.3.22bytes in a temporary home with config0600, cache/extensions0700 and noPi. No real provider credentials, production inference, global installation or gateway deployment. Full-duration queue/load and deployed service compatibility remain unverified. Default install/update remain checksum-only; provenance verification is explicit. Final progress records stay local outside the immutable tag.

Hosted macOS ARM64 capacity caused unstarted cancellations; bounded failed-job retries used unchanged source. One proactive PR100 cancellation raced runner assignment and wasted an attempt; its replacement passed before merge. Do not cancel queued jobs solely to seek capacity. Evidence and prevention are saved in Omni-graph.

## Separate work

This release did not implement the separately tracked work for issues31,32or34. The owner deferred security-score chasing and repository-age eligibility. No new score measurement, badge submission, Keychain or SBOM work was performed. Existing Best Practices owner facts and the assessment draft remain separate.


## Rooxcode context-budget changes prepared for v0.3.23

Owner initially limited publication to Rooxcode, then requested this Hibiscus release.
Client version references now use0.3.23. Discovery consumes optional
verified context_window/max_request_bytes/max_messages. Exact body measurement
runs after steering/system/extension schemas, before each Rooxcode generation.
Compaction triggers at80percent, reserves output, uses conservative UTF-8 bytes
as a token upper estimate, and processes bounded chunks carrying summaries.
Recent complete tool groups stay intact when they fit; summary failure preserves
original history. Safe request_too_large diagnostics replace the generic400.
Other providers and queue cancellation remain unchanged. Existing local working
record edits are preserved. Hibiscus publication is now authorized for this patch,
pending release verification.
Verification artifacts are under /private/tmp/hibiscus-context-*.log; full suite
passes219unit+91native+evaluation launcher, strict all-target Clippy with fuzzing
and format checks. Native fixtures verify multi-chunk continuation and preserved
history on413. No live upstream inference or production deployment.
