# OpenSSF Scorecard

The prepared [Scorecard workflow](../.github/workflows/scorecard.yml) assesses repository and software supply-chain security practices. Its aggregate score is out of 10. It is not a certification or a guarantee that application code has no vulnerabilities.

The workflow runs after pushes to the default branch, `main`, and weekly. It uses immutable action commit pins, does not persist checkout credentials, and grants publishing OIDC permission only to the analysis job. It publishes measured results to OpenSSF and retains the JSON report as a short-lived GitHub Actions artifact. It does not execute repository build scripts or change repository settings.

Source changes currently developed on `dev` are not a separate published score. OpenSSF's supported publishing flow assesses the default branch. A merge to `dev` alone will not activate this workflow. See the [official example](https://github.com/ossf/scorecard/blob/main/.github/workflows/scorecard-analysis.yml) and [publishing requirements](https://github.com/ossf/scorecard-action#publishing-results).

## Activate the badge

1. After publication is explicitly authorized, land the workflow on `main` and wait for its run.
2. Inspect the `openssf-scorecard-results` artifact and the [public API result](https://api.scorecard.dev/projects/github.com/Rekabytes-Enterprise/hibiscus). Verify its date and repository commit identify the intended scan. A successful action alone does not confirm publication; the pinned action can report publication failures as warnings.
3. Confirm the [badge endpoint](https://api.scorecard.dev/projects/github.com/Rekabytes-Enterprise/hibiscus/badge) returns the measured score.
4. Move the prepared badge line out of its comment and into the README's existing badge paragraph, preserving the original four badges.

The workflow activates when it reaches `main`. No published Hibiscus rating has been verified yet, so the README badge remains commented out until the first measured result is available.

## Interpret the result

Review the individual checks and their evidence before changing settings. Some checks need additional API visibility, such as classic branch protection; missing permissions are not proof a protection is absent. Keep unknown or unavailable findings distinct from a confirmed failure. The default workflow token is used; no personal token or repository setting change is required by this preparation.

Scorecard complements dependency vulnerability checks, secret scanning and code review. Its score should be presented as a repository-practices assessment, not a claim that local shell execution or extensions are sandboxed. [Project documentation](https://securityscorecards.dev/).
