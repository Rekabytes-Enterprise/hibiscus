# Coding-task evaluations

The evaluation runner measures edited files, independent checks and recorded native tool outcomes. It compares instruction or extension implementation variants through the same Hibiscus binary. It is a development tool requiring Python 3.9 or newer on Linux/macOS, with no third-party Python packages. It adds no dependency or command to the shipped agent.

## Run offline

From the repository root:

```sh
cargo build --locked
python3 -m tools.evaluation --plan evals/offline.json \
  --binary target/debug/hibiscus --output /tmp/hibiscus-selection.json
```

Choose a new output filename for each experiment. Existing reports are never overwritten. The report is created with mode `0600`. A structurally comparable experiment exits zero even when some tasks fail; inspect `summary[].solved` and individual results. Invalid input or an invalid/incomparable native preflight exits nonzero. Per-run provider, process and grader failures remain in the report and denominator.

The included synthetic provider sends a fixed read/edit/bash sequence through the actual agent. Both variants should solve both repetitions of the selection task. This proves the runner can execute and score the flow. It does not establish model quality or an improvement from the candidate instructions. Native regression tests additionally reject a valid assistant response that only claims success without changing files.

## Keep the splits separate

`evals/suite.json` is versioned as `coding-v1` and references three distinct cases:

| Split | Case | Use |
| --- | --- | --- |
| `learning` | Greeting whitespace and blank-name fallback | Inspect examples and develop a change |
| `selection` | Whitespace-normalized slugs | Choose among candidate configurations |
| `held_out` | Repeated-category totals | Evaluate the chosen configuration after selection |

Selection is the default. Run the other splits separately:

```sh
python3 -m tools.evaluation --plan evals/offline.json \
  --binary target/debug/hibiscus --split learning --output /tmp/hibiscus-learning.json
python3 -m tools.evaluation --plan evals/offline.json \
  --binary target/debug/hibiscus --split held_out --output /tmp/hibiscus-held-out.json
```

Every run gets a fresh workspace containing only that case's starter files and visible tests. The private application home receives only the chosen variant's instructions and copied extension sources. Other cases, answers and objective graders are not placed in the running workspace. The grader is materialized outside it only after the agent and its subprocess group stop. The runner does not feed learning results into selection or search for a winning candidate. Once held-out results influence a change, treat that case as development evidence and add new held-out cases before making a fresh evaluation claim.

These small public cases are an initial harness, not a representative benchmark or secret test set. Separate paths prevent accidental prompt contamination, not intentional access by code running with the same user's permissions.

## Baseline and candidate controls

`evals/offline.json` owns common model/API settings, native `maxSteps`, wall/check timeouts and repetition count. Version 1 starts fresh sessions with thinking `off`; it exposes no sampling/seed override that the native runtime cannot enforce. Add an optional `initial` variant using the same variant shape when comparing a historical configuration. The first variant must remain `baseline`.

Each variant supplies an instruction file and zero or more extension source directories, relative to the plan directory using normal paths without `..`. Put extension snapshots beneath the plan directory, for example `variants/candidate_extensions/review`. Each directory needs its original `extension.json` and implementation files. Copy source only, not `.versions` stores or credential files. Prefer `{extensionDir}` in manifest arguments and bundle implementation files rather than referencing mutable programs elsewhere.

Inputs are read into bounded immutable snapshots before execution. Reports identify the suite, task, instructions, extension source contents/modes, plan and frozen binary by SHA-256. The runner records Python/shell binary identities and OS/architecture. It does not derive identities from modification times or unverified revision labels. Managed extension activation IDs can differ across fresh homes; the content digest is the comparison identity.

A local scripted preflight invokes native `list_extensions` without running extension programs. Native schema/registration errors are reported separately from task performance. It captures the provider's advertised native/extension tool definitions and requires identical catalogs across variants. Changing instructions or tool implementation is allowed. Changing tool names, descriptions or schemas makes this comparison incomparable. Fix the experimental controls before interpreting it as an improvement. The same compiled binary, model settings, budgets, cases and repetition count apply to every variant. Paired order reverses on alternate repetitions to reduce fixed-order bias.

## Objective scoring and results

Before the experiment, each unchanged starter must fail its independent checks. Every run then records:

- Task/split/repetition and input/configuration digests.
- Agent exit status, execution error and measured duration.
- Session structural validity, correlated tool outcomes and shell exit codes when recorded.
- Independent grader outcome, required changed files and byte/mode preservation of declared user files and visible tests.
- Provider-reported input/output/cache tokens, retaining `null` for unknown values. Cost is always `null`, because the native session does not provide verified pricing.

`solved` requires a completed valid session, successful agent process, passing independent checks, required edits, preserved files and at least one successful recorded tool. Assistant prose is never the scoring source. Tool errors are counted separately; an agent may recover from a failed tool and still solve the task. The report keeps all planned runs in each variant's solve-rate denominator, including failures, and compares candidate wins/losses/ties against matching baseline task/repetition pairs. Token totals stay unknown if any included usage is unknown. Time is measured, not deterministic fixture data.

Reports omit prompts, raw tool text, stdout/stderr, credentials and session transcripts. Temporary workspaces, sessions and process logs are removed after the experiment. A caller can inspect aggregate/error-code evidence without publishing private conversations. Review instruction and extension sources before running them; there is no filesystem/network sandbox. A changed dependency outside a bundled extension, OS scheduling or a remote provider update can still change results.

## Explicit real-provider evaluation

Copy `evals/live.example.json` to `evals/live.local.json`, then replace its placeholder HTTPS endpoint and model with your provider's native API configuration. Supported API adapters are `responses`, `chat_completions` and `anthropic`. The file and normal `evals/reports/` output folder are ignored by Git. Set the declared API-key environment variable in your shell using your usual credential method. No stored account, OAuth file or other application home is imported.

```sh
python3 -m tools.evaluation --plan evals/live.local.json \
  --binary target/debug/hibiscus --live --output /tmp/hibiscus-live-selection.json
```

The runner refuses a live plan without `--live`, and refuses `--live` for an offline plan. Native structural preflight still uses the local synthetic provider; task execution uses the explicitly configured endpoint. The key is supplied to the agent environment only, not the grader or report. Native tools/extensions inherit that environment as in normal Hibiscus use. Evaluations can make billable requests. `maxSteps`, per-response `maxTokens` and wall time are limits, not a total token or spend cap. Provider-internal retries, model updates and sampling may vary; repetitions expose variation rather than guaranteeing reproducibility.

The included live plan is a configuration example. Offline test success does not establish real-account acceptance, provider availability or comparative coding performance.

## Add a case

Create a task directory containing `task.json`, a bounded `workspace/` tree and `verify.py`. Register it once in `evals/suite.json` with a unique ID and explicit split. Use ordinary relative paths, at most 128 files/8 MiB per inventory and no symlinks or special files. `changed` and `preserved` must reference distinct starter files. Keep graders independent of assistant claims and visible test modifications; use deterministic Python assertions that exit nonzero for incorrect behavior. They receive the workspace path as their sole argument.

Declare a deterministic `offline` sequence of native tool-call batches to repair that case for harness testing. Its answers are used only by the local fixture, never as real-provider prompts. Prove that the starter fails and the repaired implementation passes. Preserve existing file/tool/stream regression tests; a task-level score does not replace them.
