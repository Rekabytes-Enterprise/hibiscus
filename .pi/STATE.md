# Current state

## v0.3.30 release preparation

The owner asked to ship Codex plan model discovery for the TUI and CLI as v0.3.30 through a PR to main, separately from the desktop app, which goes to dev only and is not production-ready. The release branch carries only catalog.rs, config.rs and thinking.rs changes plus docs, changelog and the version bump.

Codex models come from GET /models on the Codex endpoint with the current official Codex release as client_version (npm lookup once per run, last-seen and pinned fallbacks). Local verification: 235 unit tests, 110 native fixtures, fuzz_boundaries, strict all-target Clippy with fuzzing, fmt, workflow/coverage/provenance/installer/bump/npm checks, and a live CLI `/models openai-codex` listing 8 plan models. Hosted CI, the tagged draft build and signed archive verification remain pending.

After release, merge main back into dev so the desktop PR to dev carries only desktop work.

v0.3.29 remains published latest until v0.3.30 is verified and published. Old v0.3.25 and v0.3.12 drafts remain unpublished. Security-score/age eligibility and unrelated tasks remain deferred.
