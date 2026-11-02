# Current state

## Issue33 extension output schemas, prepared for v0.3.20

The owner authorized implementing issue33, PRs through dev/main, proper issue/task closure and a new patch release. Branch codex/issue-33-extension-output-schemas starts from released main494b35a. Only a prior local STATE update existed and has been preserved outside this snapshot. Older rejected records branches remain excluded.

Tools now accept optional outputSchema declarations compiled with the existing offline Draft7/linear-regex validator. Input/output validators stay paired in each registry snapshot. Successful replies validate data before producing a tool result; omitted data is null, explicit null schemas are invalid. Schema failures retain extension ID and exitCode0 but replace rejected data/success text with a fixed tool error. Logical errors, no-schema extensions and slash commands retain existing behavior. Engine call IDs, timing and error recovery are unchanged. Agent authoring advertises the field; managed update checks and rollback use the same boundary.

Four unit regressions and two native integration regressions added. Baseline failed to load the new declarations. Complete local cargo test passes210unit+71native+1evaluationlauncher21Python. Valid/invalid outputs exercised across all three provider protocols, plus agent creation, candidate refusal, snapshot retention and rollback. Installer, version bump, offline npm,44workflow,3coverage-policy and17provenance-policy tests pass. Strict all-target/feature Clippy and curated fuzz replay pass after updating the existing fuzz adapter for paired validators and adding an output-schema seed.

No PR, tag or release for this patch has been published yet. Current public release remainsv0.3.19. Required hosted checks, exact-head review, signed draft verification, release-binary feature smoke and public installer verification remain before completion. Score chasing is deferred per the owner; no badge submission or registry publication is authorized.
