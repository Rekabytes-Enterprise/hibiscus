# Current state

## v0.3.28 release preparation

The owner requested a new release after local Rooxcode thinking implementation. Prepare the patch through dev and main in Rekabytes-Enterprise/hibiscus, then verify and publish the signed GitHub Release. This request supersedes the preceding local-only scope. No registry publication or gateway deployment is included.

The patch validates advertised per-model reasoning efforts, exposes Default plus supported levels in CLI/TUI, and sends reasoning_effort only for explicit Rooxcode choices. Default omits the field and does not imply disabled upstream reasoning; advertised none remains distinct. Selection survives tool/length continuation, compaction and resume. Successful catalog/model changes reset unsupported choices with a durable reset and short notice; failed discovery preserves the choice and blocks unavailable inference. Existing providers and anonymous credential isolation remain unchanged.

Local implementation verification passed 227 unit tests, 107 native integration fixtures and the evaluation test, strict all-target Clippy with fuzzing, formatting and diff checks. Real CLI/TUI PTYs and captured request bodies cover selectors, headers, resets, lifecycle, cancellation and other-provider behavior. The v0.3.28 version-sensitive Rust suite, strict Clippy, parser corpus, installer/version/npm checks and formatting/diff checks passed. Hosted checks and signed archive verification remain pending.

The adjacent gateway contract/source was read without changes. Production must advertise reasoning:true with per-model reasoning_efforts and accept matching request values. No live upstream inference or deployment verification is claimed.

Previous v0.3.27 remains published latest until this release is verified and published. It fixed startup for retired codexContextWindow configs while preserving native settings, original files and credentials. PR114/115/116 passed; main9cc88ab and dev428201a had matching trees and correct ancestry. Its four signed archives, public downloads, installer and isolated old-updater flow were verified. The old v0.3.25 intermediate tag/draft remains immutable and unpublished; its changes shipped in v0.3.26. Security-score/age eligibility and unrelated tasks remain deferred.
