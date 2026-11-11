# Changelog

All notable changes to Hibiscus are documented here.

## [Unreleased]

## [0.3.38] - 2026-11-12

### Added

- Sign in with Reka Bytes to use Rooxcode models (#185). The desktop app opens on a sign-in screen with "Sign in with Reka Bytes", or "Continue with my own provider" for people who only use their own API key or Codex. Sign-in happens in the browser and uses the same account as the Rooxcode website. In the terminal, the model list still loads without an account; the first prompt to a Rooxcode model asks you to sign in with `/login` → Browser Login → Reka Bytes and puts the prompt back in the input. One sign-in covers the desktop app and the terminal on the same computer. Sign out from Settings → Providers in the desktop app, or `/logout` in the terminal.

## [0.3.37] - 2026-11-12

The 0.3.36 desktop build never finished, so this is the first desktop release with the 0.3.36 changes below.

### Fixed

- Release: the desktop Linux build skips the GitHub runner's Azure Ubuntu mirror and downloads its libraries from archive.ubuntu.com. The Azure mirror stalled the 0.3.36 build for over 30 minutes.

## [0.3.36] - 2026-11-11

### Added

- Desktop app: Hibi can test web pages in a built-in browser. A new `browser` tool drives a headless Chrome through [agent-browser](https://github.com/vercel-labs/agent-browser), which ships inside the app, and a browser pane shows the page live. The pane opens on the first browser call of a run, or from the globe button in the title bar. A cursor moves to each element Hibi clicks or types into, and you can click, scroll and type in the page, use the address bar, or have the pane find a running dev server on the usual local ports. The browser uses Google Chrome when it's installed; otherwise the app downloads Chrome for Testing the first time Hibi needs it.
- Desktop app: Hibi sees browser screenshots as images, not just the page's text, so it can check layout, overlap, colours and blank areas. `screenshot` captures the visible page, one element, or the whole page with `--full`. The app scales each image down to JPEG before sending it, and the chat shows a thumbnail you can click to see full size. Models that don't accept images get a note instead.
- Desktop app: comment on elements in the browser pane. The comment button outlines the element under the pointer; click one, type what should change, and add more if you like. "Add to message" puts the comments in the message box, each with the element's CSS path, text, page URL and a cropped image (up to 4), so Hibi changes that exact element. The pins stay on the page until Hibi's run ends.
- Desktop app: the app can start the project's dev server. When none is running, the browser pane offers the usual command (a `package.json` dev or start script, Django, Rails, or a static `index.html`), starts it in the background and opens its URL. Hibi gets a `dev_server` tool that starts, checks, reads and stops the same server, so it can test a change and read build errors without a terminal. The pane shows the server's output with a Stop button. One server runs per folder, and it stops when you open another folder, remove the folder from the sidebar, or quit.
- Desktop app: the browser pane shows a red badge with a count when the page logs a console error, throws an uncaught exception, or gets a 4xx or 5xx response. Click it to list them, or clear them. Hibi's `browser` tool now tells it how to read the console, exceptions, failed requests and one request's detail, and shows only the latest 100 entries, each cut to 500 characters.

### Changed

- Desktop app: opening a saved session loads its latest 60 messages instead of the whole conversation, and older ones as you scroll up. Tool output over 2 KB loads in full when you open or copy its row, and screenshots load as they scroll into view. The session panel still lists every file the session edited. Hibi still sees the whole conversation.

## [0.3.35] - 2026-11-11

### Changed

- Desktop app: clicking a session that hasn't been opened yet highlights it and changes the title at once. If its conversation takes longer than 150 ms to load, grey placeholders fill the chat and the details panel, and the message box is disabled until the session is ready.

## [0.3.34] - 2026-11-10

### Added

- Desktop app: in-app updates. The app checks for a newer release when it starts and every 6 hours, shows a banner, and on request downloads the update, installs it and restarts. Updates are signed, and each release now has 14 files: the macOS `.app.tar.gz`, signatures for it and the AppImage, and `latest.json` are new. Versions before this one have to be updated by hand once.
- Desktop app: rename and delete saved sessions from the menu next to each session in the sidebar. Delete asks first. The name is saved in the session file.
- Desktop app: an "Add folder" button in the sidebar and a "Remove from sidebar" item on each folder. The new-session picker can also remove folders from its list. The sidebar keeps up to 8 recent folders.
- Desktop app: Settings has an About page with the installed version, whether a newer one is out, and the release notes for this version and earlier ones.

### Changed

- Desktop app: Settings pages use the full window width and keep their content centred.
- `scripts/bump-version.sh` moves the `## [Unreleased]` entries in `CHANGELOG.md` under the new version with today's date, and refuses to bump while Unreleased is empty.

## [0.3.33] - 2026-11-10

### Changed

- Desktop app: switching to another session in the same folder no longer restarts the engine or downloads the model lists again. The conversation shows as soon as its messages load, and the session details, git changes and session list then load in parallel. In our measurements a switch went from about 600 ms (up to 1.9 s on a cold start) to about 35 ms.
- The changes pane compares the branch with its nearest base branch, such as `dev`, instead of always using the repository's default branch, so unreleased work on the base no longer shows as the branch's own. The base label is now a menu for picking another branch, and the app remembers the pick per folder.
- `docs/desktop/release.md` explains how to open the unsigned desktop app on macOS 15 with System Settings' "Open Anyway".
- Dependency updates: `sha2` 0.11, plus GitHub Actions updates including `actions/setup-node` 7.1.0.

### Fixed

- Desktop app: edit diffs no longer squeeze the code into a third of the width when they start partway through a file. Long lines wrap between words, durations under 0.1 s are hidden, back-to-back goal updates show as one row, and paths inside the session folder show as relative paths in command rows.

## [0.3.32] - 2026-11-09

### Fixed

- Desktop release files are signed and attached to the release. The v0.3.31 desktop build could not be signed (its provenance lacked a field GitHub requires), so v0.3.31 was not published; this release ships everything listed under 0.3.31.

## [0.3.31] - 2026-11-08

### Added

- Desktop app for macOS (Apple Silicon) and Linux (x86_64), in `desktop/` (Tauri v2 + Svelte). It has a sidebar of sessions by folder, chat with Markdown, highlighted code blocks and copy buttons, grouped tool calls with edit diffs, a session details panel, a branch diff view, image attachments, and a Settings page for providers with Codex browser sign-in and sign-out.
- Release tags now also build `hibiscus-desktop-aarch64-apple-darwin.dmg` and `hibiscus-desktop-x86_64.AppImage`, with their own `SHA256SUMS-desktop` and signed provenance, in the same draft release. The builds are unsigned; see `docs/desktop/release.md` for how to open them.
- Privacy and Terms of Use pages in the desktop app's Settings.
- A "Don't let providers train on my prompts" setting. While it is on, OpenRouter requests ask for providers that don't store or train on prompts. It is stored in `privacy.json` in the Hibiscus home, applies to the terminal app too, and is off by default.
- `hibiscus::desktop` library API used by the app: start engines per project folder, send commands, read events, save or remove credentials, sign in to Codex through the browser, list sessions, and read or set the training preference.

### Changed

- Internal restructuring with no behaviour change: the engine (`src/engine/`) and the terminal's side of the connection (`src/client/`) are now separate, the login screens moved out of the engine, and the largest files were split.

## [0.3.30] - 2026-11-08

### Added

- Codex models now come from the signed-in plan's own list at startup and when opening `/models`, in the TUI and CLI. Each model shows the context window, image support and thinking levels Codex reports, and `/thinking` offers exactly those levels (up to `xhigh` and `max` where listed). Hidden models are left out. `gpt-6.1-sol` remains the fallback before the first fetch or if it fails, and listing `openai-codex` models in `config.json` turns discovery off.
- The list request carries the current official Codex release, looked up from npm once per run, because Codex lists a model only to clients at least as new as its minimum version. Without npm, Hibiscus uses the last release seen, then a pinned one. Other providers' thinking levels are unchanged.

## [0.3.29] - 2026-11-08

### Added

- Built-in OpenRouter (`OPENROUTER_API_KEY`) and OpenCode Go (`OPENCODE_API_KEY`) API-key providers. With a key, Hibiscus loads each provider's own model list at startup and when opening `/models`, using the context window, image and reasoning support it reports and skipping models without tool support. No `max_tokens` is sent, so each provider applies its own output limit. `/login` → API Key shows where to create the key, then opens the model picker. Listing `models` for either provider in `config.json` turns discovery off for it.
- `/models` shows one tab per provider you can use, plus All. Tab or ←/→ switches provider, search covers the open tab, and each row shows the context window, image and reasoning support its provider reports. Providers without a key or sign-in are hidden; `/login` still lists them. The picker opens on the current model's provider and shows up to 10 rows.
- CLI `/models` prints a provider summary when there are more than 50 models. `/models <provider> [search]` lists one provider and `/models <provider>/<model>` selects directly; the TUI opens the matching tab and search.

### Fixed

- A model discovery error is shown once rather than again every time `/models` opens.
## [0.3.28] - 2026-11-05

### Added

- Use each Rooxcode model's advertised thinking levels in `/thinking`, CLI and TUI, and send the selected `reasoning_effort` through generation, tool continuation, compaction and resume. Default leaves the field unset; explicit `none` is offered only when advertised. Unsupported choices reset to Default with a notice after confirmed catalog or model changes.

## [0.3.27] - 2026-11-05

### Fixed

- Start automatically after updating an older installation whose config contains `codexContextWindow`. Ignore the retired override and use native defaults for missing settings while preserving the original file, valid provider configuration and stored credentials.
- Cover legacy numeric/null settings in both terminal interfaces, native provider authentication, anonymous Rooxcode access and installer preservation. Invalid native configuration still fails without resetting user settings.

## [0.3.26] - 2026-11-04

### Changed

- Merge the six reviewed Dependabot updates: Base64 0.23.1, Rand 0.10.3, libc 0.2.191, serde_json 1.0.154, unicode-width 0.2.3 and libfuzzer-sys 0.4.13.
- Migrate Codex OAuth and Rooxcode session randomness to Rand's system RNG API while retaining fallible error handling.
- Update pinned Python setup and cache restore/save actions to their reviewed releases.

## [0.3.25] - 2026-11-04

### Fixed

- Chat-completions replies that stop at the output limit (`finish_reason: length`) are continued automatically, up to three times per response, and the new text joins the same reply. After the cap the text is kept with a visible notice instead of an error. Rooxcode still omits `max_tokens`.
- Continuation usage totals include every request while the context gauge uses the final request. Summaries still cut off after the continuation cap preserve the original conversation.
- CLI diff regression waits for input readiness before quitting, avoiding a premature-exit test race.
- A tool call cut off at the output limit is never executed. It is discarded and the model is asked once to re-issue it in full; a second cut-off reports a clear error. Content-filter stops now say the provider blocked the response. A missing finish reason remains an error. Anthropic and Responses are unchanged.

## [0.3.23] - 2026-11-04

### Fixed

- Rooxcode requests now measure the full encoded payload and compact before advertised byte, message or context budgets are reached. Large histories use bounded summary chunks and retain recent complete tool groups when they fit.
- Failed or cancelled compaction preserves conversation history. Structured request-size errors provide sanitized recovery guidance. Codex and other providers retain their existing compaction policy.

## [0.3.22] - 2026-11-03

### Added

- Rooxcode v0.1.9 waiting support with animated `Rooxcode is busy. Waiting…` status below the TUI composer, CLI status, SSE keepalives and distinct queue, daily-quota and work-budget errors, including failures delivered after HTTP200 streaming headers.
- Queue regressions for cancellation, tool continuation, manual recovery without replay and provider credential isolation.

### Fixed

- Empty assistant messages no longer create orphan `hibi` labels or split consecutive tool activity. Tool summaries use green/red success/failure badges with muted details.

## [0.3.21] - 2026-11-03

### Added

- Built-in Rooxcode model discovery and anonymous Chat Completions access with per-conversation client metadata, streaming, tool continuation and cancellation.
- Fixed service diagnostics for catalog failures, unavailable models, metadata rejection, rate limits and disabled/upstream inference, with no automatic generation retries or provider fallback.

## [0.3.20] - 2026-11-03

### Added

- Optional offline Draft 7 output schemas for extension tool data, including agent authoring and versioned update checks.
- Regressions for provider recovery, rejected-output removal, schema compilation, omitted data, logical errors, slash commands and retained version contracts.

### Fixed

- Treat invalid extension success data as a correlated tool error while preserving the actual zero exit status and existing timing flow. Rejected data and success text stay out of session results and subsequent model requests.

## [0.3.19] - 2026-11-02

### Fixed

- Open API-key credential files once with symlink refusal and nonblocking flags, validate the opened file, and cap reads at 128 KiB even if the file grows after its metadata check.
- Keep credential values and private paths out of malformed-store errors. Preserve rejected files and reject oversized saves before atomic replacement.

### Added

- Regressions for exact-limit stores, file growth, path replacement, rejected-save preservation, FIFO rejection and redacted CLI/TUI errors. Existing credential JSON and normal login behavior are preserved.

## [0.3.18] - 2026-11-02

### Added

- A pinned local Rust source/branch coverage runner, annotated HTML and JSON reports, failure checks and a read-only CI workflow with checksum-verified tooling. Reports include native subprocess execution and explicit measurement limits.
- Regression tests for rejected API-key stores and blank login input, plus beginner secure-development exercises tied to Hibiscus code and tests.

### Changed

- Route evaluation profiling output through the test harness so instrumentation does not alter graded workspaces; production environment policies and graders are unchanged.
- Update the Best Practices draft from maintainer answers. No received vulnerability reports makes response timing N/A; the two security-knowledge criteria remain Unmet pending learning, and three owner checks remain Unknown. No badge is submitted or awarded.


## [0.3.17] - 2026-11-02

### Added

- A 67-criterion OpenSSF Best Practices passing assessment with source and run evidence, solo-maintainer guidance, honest coverage limits, and explicit owner-only confirmations. This is an application draft, not an awarded badge.

### Changed

- Require automated tests for major new features in the contribution policy.
- Update native startup/module ownership, dev/main CI guidance and documentation navigation; remove stale Pi and approval wording from developer instructions.
- Clarify that saved tool output can contain secrets even though authentication handlers keep stored credentials and provider error bodies out of session messages.

## [0.3.16] - 2026-11-02

### Changed

- Start extension tools, slash commands and validation checks with a minimal environment. Additional parent variables must be named in the manifest's `inheritEnv` list; missing declared variables fail before execution. Native shell tools keep their normal environment.
- Keep environment declarations with managed extension versions, expose names in the extension catalog, and document migration for extensions that previously relied on implicit credentials or other variables.

## [0.3.15] - 2026-11-02

### Added

- Six bounded native parser fuzz targets, curated malformed inputs and a digest-pinned ClusterFuzzLite workflow for pull requests and scheduled campaigns. Normal builds have no fuzzing dependency.

### Fixed

- Preserve SSE frame order with mixed LF/CRLF delimiters regardless of network chunk boundaries, and avoid repeatedly shifting the remaining stream buffer.
- Reject malformed Anthropic content blocks and mismatched text deltas with normal errors instead of panicking.

### Changed

- Share the application entry point through a library while keeping native modules private; expose fuzz adapters only through an explicit development feature.
- Share one workspace lockfile between native and fuzz builds so dependency auditing and updates cover both packages.

## [0.3.14] - 2026-11-02

### Changed

- Replace illustrated README terminal previews with actual TUI, CLI and Mermaid captures, including real file edits, colored diffs and test results from local demo sessions. Preserve the existing README layout and document the capture method.
- Scan full Git history with a checksum-pinned secret detector and native credential-store rules in required CI, with actual detection and failure-handling controls. This changes repository checks only; the native runtime is unchanged.

## [0.3.10] - 2026-11-01

### Changed

- Analyze Rust, JavaScript/TypeScript, Python and GitHub Actions with pinned CodeQL on PRs, protected-branch pushes and a weekly schedule; verify actual Rust extraction and finding detection with a disposable fixture.
- Require passing CI, RustSec and CodeQL checks on both development and release branches, while preserving PR, thread-resolution, deletion and force-push protections without a hidden bypass.
- Reject unmerged release source before checkout or compilation; compiler helpers accept only frozen commit SHAs and never run on PR events.
- Publish a security reporting policy with private GitHub reports, latest-release fixes and accurate local execution and checkpoint limits.

## [0.3.9] - 2026-11-01

### Changed

- Add reviewed Cargo/Actions dependency update PRs, daily pinned RustSec audits, expiring advisory exceptions and real vulnerable-input detection checks.
- Use immutable CI/release action pins, a fixed Rust toolchain and digest-pinned Linux build images.
- Preserve annotated release-tag refs during checkout so exact-tag verification succeeds; branch-only builds still freeze the event commit.
- Keep release verification and compilation read-only; verify all four compiler archive hashes before a separate publisher uploads assets. Recovery uses cache transport without granting write access to build jobs.

## [0.3.7] - 2026-11-01

### Added

- Development-only coding evaluations with versioned learning/selection/held-out tasks, paired baseline/candidate runs, native extension preflight, independent grading and private JSON reports. Offline fixtures are the default; live providers require explicit configuration.
- Native workspace checkpoints for write/edit, with request/session correlation, recorded diffs, conflict-checked selective restore, interrupted-operation recovery and explicit retention tools.
- Extension candidate updates with retained version history, parent and changed-file records, configured regression commands/protocol checks, native history/diff/rollback tools and atomic activation.

### Changed

- Restore the full README presentation with native runtime documentation and prepare a default-branch OpenSSF Scorecard workflow. The score badge remains hidden until a published scan is verified.
- Keep the raspberry HIBI banner pinned above TUI conversations, with live thinking/model/workspace/session metadata and compact layouts for small terminals and multiline drafts. Unicode labels respect terminal column widths.
- Remove thinking confirmations from TUI chat and the obsolete Pi attribution from CLI confirmations; guard installer wording against the old Pi-ready message.
- Extension registry snapshots now retain implementation files with their metadata. Existing source folders are preserved; rejected updates and interrupted candidates keep the last active version usable.

## [0.3.6] - 2026-10-31

### Added

- Local executable extensions with versioned JSON, validated model tools and custom slash commands in both views.
- An empty user extensions directory and native `create_extension`, `list_extensions` and `reload_extensions` agent tools. Creation registers new tools and commands in the same session; TUI completion includes custom commands.

### Changed

- Separate CLI and fullscreen TUI renderers, shared terminal/display services and view-specific integration tests.
- Installation/startup creates the user extensions folder while preserving existing manifests and source files.

## [0.3.5] - 2026-10-31

### Added

- Native Rust provider adapters, tools, authentication, sessions, queues and compaction, replacing the Pi SDK and subprocess runtime.
- Native Codex browser OAuth with PKCE/state validation, masked redirect input, private token storage and serialized refresh.
- `/cli` and `/tui` switch views without reconnecting or resending prompts. TUI remains the default; CLI supports multiline editing and bracketed paste.
- `/output`, `/output list` and numbered saved-result inspection, with no provider call or tool rerun.
- Compact CLI tool rows with green/red status backgrounds, measured timing, exit codes, wrapped commands, numbered edit previews and colored Git diffs.

### Changed

- Restore the raspberry HIBI welcome design and hide login hints when a provider has credentials. Signing out clears an unavailable model from the header.
- Installer and updater manage Hibiscus only. The agent requires neither Pi nor Node; the optional npm installer still requires Node/npm and separate publication.
- Authentication failures remain inside Hibiscus. Browser login supports native Codex; API-key login uses configured providers.

### Removed

- Remove `/approval`, approval settings, grants and tool confirmation dialogs. Validated native tools execute directly, including shell commands in piped mode. Tools are not sandboxed.

### Migration

- Configure native providers/models in `~/.hibiscus/config.json`. Pi credentials and legacy sessions are not imported automatically. Back up existing data and read [native configuration and migration](docs/native-runtime.md).
- Provider protocols include OpenAI Responses, compatible Chat Completions and Anthropic Messages. Model IDs and capabilities are explicit configuration.

## [0.3.4] - 2026-10-19

### Added

- `/login` now offers Browser Login (Codex or Claude) and API Key, with Pi SDK provider discovery, masked credential entry and same-chat reconnect.
- Publishable `@rekabytes/hibiscus` npm installer for npx, reusing the bundled checksum-verified shell installer and pinning downloads to the npm package version. Requires npm publication and matching GitHub release assets.

### Changed

- Keep slash-command submissions and model-selection confirmations out of full-screen chat; show the active model in the header and picker.

## [0.3.3] - 2026-10-17

### Added

- Searchable `/models` picker with a labeled input, divider, case-insensitive provider/model filtering, paste support, and preserved current-model selection.
- `/approval` Enable/Disable picker and persistent preference, restored across launches.
- Hibiscus home directory (`~/.hibiscus`, configurable with `HIBISCUS_HOME`) for settings and update cache, with legacy approval-setting migration.
- Configurable 1M context default for explicitly supported Codex models, applied through Pi's model registry and active-model selection.
- Separate always-on writing guidance that preserves user-requested tone, code, quotations, and uncertainty.

### Changed

- `hibiscus update` now attempts both Hibiscus and Pi updates and reports partial failures. The in-chat update prompt remains Hibiscus-only.

### Removed

- `/loop`, including guided/strict controllers, reviewer integration, evidence bridge, and loop-specific tools and UI.

## [0.3.2] - 2026-09-28

### Added

- Full-screen `@path` completion searches local files and directories at the composer cursor, with quoted paths for spaces and continued directory completion. Enter or Tab inserts a path without sending the prompt or reading file contents. Search runs off the UI thread through bounded `fd` results where available, with a direct-directory fallback. Pi receives ordinary prompt text and may choose to read the referenced file; this is not Pi's CLI `@file` attachment expansion.

### Verification limits

- Synthetic file-picker/PTY tests check inserted prompt text and that file contents are not silently sent. Real-provider, physical-terminal and macOS acceptance remain pending; a dev push is not a tagged release.

## [0.3.1] - 2026-09-28

### Added

- Render completed assistant Mermaid code blocks as Unicode diagrams in full-screen chat after Pi settles, using the selected Pi installation's optional diagram renderer. Keep Markdown visible while streaming and as fallback when a diagram cannot be rendered or fit the transcript.

### Fixed

- Lay out Markdown tables as aligned, width-aware columns with cell wrapping and per-row separators; accept pipe-delimited model output without a separator line. Use rounded Unicode corners and raspberry-colored borders in color terminals.

### Verification limits

- Diagram replacement and table layout are covered by synthetic tests; live-provider, physical-terminal and macOS presentation remain to be checked. A `dev` push does not publish release artifacts.

## [0.3.0] - 2026-09-28

### Added

- Experimental interactive `/loop <goal>` drives Pi through repeated settled runs, temporarily bypasses Hibiscus's shell approval gate, and instructs Pi to provision browser UAT tooling on the user's host. Esc and reported blockers restore ordinary approvals. Both modes use an internal `loop_status` tool signal rather than displaying a magic completion marker and keep the between-turn footer raspberry until the loop exits. Opt-in strict mode freezes the initial named checklist and requires passing Playwright JSON tests and screenshot/console attachments per step, plus a read of an attached image. A separate reporting-only Pi SDK reviewer checks the immutable original request and evidence each turn, permits missing steps to be added before locking scope, and fails closed on unavailable verdicts. Consolidate missing UAT criteria in targeted continuation prompts; show checklist and verified UAT progress separately, fold repeated completion-only chat rows locally, and report stalled loops after targeted recovery. This still does not prove that generated tests meaningfully cover every behavior.

### Changed

- Loop instructions now request Playwright failure capture before the first browser run (`only-on-failure` in guided mode, `on` in strict mode, with traces and optional failure video), plus original and separately annotated failure screenshots, failed-control/locator identification, expected-versus-actual results, a Markdown failure summary, and post-fix screenshots. Model-driven guidance applies to both modes and continuations; unavailable annotations must be reported, not fabricated.
- Default `/loop <goal>` now uses flexible Pi-guided test-and-fix instructions with explicitly model-reported completion. Existing Playwright evidence contracts and independent reviewer are opt-in through `/loop --strict <goal>`; guided mode needs no reviewer SDK or prescribed artifacts.
- Accept inline and duplicate Playwright attachments; retain accepted evidence with explicit stale reasons based on bounded source comparison. Return report validation results directly to Pi and provide bounded test-source, console/log and historical failure excerpts to the reviewer.
- Reviewer verdicts use Pi-native schema-defined `submit_verdict` tool arguments instead of parsing JSON prose, with one bounded format correction and explicit handling of provider errors, cancellation, truncation and incompatible SDKs. No custom Pi installation or global configuration change.
- Accept `/loop` UAT evidence from saved Playwright JSON reports through `loop_status submit_evidence`; resolve relative artifact paths, check report freshness, preserve results during known read-only log checks, and send validated evidence metadata/missing checks to the independent reviewer.
- Add a separate Hibiscus operational-guidance prompt section alongside identity guidance: inspect safely, ask about material ambiguities, use conservative defaults for minor reversible details, and report verification honestly. Keep Pi's original prompt and tools; model compliance is not guaranteed.
- Dangerous-command Always Allow can authorize displayed per-command prefix patterns for the current directory/session (for example, `git push *`), including changed comments and arguments. Compound commands are reviewed per component where the syntax is simple; complex shell syntax stays exact. Parallel pending decisions reuse a matching grant, and Deny rejects queued approvals. Broad patterns can include force flags or other targets—review the scope before approving.

## [0.2.5] - 2026-09-27

### Added

- Full-screen transcript drag selection and clipboard copying, including Ctrl+C to copy an active selection. On an empty selection, two Ctrl+C presses within two seconds quit instead of one.
- Bracketed multiline terminal paste inserts text into the composer without submitting the first line.

### Changed

- Improve layouts below 60 terminal columns with smaller margins, a compact header, and complete activity/Goal fields instead of clipped usage text. Wider layouts retain their previous spacing.
- Dangerous-command approval waits for an explicit choice instead of auto-denying after 60 seconds. Deny remains the default, Esc cancels, and unrelated explicit dialog timeouts still apply.

### Verification limits

- Clipboard and mobile terminal presentation still require physical-terminal retesting. The approval gate remains a best-effort guard, not a sandbox.

## [0.2.4] - 2026-09-26

### Added

- Pi-confirmed thinking level in the header and cumulative context/input/output/cache tokens plus approximate cost in the single footer row beside activity and optional Goal. Usage refreshes only at confirmed idle boundaries; unsupported Pi stats and null context do not invent values.

### Changed

- Abbreviate displayed token counts and manual compaction notices with decimal K/M/B/T units (e.g. 200K/1M); shorten cache reads to `Cache ~value`, omit cache writes, and cap displayed USD cost at two decimals (with `<$0.01` for small nonzero costs), without changing Pi's accounting.

## [0.2.3] - 2026-09-26

### Added

- `/thinking [level]` to choose only Pi-reported reasoning levels for the selected model, and `/compact [instructions]` to let Pi summarize older context with live status, Esc cancellation and explicit response correlation.

## [0.2.2] - 2026-09-25

### Fixed

- Preserve Hibiscus identity guidance when Pi's `before_agent_start` event has no structured prompt sections. Fall back to Pi's existing rendered prompt or append option instead of throwing an extension warning.

## [0.2.1] - 2026-09-25

### Fixed

- Route Pi stderr warnings to a bounded, sanitized diagnostic row below the full-screen composer/status instead of allowing raw output to overwrite it. Preserve drafts, cursor layout, dialog focus, reconnect routing and ordinary stderr in line/piped modes; provider settings are unchanged.

## [0.2.0] - 2026-09-25

### Added

- Standard MIT license with Rekabytes Enterprise attribution and a fork/feature-branch contribution guide targeting `dev`.
- Repeatable formatter, allocation and session-list benchmarks, plus a large-history paste regression and performance review.
- Correlate delayed steering acknowledgements after final settlement without hanging or replaying, while still waiting for nonempty Pi queues to drain.
- Full-screen `/sessions` and `/continue` now scan Pi session files off the UI thread, showing a cancellable loading notice and buffering typing for the composer; line-mode/startup listing is unchanged.
- Optional isolated Linux input-wake profiler and shared input/RPC activity notification, reducing quiet-run key-to-output latency while retaining the timed animation fallback.
- Optional Linux-only synthetic memory profiler with per-process RSS/high-water counters, isolated mock Pi/clipboard fixtures, and logical live/peak heap-byte benchmarks. No runtime behavior changes are part of this profiling addition.

### Changed

- Guide identity answers to introduce Hibiscus as the terminal assistant powered by Pi, without replacing Pi's system prompt or misidentifying the model/provider.
- Full-screen chat, composer, status and docked panels now grow with the terminal instead of remaining capped at 100 columns; drafts and cursor layout reflow on resize.
- Validate but do not materialize incoming user image-echo base64 when only text and attachment counts are needed; leave Pi's saved messages and other RPC event payloads intact.
- Validate but do not materialize the unused `partialResult` JSON subtree of `tool_execution_update`; preserve all other event fields, errors, approvals, queue ordering and settlement.
- Share staged image values between the composer and recovery slot, avoiding a second base64 allocation on queued rejection while keeping Pi's JSONL wire format and explicit `/restore` behavior.
- Bound RPC wire-record/backlog memory and record counts with explicit overload failure, nonblocking/deadline-controlled writes, partial-send cancellation and reviewable uncertain queued drafts. Add synthetic duplex/overload regressions and opt-in queue metrics.
- Cache transcript layout independently of terminal row painting, reuse completed message blocks, and coalesce scrolled row counting.
- Batch available printable input and read terminal bytes in chunks without changing control-key or UTF-8 handling.
- Avoid per-character Markdown string allocations, repeated front-shifting during wrapping, and repeated failed link searches.
- Cache unchanged session metadata with bounded memory, file-identity checks, invalidation and periodic expiry.
- Borrow images during RPC serialization, move response data/base64 buffers, and queue compact raw incoming records for consumer-side parsing. Buffered output errors do not implicitly retry commands.

## [0.1.7] - 2026-09-25

### Added

- Editable full-screen input during Pi runs: Enter requests steering, and Ctrl+Q (WSL) or Alt+Enter queues a follow-up, including image attachments.
- A pending-message panel that distinguishes queue acceptance from delivery; user rows are inserted on Pi's user-message event.
- An explicit `goal` checklist tool with model-reported completion counts and a compact progress indicator beside the live status.
- Shared docked modal components for approvals, selections, confirmations, text input, and supported auth prompts.

### Changed

- Composer editing supports arrow navigation, Home/End, and insertion/deletion at the cursor across multiline drafts.
- The terminal UI uses an aubergine background, a latest-file exploration indicator, checked Read/Bash counts, and per-edit diff previews.
- Full-screen rendering caches rows and batches streamed text updates, with synchronized output where supported and a steady-cursor request. `HIBISCUS_NO_SYNC_UPDATE=1` disables synchronized markers.
- Project state and memory records now separate implementation, automated evidence, user reports, and unresolved risks instead of accumulating historical test claims.

### Verification limits

- Automated checks use unit tests, mocked Pi/SDK subprocesses, and PTYs. Real terminal flicker, provider queue ordering, auth flows, and macOS behavior still require targeted retesting.
- Goal percentages reflect checklist updates supplied by the model, not independently verified work. The approval gate remains a best-effort guard, not a sandbox.

## [0.1.6] - 2026-09-25

### Added

- A best-effort approval gate for recognized dangerous agent shell commands, with Deny, Allow once, and session-scoped Always Allow for the exact command and working directory. Routine `read`, `edit`, `write`, and `bash` remain automatic.
- `/restore` for reviewing a failed prompt and its images, plus `/reconnect` for reconnecting Pi without automatically replaying commands.
- In-chat `/logout` for choosing and removing a stored Pi provider credential with explicit confirmation.
- Raspberry flower logo, illustrated terminal preview, release-download badge, and a streamlined README.

### Fixed

- Provider failures such as rate limits and quota exhaustion no longer close interactive chat; successful Pi retries no longer inherit earlier attempt failures. One-shot and piped errors retain nonzero exit statuses.
- `/new` opens a blank full-screen chat after Pi confirms session creation, without deleting earlier saved sessions.
- Restored session history distinguishes user and agent messages; user rows have a subtle tint and agent replies display a `hibi` heading.

### Notes

- The approval gate is not a sandbox: scripts, indirect tool effects, and ordinary file edits can still change data. Deny is the default; no interactive approval means no permission for a flagged tool call.
- Logout removes a stored credential only. It does not revoke provider tokens or unset environment/configuration credentials.

## [0.1.5] - 2026-09-25

### Added

- Multiline composer input with wrapping, scrolling, and Ctrl+Enter support.
- Clipboard image attachments with platform-specific paste shortcuts.
- Image-only and text-plus-image prompts through Pi RPC.

### Changed

- Improved full-screen composer layout and attachment controls.
- Added bounded, cancellable clipboard helper processes.
- Added image placeholders to history instead of exposing image data.

## [0.1.4] - 2026-09-25

### Added

- `/models` support for models across all configured providers.
- Provider-aware model selection and switching without reauthentication.

## [0.1.3] - 2026-09-25

### Changed

- Codex browser sign-in now opens the validated OAuth URL with the platform browser on macOS.
- Improved OAuth completion handling and fallback behavior.

## [0.1.2] - 2026-09-25

### Changed

- Incremental patch release following the initial release and installer work.

## [0.1.1] - 2026-09-24

### Added

- Checksum-verified prebuilt installation for Linux and macOS.
- Automatic Pi installation when Pi is not already available.
- `hibiscus update` with atomic, checksum-verified updates.
- Background update checks for full-screen prebuilt installations.

### Changed

- Restricted Hibiscus-launched Pi instances to built-in tools and disabled extensions.
- Improved session handling, authentication handoffs, and terminal test reliability.

## [0.1.0] - 2026-09-24

### Added

- Initial Rust CLI for chatting with Pi through JSONL RPC.
- Full-screen terminal chat with Markdown rendering, progress indicators, tool activity, and scrolling.
- Persistent Pi sessions, session resume, `/new`, `/sessions`, `/models`, and authentication flows.
- Line-mode fallback for non-interactive terminals.

[Unreleased]: https://github.com/Rekabytes-Enterprise/hibiscus/compare/v0.3.6...HEAD
[0.3.6]: https://github.com/Rekabytes-Enterprise/hibiscus/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.5
[0.3.4]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.4
[0.3.3]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.3
[0.3.2]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.2
[0.3.1]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.1
[0.3.0]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.0
[0.2.5]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.5
[0.2.4]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.4
[0.2.3]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.3
[0.2.2]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.2
[0.2.1]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.1
[0.2.0]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.0
[0.1.7]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.7
[0.1.6]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.6
[0.1.5]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.5
[0.1.4]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.4
[0.1.3]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.3
[0.1.2]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.2
[0.1.1]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.1
[0.1.0]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.0
