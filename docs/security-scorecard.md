# OpenSSF Scorecard

The [Scorecard workflow](../.github/workflows/scorecard.yml) assesses repository and software supply-chain security practices. Its aggregate score is out of 10. It is not a certification or a guarantee that application code has no vulnerabilities.

The workflow runs after pushes to the default branch, `main`, and weekly. It uses immutable action commit pins, does not persist checkout credentials, and grants publishing OIDC permission only to the analysis job. It publishes measured results to OpenSSF and retains the JSON report as a short-lived GitHub Actions artifact. It does not execute repository build scripts or change repository settings.

Source changes currently developed on `dev` are not a separate published score. OpenSSF's supported publishing flow assesses the default branch. A merge to `dev` alone will not activate this workflow. See the [official example](https://github.com/ossf/scorecard/blob/main/.github/workflows/scorecard-analysis.yml) and [publishing requirements](https://github.com/ossf/scorecard-action#publishing-results).

## Published results

The first published scan on 2026-11-01 measured **3.4/10** at commit `27fb82b3863930919a7c6bc2e78d781e3b7041a5`. Its [workflow run](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/41286151028), retained JSON artifact and [public API result](https://api.scorecard.dev/projects/github.com/Rekabytes-Enterprise/hibiscus) were checked for matching date and commit. The [live badge](https://api.scorecard.dev/projects/github.com/Rekabytes-Enterprise/hibiscus/badge) returned the same score before it was enabled alongside the README's original four badges.

The README badge tracks the latest published result. The initial score above is a dated observation and may differ after later scans. Review the [current report](https://scorecard.dev/viewer/?uri=github.com/Rekabytes-Enterprise/hibiscus) for check details.

For future publication checks, compare the `openssf-scorecard-results` artifact with the public API result and verify the scan date, repository commit and badge. A successful action alone does not confirm publication; the pinned action can report publication failures as warnings.

## Interpret the result

Review the individual checks and their evidence before changing settings. Some checks need additional API visibility, such as classic branch protection; missing permissions are not proof a protection is absent. Keep unknown or unavailable findings distinct from a confirmed failure. The default workflow token is used; no personal token or repository setting change is required for this workflow.

Scorecard complements dependency vulnerability checks, secret scanning and code review. Its score should be presented as a repository-practices assessment, not a claim that local shell execution or extensions are sandboxed. [Project documentation](https://securityscorecards.dev/).
