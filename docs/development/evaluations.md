# Evaluation runner boundary

## Grounding and candidate designs

The installed binary accepts one-shot prompts, reads explicit configuration/instructions/extensions from `HIBISCUS_HOME`, and persists canonical format-v1 sessions. The session contains correlated tool outcomes and nullable usage. Existing native runtime tests exercise execution and presentation, but do not compare experiment configurations against independent coding-task checks.

One design adds `hibiscus eval` and calls private engine/session APIs from a new Rust runtime module. It could share internal types, but would ship benchmark policy and widen the product CLI. It would also create another execution path to maintain beside normal prompts.

The chosen design is a development-only standard-library Python module driving one frozen compiled binary. It uses the public one-shot/config/session boundary and the authoritative native extension registry. It keeps experiment inputs, split discipline and scoring outside installed runtime code. Python is a development test dependency, not an agent runtime dependency. There is no alternate provider SDK or copied agent/tool loop.

## Caller and type sketch

```text
plan = load_plan(path, explicit_live_flag) -> Plan
suite = load_suite(plan.suite) -> Suite
report = evaluate(plan_path, binary, split, explicit_live_flag)
write_report(report, fresh_output_path)

Plan = common provider/model/limits/repetitions + tuple[Variant]
Variant = ID + frozen instructions + tuple[extension source images] + content digest
Suite = versioned ID + tuple[Task] + content digest
Task = unique ID + split + prompt + frozen starter files + independent grader
       + changed/preserved file contract + deterministic offline batches + digest
Image = relative path + bytes + ordinary permission bits
```

`tools/evaluation/spec.py` owns input parsing, bounds, path rules and immutable snapshots. Unknown fields, duplicate JSON keys, unsupported versions, overlapping scoring paths, missing splits, invalid API-key environments and symlinks fail at this boundary.

`execution.py` owns fresh native homes/workspaces, constrained inherited environment, subprocess groups, output/time bounds, canonical session evidence and independent grading. It never scores assistant text. It saves actual tool error flags and numeric shell status, without parsing human-formatted error text. Provider usage remains nullable. Only supported session format/model/thinking/workspace and completed correlated calls are accepted.

`fixture.py` owns the deterministic local Responses adapter. It drives actual native read/edit/bash calls and supports negative-test replies. It is never used as evidence of coding ability.

`runner.py` owns experiment comparability, ordering, repetitions, native preflight and summaries. Every variant is loaded through native `list_extensions`; actual advertised definitions determine the shared tool-catalog digest. Invalid extensions stop performance runs without becoming solved/unsolved task scores. Extension source digests are derived from the same frozen files copied into every run. One binary copy protects an experiment from concurrent rebuilds. One plan owns controls, preventing per-variant model/budget drift. Failed runs remain in their paired denominator. A new optional initial variant has the same contract, not a separate runner.

`__main__.py` is the sole command boundary. It requires explicit `--live` plus a live plan and writes a private exclusive report. It neither publishes nor overwrites experiments. It prints error categories rather than potentially sensitive subprocess output. The generated report contains versioned nested result objects, keeping structural state, execution errors and objective outcomes distinct.

## Limits and checks

Grading code is withheld until process settlement. This prevents accidental access through the task workspace; it is not an adversarial security boundary. Shell/extension execution retains the user's OS permissions, and managed runtime tools may create new tools during a task. Reports compare initial advertised catalogs and record actual tool names/outcomes, not an immutable capability sandbox. Bundled code digests cannot freeze arbitrary system dependencies. Native response/step and subprocess wall limits are not total token/spend budgets. Session and suite versions must change or receive an explicit reader update when their schemas change.

The small cases, grading contracts, fake-success rejection, failed tools/providers, nullable usage, split isolation, native schema validation, private/non-overwriting reports and process descendant termination are tested by `tests/evaluation/test_evaluation.py`. `tests/evaluation.rs` invokes them through `cargo test` using the exact freshly built executable. CI supplies Python 3.12 on Linux/macOS. There is no separate hand-maintained native tool list or provider wire parser in the scorer.
