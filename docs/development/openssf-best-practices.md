# OpenSSF Best Practices passing assessment

Draft assessed on 2026-11-02 against released [v0.3.16](https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.16), source `f922054815e7f1e828b447a8bfaa586b5d7f5bb5`. The documentation corrections in this revision are identified separately from that runtime evidence. This assessment covers the Hibiscus CLI/TUI repository, not a gateway or another application.

This is an evidence checklist and application draft. This assessment has not created or updated a BadgeApp entry, and no badge is claimed. The existing OpenSSF Scorecard result is a separate assessment; publishing this checklist does not by itself change it. Before submission, search BadgeApp for the exact repository URL and reuse an existing owner entry if one exists.

The [official passing criteria](https://www.bestpractices.dev/en/criteria/0?details=true&rationale=true) contain **67 items**: 43 MUST/MUST NOT, 10 SHOULD/SHOULD NOT and 14 SUGGESTED. The fetched criteria HTML had SHA-256 `0bd40483603fc71b91b5787991e46e3d5dfac81a5fafba5cd3864b943348787a`. IDs below match that complete set. Wording is summarized; the official criteria and details govern the answers.

The [program's rules](https://www.bestpractices.dev/en/criteria_discussion#achieving-a-badge) require mandatory items to be met or an allowed N/A, SHOULD items to be met or explained, and suggestions to be considered. This table is not BadgeApp's computed progress or an independent security audit.

## Solo-maintainer scope

The owner confirmed that Hibiscus currently has one maintainer. The passing criteria do not impose a second reviewer; the [program explicitly accommodates single-person projects](https://www.bestpractices.dev/en/criteria_discussion#non-criteria). Keep the existing protected checks and single-maintainer review policy. Do not claim AI review is independent human review or enable a review rule that the maintainer cannot satisfy. Gold-level requirements and the Scorecard Code-Review score are separate topics.

## Current readiness

| Evidence label | Count | Meaning |
| --- | ---: | --- |
| Evidence | 57 | A proposed Met answer has inspected source, public metadata or actual run evidence. The maintainer still owns the attestation. |
| Documented policy | 2 | The contribution policy in this revision explicitly requires tests for major new features. Cite the approved public revision when submitting. |
| Owner check | 3 | Release identifier history and unresolved known-issue status still need explicit confirmation. |
| Learning needed | 2 | The maintainer requested beginner guidance; both mandatory knowledge criteria remain Unmet. |
| N/A | 2 | The official details permit these scoped exceptions, including no vulnerability reports received. |
| Unmet suggestion | 1 | Considered and honestly left Unmet; no fabricated coverage percentage. |

On 2026-11-02 the owner confirmed no vulnerability reports have been received and requested beginner security guidance. The response-timing criterion can therefore be N/A; the two knowledge requirements remain Unmet until learning and reassessment. The remaining owner checks concern release identifiers and known unresolved issues, which a statement about received reports alone does not establish. Review the documented report classification and cite an approved public revision of the policies before submission. No paid service, second maintainer or new runtime dependency is needed. The coverage suggestion is not a mandatory blocker.

## Criteria checklist

“Met” below is a proposed form answer supported by the stated evidence, not a badge award. “Unknown” must not be submitted as Met without resolving the reason. Source details and URLs for E01–E10 follow the table.

| Criterion | Evidence label / proposed answer | Evidence and limitation |
| --- | --- | --- |
| `description_good` · MUST | Evidence / Met | E01: README and contribution guide describe the native agent, installation, feedback and dev-targeted PR workflow. |
| `interact` · MUST | Evidence / Met | E01: README and contribution guide describe the native agent, installation, feedback and dev-targeted PR workflow. |
| `contribution` · MUST | Evidence / Met | E01: README and contribution guide describe the native agent, installation, feedback and dev-targeted PR workflow. |
| `contribution_requirements` · SHOULD | Evidence / Met | E01: README and contribution guide describe the native agent, installation, feedback and dev-targeted PR workflow. |
| `floss_license` · MUST | Evidence / Met | E01: Top-level MIT license; Cargo and npm metadata identify MIT. |
| `floss_license_osi` · SUGGESTED | Evidence / Met | E01: Top-level MIT license; Cargo and npm metadata identify MIT. |
| `license_location` · MUST | Evidence / Met | E01: Top-level MIT license; Cargo and npm metadata identify MIT. |
| `documentation_basics` · MUST | Evidence / Met | E02: Installation, safe-use limits, CLI help, configuration and extension request/reply interfaces are documented. The index corrections in this revision improve navigation. |
| `documentation_interface` · MUST | Evidence / Met | E02: Installation, safe-use limits, CLI help, configuration and extension request/reply interfaces are documented. The index corrections in this revision improve navigation. |
| `sites_https` · MUST | Evidence / Met | E01 E03: Canonical repository, downloads and discussion URLs use HTTPS. Public issue/PR discussions and user/developer guides are in English. |
| `discussion` · MUST | Evidence / Met | E01 E03: Canonical repository, downloads and discussion URLs use HTTPS. Public issue/PR discussions and user/developer guides are in English. |
| `english` · SHOULD | Evidence / Met | E01 E03: Canonical repository, downloads and discussion URLs use HTTPS. Public issue/PR discussions and user/developer guides are in English. |
| `maintained` · MUST | Evidence / Met | E03: Repository is active and unarchived; recent changes and releases are visible. The separate Scorecard project-age penalty is not a passing-badge age requirement. |
| `repo_public` · MUST | Evidence / Met | E03: Public Git repository retains dated commits, development branches, intermediate changes and PRs. |
| `repo_track` · MUST | Evidence / Met | E03: Public Git repository retains dated commits, development branches, intermediate changes and PRs. |
| `repo_interim` · MUST | Evidence / Met | E03: Public Git repository retains dated commits, development branches, intermediate changes and PRs. |
| `repo_distributed` · SUGGESTED | Evidence / Met | E03: Public Git repository retains dated commits, development branches, intermediate changes and PRs. |
| `version_unique` · MUST | Evidence / Met | E04: Published versions use unique vMAJOR.MINOR.PATCH tags; package metadata and the bump helper keep version references aligned. |
| `version_semver` · SUGGESTED | Evidence / Met | E04: Published versions use unique vMAJOR.MINOR.PATCH tags; package metadata and the bump helper keep version references aligned. |
| `version_tags` · SUGGESTED | Evidence / Met | E04: Published versions use unique vMAJOR.MINOR.PATCH tags; package metadata and the bump helper keep version references aligned. |
| `release_notes` · MUST | Evidence / Met | E04: All 29 published versions have a summary in current main CHANGELOG.md or their GitHub release body. Six oldest tags lack their own notes; their summaries are present in the published main changelog. |
| `release_notes_vulns` · MUST | Owner check / Unknown | E04 E09: No public repository advisory was returned. Confirm whether any released runtime fix had an assigned public CVE or equivalent that its notes need to identify; otherwise explain the allowed N/A. |
| `report_process` · MUST | Evidence / Met | E01 E03: Public GitHub issues provide intake, searchable discussion and archived resolution; the security policy redirects sensitive reports to private intake. |
| `report_tracker` · SHOULD | Evidence / Met | E01 E03: Public GitHub issues provide intake, searchable discussion and archived resolution; the security policy redirects sensitive reports to private intake. |
| `report_responses` · MUST | Evidence / Met | E03: Conservatively classify #23 as a bug/behavior report: one report, one completed closure by the maintainer in the chosen two-month window. No 100% claim is made about undisclosed intake channels. |
| `enhancement_responses` · SHOULD | Evidence / Met | E03: The other 18 public items are enhancement/maintenance requests. Ten have maintainer closing events, exceeding half even when the additional bot-completed item is not counted as acknowledgement. All seven open requests remain in the denominator. |
| `report_archive` · MUST | Evidence / Met | E01 E03: Public GitHub issues provide intake, searchable discussion and archived resolution; the security policy redirects sensitive reports to private intake. |
| `vulnerability_report_process` · MUST | Evidence / Met | E09: SECURITY.md describes the private GitHub reporting form, fallback and coordinated handling; private reporting read back enabled. No private report content was read. |
| `vulnerability_report_private` · MUST | Evidence / Met | E09: SECURITY.md describes the private GitHub reporting form, fallback and coordinated handling; private reporting read back enabled. No private report content was read. |
| `vulnerability_report_response` · MUST | N/A | E09: The owner confirmed on 2026-11-02 that no vulnerability reports, including private reports, have been received. The criterion explicitly permits N/A when none were received during the last six months. This is not a measured response time or a future SLA. |
| `build` · MUST | Evidence / Met | E05: Cargo/LLVM and a C linker build the native source. The Linux musl build provides a FLOSS-tool build path; macOS SDK use does not make Linux builds depend on Apple tools. |
| `build_common_tools` · SUGGESTED | Evidence / Met | E05: Cargo/LLVM and a C linker build the native source. The Linux musl build provides a FLOSS-tool build path; macOS SDK use does not make Linux builds depend on Apple tools. |
| `build_floss_tools` · SHOULD | Evidence / Met | E05: Cargo/LLVM and a C linker build the native source. The Linux musl build provides a FLOSS-tool build path; macOS SDK use does not make Linux builds depend on Apple tools. |
| `test` · MUST | Evidence / Met | E05 E06: The MIT-licensed suite runs with cargo test --locked; development docs list additional installer, npm and Python prerequisites and checks. |
| `test_invocation` · SHOULD | Evidence / Met | E05 E06: The MIT-licensed suite runs with cargo test --locked; development docs list additional installer, npm and Python prerequisites and checks. |
| `test_most` · SUGGESTED | Unmet suggestion / Unmet | E06: Local Rust source/branch coverage is now measured separately, with inline test code and platform limitations documented. The initial 66.19% source branch figure is below the criterion's 80% guidance and is not a whole-project production-only measurement. Keep Unmet. |
| `test_continuous_integration` · SUGGESTED | Evidence / Met | E05: GitHub Actions runs checks for PRs and pushes to dev/main, including Linux and macOS tests. |
| `test_policy` · MUST | Documented policy / Met | E01 E06: CONTRIBUTING.md in this revision explicitly requires automated tests for every major new feature; other bug/behavior changes retain a documented testing-gap explanation. |
| `tests_are_added` · MUST | Evidence / Met | E06: Recent extension environment, parser fuzzing and checkpoint changes include ordinary regressions and actual binary integration tests. |
| `tests_documented_added` · SUGGESTED | Documented policy / Met | E01 E06: CONTRIBUTING.md in this revision explicitly requires automated tests for every major new feature; other bug/behavior changes retain a documented testing-gap explanation. |
| `warnings` · MUST | Evidence / Met | E05: CI runs Clippy with -D warnings across all targets and the fuzzing feature; latest lint passed. This does not claim every historical warning was a vulnerability. |
| `warnings_fixed` · MUST | Evidence / Met | E05: CI runs Clippy with -D warnings across all targets and the fuzzing feature; latest lint passed. This does not claim every historical warning was a vulnerability. |
| `warnings_strict` · SUGGESTED | Evidence / Met | E05: CI runs Clippy with -D warnings across all targets and the fuzzing feature; latest lint passed. This does not claim every historical warning was a vulnerability. |
| `know_secure_design` · MUST | Learning needed / Unmet | E10: The owner requested beginner guidance on 2026-11-02. Complete learning and practical review before reassessing personal understanding. Repository controls and AI assistance cannot supply this attestation. |
| `know_common_errors` · MUST | Learning needed / Unmet | E10: The owner requested beginner guidance on 2026-11-02. Work through common failure types and the corresponding Hibiscus tests before reassessment. No paid certificate is required. |
| `crypto_published` · MUST | Evidence / Met | E08: The native client uses reqwest/rustls/ring, SHA-256, base64 and OS randomness instead of implementing cryptographic primitives. These implementations are FLOSS. |
| `crypto_call` · SHOULD | Evidence / Met | E08: The native client uses reqwest/rustls/ring, SHA-256, base64 and OS randomness instead of implementing cryptographic primitives. These implementations are FLOSS. |
| `crypto_floss` · MUST | Evidence / Met | E08: The native client uses reqwest/rustls/ring, SHA-256, base64 and OS randomness instead of implementing cryptographic primitives. These implementations are FLOSS. |
| `crypto_keylength` · MUST | Evidence / Met | E08: Locked defaults offer AES-128/256 or ChaCha20, X25519/P-256/P-384 exchange, SHA-256/384 and vetted certificate verification with RSA minimum 2048 bits. OAuth state/verifier use 32 random bytes. This is a default-mechanism/key-size assessment, not FIPS certification or a claim about provider-issued credential entropy. |
| `crypto_working` · MUST | Evidence / Met | E08: Locked rustls defaults use TLS 1.2 ECDHE and TLS 1.3 ephemeral exchange with AEAD suites. No application override enables legacy TLS/ciphers or disables certificate validation. Plain loopback test/callback routes are identified separately. |
| `crypto_weaknesses` · SHOULD | Evidence / Met | E08: Locked rustls defaults use TLS 1.2 ECDHE and TLS 1.3 ephemeral exchange with AEAD suites. No application override enables legacy TLS/ciphers or disables certificate validation. Plain loopback test/callback routes are identified separately. |
| `crypto_pfs` · SHOULD | Evidence / Met | E08: Locked rustls defaults use TLS 1.2 ECDHE and TLS 1.3 ephemeral exchange with AEAD suites. No application override enables legacy TLS/ciphers or disables certificate validation. Plain loopback test/callback routes are identified separately. |
| `crypto_password_storage` · MUST | N/A / N/A | E08: Hibiscus is an outbound API/OAuth client, not a server authenticating external users with passwords. The official details exclude outbound credentials from this password-hashing criterion. Reversible private API/token storage is assessed separately. |
| `crypto_random` · MUST | Evidence / Met | E08: OAuth state and PKCE verifier use OsRng with a checked failure path; rustls/ring obtains TLS randomness from the OS. Ordinary session identifiers are not used as cryptographic secrets. |
| `delivery_mitm` · MUST | Evidence / Met | E04 E08: Installer release/checksum URLs use HTTPS; public downloads and exact signed provenance were verified. Default installation remains checksum-based, and optional signature verification is explicit. This is not a claim that every user-customized endpoint or external tool is constrained. |
| `delivery_unsigned` · MUST | Evidence / Met | E04 E08: Installer release/checksum URLs use HTTPS; public downloads and exact signed provenance were verified. Default installation remains checksum-based, and optional signature verification is explicit. This is not a claim that every user-customized endpoint or external tool is constrained. |
| `vulnerabilities_fixed_60_days` · MUST | Owner check / Unknown | E09: Public scans/advisories show no open finding, but they do not establish the maintainer's full knowledge of externally reported or privately handled issues. Confirm no overdue known medium/high or outstanding critical vulnerability before attesting. |
| `vulnerabilities_critical_fixed` · SHOULD | Owner check / Unknown | E09: Public scans/advisories show no open finding, but they do not establish the maintainer's full knowledge of externally reported or privately handled issues. Confirm no overdue known medium/high or outstanding critical vulnerability before attesting. |
| `no_leaked_credentials` · MUST | Evidence / Met | E07: Pinned full reachable-history and current-tree secret detection reported no findings, with native credential rules and real detector controls. Detection has documented limits; this is not a proof that every possible secret format is recognized. |
| `static_analysis` · MUST | Evidence / Met | E05 E07: CodeQL security-extended scans Rust, JS/TS, Python and Actions on protected-branch PRs/pushes and on schedule. The actual Rust extractor/query path has a harmless positive control. |
| `static_analysis_common_vulnerabilities` · SUGGESTED | Evidence / Met | E05 E07: CodeQL security-extended scans Rust, JS/TS, Python and Actions on protected-branch PRs/pushes and on schedule. The actual Rust extractor/query path has a harmless positive control. |
| `static_analysis_fixed` · MUST | Evidence / Met | E07: All four released-main reports have zero findings/errors/warnings and no open code-scanning alert; the five earlier alerts were fixed through source changes rather than suppressed. Reassess if new confirmed findings arrive. |
| `static_analysis_often` · SUGGESTED | Evidence / Met | E05 E07: CodeQL security-extended scans Rust, JS/TS, Python and Actions on protected-branch PRs/pushes and on schedule. The actual Rust extractor/query path has a harmless positive control. |
| `dynamic_analysis` · SUGGESTED | Evidence / Met | E06: Real libFuzzer/AddressSanitizer campaigns run on selected native parsers with debug assertions and bounded inputs. The six targets vary inputs; unsafe FFI/terminal code and all filesystem races are not claimed covered. |
| `dynamic_analysis_unsafe` · SUGGESTED | Evidence / Met | E06: Real libFuzzer/AddressSanitizer campaigns run on selected native parsers with debug assertions and bounded inputs. The six targets vary inputs; unsafe FFI/terminal code and all filesystem races are not claimed covered. |
| `dynamic_analysis_enable_assertions` · SUGGESTED | Evidence / Met | E06: Real libFuzzer/AddressSanitizer campaigns run on selected native parsers with debug assertions and bounded inputs. The six targets vary inputs; unsafe FFI/terminal code and all filesystem races are not claimed covered. |
| `dynamic_analysis_fixed` · MUST | Evidence / Met | E06: The two discovered provider-parser failures were reduced, corrected and released, with ordinary regression tests and later clean campaigns. This covers known campaign findings, not undiscovered defects. |

## Evidence references

### E01. Project and contribution

[Published README](https://github.com/Rekabytes-Enterprise/hibiscus/blob/f922054815e7f1e828b447a8bfaa586b5d7f5bb5/README.md), [MIT license](../../LICENSE), [Cargo metadata](../../Cargo.toml), [npm metadata](../../package.json) and [contribution guide](../../CONTRIBUTING.md). The contribution policy in this revision strengthens the major-feature test requirement and updates module ownership; use the approved public revision as form evidence.

### E02. User and interface documentation

[Native configuration and authentication](../native-runtime.md), [terminal behavior](../terminal.md), [extension protocol and environment](../extensions.md), [checkpoint recovery](../checkpoints.md), [installation](../installation.md), and the released binary's actual `hibiscus --help` output were inspected. The [index](../README.md), [development guide](../development.md) and [architecture map](../architecture.md) remove stale Pi/approval references and correct library startup ownership. Authentication does not implicitly serialize stored keys into sessions, but tool output and files can contain secrets; session privacy remains important.

### E03. Repository and response history

[Public repository](https://github.com/Rekabytes-Enterprise/hibiscus), [issues](https://github.com/Rekabytes-Enterprise/hibiscus/issues), [all issue states](https://github.com/Rekabytes-Enterprise/hibiscus/issues?q=is%3Aissue) and [pull requests](https://github.com/Rekabytes-Enterprise/hibiscus/pulls). The API reports a public, unarchived repository created on 2026-09-24 with issues enabled. All 19 issue descriptions and paginated comment/closing-event metadata were inspected. The chosen two-month window is 2026-09-02 through 2026-11-02 and includes every current issue. No bug label was present, so labels were not used to exclude reports. Conservatively classify [#23](https://github.com/Rekabytes-Enterprise/hibiscus/issues/23), the authentication-handoff behavior correction, as a bug report: its maintainer closure acknowledges one of one reports. The other 18 items are enhancement/maintenance requests. Maintainer closing events exist for #29, #30, #37, #43, #44, #45, #46, #47, #48 and #50: ten of 18, above half. #49 was completed by Mokara and is not counted in that conservative acknowledgement numerator. All seven open requests remain included. Legitimate requests mirrored by bots were not discarded. These are public-channel observations and a proposed classification for the maintainer to review, not a claim about unknown outside reports. No private report content was accessed.

### E04. Versions, release notes and delivery

[Published releases](https://github.com/Rekabytes-Enterprise/hibiscus/releases), [current changelog](../../CHANGELOG.md), [version helper](../../scripts/bump-version.sh), [installer](../../install.sh), [installation/provenance guide](../installation.md) and [release workflow](../../.github/workflows/release.yml). There are 29 published releases in the inspected history. All have a human-readable version summary in the current main changelog or a release body. Six oldest tags lack their own changelog section and release body; their summaries exist in current published main. No historical tag, asset or release metadata was rewritten for this assessment.

The [v0.3.16 release run](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/41378185495) passed nine jobs. Previous independent verification checked four archive receipts and exact signed source/workflow/tag/run identities, six anonymous public downloads and an isolated public installation. It executed only the macOS ARM64 binary locally, including default/declared-environment flows against a synthetic provider. Other architectures were built and inspected. Default installation is checksum-based over HTTPS; explicit provenance verification remains separate.

### E05. Build, CI and warnings

[Development commands](../development.md), [CI workflow](../../.github/workflows/ci.yml), [required checks](security-checks.md), [dependency audit](../../.github/workflows/dependency-security.yml) and the [release compilers](../../.github/workflows/release.yml). The latest released-main [CI run](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/41378118620) passed on attempt 2. Its initial Linux failure was an unchanged TUI test sending quit before settlement; all new extension tests passed. The failure and retry remain recorded. Strict Clippy, formatting and installer checks passed. Reusing this evidence does not claim a new full test run during this documentation assessment.

### E06. Tests and dynamic analysis

[Native integration tests](../../tests/native_runtime.rs), [extension environment regressions](../../tests/native_runtime/extension_environment.rs), [checkpoint tests](../../tests/native_runtime/checkpoints.rs), [fuzz adapters](../../fuzz/fuzz_targets), [fuzzing guide](fuzzing.md), [assertion-enabled build](../../.clusterfuzzlite/build.sh) and [workflow](../../.github/workflows/fuzzing.yml). Release-source validation passed 200 unit tests, 66 native integrations, the evaluation launcher with 20 Python checks, and curated fuzz replay. Actual hosted campaign [41376702988](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/41376702988) ran 2,437,155 inputs across six targets; [released-main fuzzing](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/41378118594) also passed. Earlier reduced SSE/Anthropic failures were fixed and released in v0.3.15. Parser campaigns do not establish whole-project branch coverage, all terminal/FFI coverage or all filesystem-race coverage.

The subsequent local [coverage measurement](coverage.md) uses a dated nightly and pinned tooling. On macOS ARM64 the final source-file report measured 84.96% lines and 66.31% branches after two new credential rejection integrations. All 200 unit tests, 68 native integrations, the evaluation launcher with 21 Python checks and curated replay passed. Inline unit-test code remains in the denominator; Python, shell, dependencies and other platforms are excluded. This local evidence is separate from the released v0.3.16 baseline and is not a production-only 80% branch-coverage claim.

### E07. Static analysis and secret detection

[CodeQL workflow](../../.github/workflows/codeql.yml), [real Rust detection probe](../../scripts/verify-codeql.py), [secret detector](../../scripts/secret-scanning.py), [native credential rules](../../.gitleaks.toml) and [detector controls](../../tests/secret_scanning.py). Released-main [CodeQL](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/41378118569) passed; all four processed reports have zero findings/errors/warnings and no code-scanning alert was open at assessment. [RustSec audit](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/41378118520) passed. Pinned Gitleaks history/current-tree scans and synthetic positive controls passed during release validation. An empty scanner result is not a universal proof about every credential or vulnerability.

### E08. Cryptographic implementation review

[Provider client](../../src/runtime/provider.rs), [OAuth/PKCE](../../src/runtime/oauth/protocol.rs), [configuration boundary](../../src/runtime/config.rs), [auth storage](../../src/runtime/auth.rs), [token storage](../../src/runtime/oauth/store.rs) and [locked dependency versions](../../Cargo.lock). The inspected Cargo feature graph selects reqwest 0.12.28 with rustls 0.23.46/ring 0.17.14. The locked [rustls ring provider](https://github.com/rustls/rustls/blob/v/0.23.46/rustls/src/crypto/ring/mod.rs), [version defaults](https://github.com/rustls/rustls/blob/v/0.23.46/rustls/src/versions.rs) and [exchange groups](https://github.com/rustls/rustls/blob/v/0.23.46/rustls/src/crypto/ring/kx.rs) were checked against locally available crate source. Defaults use TLS 1.2/1.3, ephemeral exchange and AEAD ciphers; no Hibiscus setting disables certificate validation or enables legacy suites. This was source/configuration inspection, not an exhaustive provider handshake audit or a FIPS claim.

OAuth state and verifier each use 32 bytes from checked OS randomness; PKCE uses SHA-256. Provider API keys and subscription tokens are issued elsewhere. Private outbound credential storage needs the original credential and is not inbound-password verification. Native provider URLs require HTTPS except explicit loopback endpoints. The local callback/test exception and the user's ordinary OS/extension access remain part of the documented trust model.

### E09. Vulnerability handling

[Security policy](../../SECURITY.md) and [private reporting form](https://github.com/Rekabytes-Enterprise/hibiscus/security/advisories/new). The reporting API read back enabled. Public repository advisories returned an empty list; open code-scanning alerts also returned an empty list. On 2026-11-02 the owner separately confirmed that no vulnerability reports, including private reports, have been received. That supports response-timing N/A; it does not establish historical public identifiers or the status of every internally discovered issue. Keep those three answers Unknown pending confirmation. Keep private report details out of this document. Do not turn the historical 14-day acknowledgement criterion into an invented SLA or weaken private reporting.

### E10. Maintainer knowledge

The [official detailed criteria](https://www.bestpractices.dev/en/criteria/0?details=true&rationale=true#0.know_secure_design) recognize the sole developer as the primary developer. They require knowledge, not a second person, a certificate, or an AI attestation. The owner requested beginner guidance on 2026-11-02, so both mandatory knowledge answers are Unmet. The [learning guide](security-learning.md) pairs the free OpenSSF course with concrete Hibiscus code and exercises. Reassess only after learning and practice; generating this guide does not complete the requirements.

## Maintainer review notes

Use these examples to check understanding, not as an automatic claim of qualification:

- Keep mechanisms small. The extension environment policy has one command factory rather than separate policies at each process call.
- Choose safe defaults. Auth files are private, unexpected configuration fails, and extensions inherit only named variables beyond the baseline.
- Check authority at the boundary. Validate the full manifest and tool arguments, and never execute an incomplete provider stream. Direct tool execution remains intentional; there is no sandbox promise.
- Use open, reviewed designs. Delegate TLS and hashing to maintained libraries; protect changeable credentials rather than hiding protocol details.
- Separate powerful operations. Release builders, the signer and the publisher have distinct permissions and independent receipts.
- Minimize privileges. Most CI jobs have read-only tokens; extension processes receive a limited environment.
- Minimize shared mechanisms. Private homes, workspace-scoped sessions and request leases reduce unintended sharing. Concurrent non-cooperating file edits still have limits.
- Make the safe path understandable. Mask key entry, give recoverable errors, and make restore conflicts visible instead of overwriting work silently.
- Keep exposed interfaces limited. The presentation socket is private and local; extension discovery does not run arbitrary workspace programs.
- Accept known-valid inputs. Validate schemas, URL schemes, callback state, path components, framing and size bounds before acting.

For common failures, be able to explain the mitigation and its limit: untrusted text passed to shells, path traversal/symlink races, credential leakage, malicious instructions in files/model output, OAuth state mismatch, malformed network frames, and memory/FFI mistakes. Input validation, cancellation and checkpoints each solve specific problems; none makes arbitrary shell or extension execution safe in every workspace.

## Submission handoff

1. Complete learning and reassess both Unmet mandatory knowledge answers. Resolve the three remaining Unknown answers and review every proposed Met/N/A with the actual maintainer. Record response-rate denominators and the time window; never infer private response history from empty public alerts.
2. Keep `test_most` honestly Unmet. The local source branch measurement is below 80% and includes inline test code; test counts, line coverage and targeted fuzzing cannot establish the broader criterion.
3. Use public source URLs from the approved documentation revision. A working-tree draft alone is not published evidence.
4. With explicit submission authorization, find or create the BadgeApp entry for `https://github.com/Rekabytes-Enterprise/hibiscus`. Use the same URL as the project homepage, MIT as the license, and Rust as the primary implementation language. Do not create a second entry for another repository spelling.
5. Enter reviewed answers and evidence URLs. Inspect the actual resulting BadgeApp status. Add a Best Practices README badge only after its real project ID and status are verified. No account login, submission or README badge change has been performed here.

Publishing this document or a Hibiscus software release does not submit the BadgeApp application, confirm the owner-only answers or award a badge. Branch approval requirements remain separate from the passing-criteria checklist.
