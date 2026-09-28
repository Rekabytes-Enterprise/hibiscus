# `/loop` — unattended Pi workflow (experimental)

## Choose a mode

| Command | Completion decision | Testing workflow |
| --- | --- | --- |
| `/loop <goal>` | Pi reports that the requested work and tests are complete | Built-in skill-style instructions; Pi chooses suitable tools, test names and evidence formats |
| `/loop --strict <goal>` | Pi candidate signal **plus** Hibiscus's report checks and independent Pi SDK reviewer | Playwright JSON and named criteria/artifacts as described below |

These are Hibiscus commands, not a custom Pi fork or separately installed skill. Pi owns models, credentials, tools and sessions. Both modes require a full-screen interactive terminal, bypass Hibiscus's own dangerous-bash approval gate while active, and restore it when the controller exits. Esc stops the active run; it cannot undo tools already executed. Piped/line mode does not enable this bypass.

## Guided mode: the default

The controller repeats Pi prompts only after correlated acceptance and `agent_settled`. The original user request is included on each continuation. Instructions tell Pi to inspect the target project, provision project-local Playwright/browser dependencies on the user's host when necessary, start the app, perform meaningful browser UAT for web flows/buttons, inspect screenshots/DOM/console/server logs, reproduce defects, fix and retest. Video is used only where supported. Smoke tests must not substitute for requested UAT.

Pi calls the reporting-only `loop_status` tool with `candidate_complete` when it judges the requested work and tests complete, or `blocked` with a genuine blocker. No visible sentinel is needed. The completion message explicitly says **Pi reports completion; not independently verified**. The default does not require exact test titles, prescribed attachment names, report-file validation or an additional reviewer SDK call. The raspberry footer shows the loop phase and optional model-reported checklist, not a machine-verified UAT counter.

Repeated prose without tool progress or an explicit completion/blocker signal gets a targeted recovery instruction; four consecutive no-tool turns stop as stalled rather than spending indefinitely on repeated summaries. A progress/checklist percentage remains model-reported. This mode is flexible, not a guarantee that Pi actually tested everything it should.

## Failure annotations (both modes)

Before the first browser run, guided mode instructs Pi to configure `screenshot: 'only-on-failure'`, `trace: 'retain-on-failure'` and optional `video: 'retain-on-failure'`; strict mode uses `screenshot: 'on'` to keep passing screenshots too. Pi must preserve the project's other Playwright settings. This is model guidance, not a Hibiscus-managed Playwright installer/config editor. Shared prompt guidance asks Pi to report the page, failed control and locator, reproduction, expected/actual behavior and assertion error. Preserve an original failure screenshot; create a separate annotated PNG highlighting the implicated control, clearly labelled as a test annotation. Missing controls and setup/network failures must not be misrepresented as visible button defects; a browser that never opened cannot provide a page screenshot. Retain traces and a concise Markdown failure summary; save a separate passing screenshot after repair. This guidance also appears in continuation prompts, so follow-up turns retain it. It does not install a separate skill, draw annotations inside Hibiscus, or prove that a model actually performed them. Screenshot/annotation limitations must be reported honestly, and no artificial bugs should be introduced unless requested.

## Strict mode: opt-in evidence checks

`/loop --strict <goal>` retains the original evidence-checking workflow:

- Pi proposes named acceptance criteria; a separate reporting-only call through the selected Pi SDK reviews their coverage of the immutable request, allows missing scope to be added, then locks the names.
- Each step needs a passing, non-skipped Playwright test with the same title, screenshot and browser-console attachments. Reports must show no unexpected/skipped/flaky tests. This checks names and reported outcomes, not the semantic quality of assertions.
- Submit a saved report through `loop_status` action `submit_evidence`, with `projectDir`, `reportPath` and optionally `supportFiles`. The internal extension/client bridge returns immediate acceptance counts and missing checks without a human dialog. Wrapped commands, redirected output and truncated stdout do not prevent saved-file submission.
- Reports are regular JSON files of at most 4 MiB inside the project. Artifact paths can be project-relative, report-directory-relative or absolute inside the project. Inline base64 and file attachments, including duplicate names, are supported. Inline bodies have bounded MIME/signature/UTF-8 checks; they are not a full image decoder. A later Pi image read must match a recorded file or identical decoded image bytes.
- Accepted packages are retained with explicit stale reasons. Bounded content fingerprints track common JS/TS/CSS/HTML/Vue/Svelte files and package/TS configuration manifests; generated output, hidden directories, symlinks and dependencies are excluded. Limits: 10,000 directory entries, 2 MiB per source file, 32 MiB total source bytes. Source comparison is not a universal dependency monitor: external services, databases and excluded files are not proven unchanged. Report/source freshness permits one second of filesystem timestamp precision.
- Review receives counts, per-criterion metadata, bounded console and test-source excerpts, and up to eight explicitly supplied supporting files. Historical failure reports are summarized without copying their image data. Credentials/hidden/out-of-project supporting files are rejected; common-secret sanitization is best-effort. Screenshot pixels are not sent to the reviewer.
- The reviewer returns one native `submit_verdict` tool-call argument object, with `strict: prefer` schema support where available and local validation regardless. It executes no tools. One format-only correction uses the same evidence and deadline; provider errors/cancellation/truncation are not parsed as JSON prose. Incompatible SDKs block with update guidance. Extra reviewer calls may not appear in the main Pi session usage snapshot.
- Completion requires the candidate signal, reviewed complete checklist, accepted current report/image-read evidence and a positive reviewer verdict. Missing criteria are consolidated into targeted continuations. Repeated no-progress completion replies may be folded in Hibiscus's view, never deleted from Pi's session, before stalled-loop termination.

Strict mode can reject a valid workflow that does not fit its evidence contract. It is not proof of universal correctness and should not be the default for every project.

## Shared limits and verification

Billing/quota and other provider errors end the loop. Temporary rate-limit failures can be retried only if no tool started and the connection remains usable. The controller counts failed calls plus observed Pi retry events before scheduling another attempt; Pi may already have performed extra internal retries, so ten observed attempts is **not a hard provider-call cap**. Weekly/quota classification remains advisory; uncertain side effects are never automatically replayed.

The scoped `loop-active` marker and RAII guard restore ordinary approvals on completion, Esc, reported blockers and errors. Synthetic Pi/SDK tests exercise both modes and verify guided mode can finish without any reviewer SDK or strict artifacts. These tests do not install a real browser or contact a provider. Real-platform UAT, semantic quality, video capability, cost accounting and failure cleanup still need matching-build acceptance tests. User screenshots are scoped observations, not universal verification.
