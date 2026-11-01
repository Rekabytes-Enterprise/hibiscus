# Security checks and protected branches

Pull requests into `dev` and `main` run the same checks. Contributor changes target
`dev`; releases promote a tested `dev` revision into `main`. Dependency security
fixes may target the default branch directly and receive the same checks.

## Required checks

The branch rules require these GitHub Actions checks, with the source branch up
to date with its target:

- `Format, Clippy, and installer`
- `Test (ubuntu-latest)`
- `Test (macos-15)`
- `RustSec dependency audit`
- `CodeQL (rust)`
- `CodeQL (javascript-typescript)`
- `CodeQL (python)`
- `CodeQL (actions)`

The rules apply to `main` and `dev`. Both require a pull request and resolved
review threads, and prevent branch deletion and force pushes. There are no
configured bypass actors. Administrators use the same passing checks.

## Review and recovery

The repository currently has one maintainer, so the rules do not require an
approval the author cannot supply independently. Review feedback must still be
resolved before merging. AI review does not replace an independent human review.
When another trusted maintainer can review changes, require at least one
independent approval, dismiss stale approvals after new commits, and require
approval of the latest push. Verify contributor and release PRs can still merge
before making that review requirement mandatory.

If a required job fails, fix it on the PR branch and rerun the checks. If the base
branch changes, update the PR branch and test again. Do not bypass a failed check
or add a status with a matching name from another integration. A GitHub Actions
outage should leave the PR waiting rather than appear successful.

If a required check is renamed or permanently unavailable, a repository
administrator can edit the affected ruleset in GitHub's repository settings.
First verify the replacement check on a PR, record the reason and old/new rule
settings, replace only the affected requirement, and read back the active rules.
Keep PR, deletion, force-push and thread protections intact. Do not add an
unrecorded admin bypass or disable all rules as a routine release step.

## CodeQL coverage and limits

The pinned [CodeQL workflow](../../.github/workflows/codeql.yml) analyzes Rust,
JavaScript/TypeScript, Python and GitHub Actions. It runs on all PRs to `dev` and
`main`, pushes to those branches, weekly and on manual dispatch. It does not use
path filters that leave documentation PRs waiting for a required check.

Rust uses the supported `none` build mode and the fixed Rust toolchain. CodeQL
does not run a Cargo build script as part of a manual build step. Generated code
that is unavailable without a build may be outside this analysis. Shell scripts
are not a supported CodeQL language; installer syntax and behavior checks remain
in ordinary CI. Formatting, Clippy, tests and RustSec audits continue separately.

The workflow defaults to read-only repository access. Only analysis jobs receive
`security-events: write` for CodeQL's SARIF upload. Fork PRs use `pull_request`,
not `pull_request_target`; checkouts do not retain credentials, no repository
secrets are supplied, and release credentials are never shared. GitHub supplies
the supported code-scanning upload path for fork PRs. Actual fork approval and
runner behavior still follow the repository's Actions policy.

The action uses its linked CodeQL bundle, preserving the tool/library combination
associated with its immutable action revision. Repository SARIF files are retained
for five days, with distinct language categories. Review completed analyses and
alerts in GitHub code scanning. A successful extraction/upload means the scan
completed; it is not a claim that the source is free of vulnerabilities.

`scripts/verify-codeql.py` also creates a temporary dependency-free Rust crate and
runs a harmless empty-branch query with the actual Rust extractor and bundled
library. It requires exactly one SARIF finding at the expected source line and
excludes a nonempty control branch. The fixture is never executed, committed as
Rust production code, or uploaded as a repository alert. Tool/extraction/query
failures and missing findings fail the job. This checks scanner wiring; it does
not measure the detection rate of every security query.

## Initial workflow finding review

The initial extended Actions scan reported four findings in the existing release
compiler helper: two poisonable build steps, cache transport, and a checkout
reported as possibly receiving PR source. These appeared in retained SARIF even
though GitHub's PR summary excluded them as unchanged lines. Always inspect both
the retained report and GitHub's analysis result.

The compiler helper now rejects PR events and accepts only a full frozen commit
SHA before checkout. Its caller obtains that SHA from the exact checked source;
tagged publication also checks the tag and Cargo version. Manual release builds
are a maintainer operation: select only source you trust. They execute Cargo and
its dependencies. Read-only repository permission does not make builds a sandbox.

Cache transport is an explicit recovery mode for the release archives, separate
from reusable dependency/build caches. It uses exact run/target/digest keys and
checks independent compiler receipts before publication. The compiler helper has
no shared Cargo cache and receives no release token. Workflow analysis can still
flag manual-dispatch source as potentially cache-poisoning because the source
verification crosses reusable-job outputs. Keep that trust assumption visible;
do not suppress a query or dismiss an alert solely to obtain a clean report.
