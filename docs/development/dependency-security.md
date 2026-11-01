# Dependency updates and audits

Dependabot checks Cargo and GitHub Actions weekly. Version PRs target `dev`, compatible updates are grouped, and each ecosystem permits three open version PRs. Major updates remain separate. Updates keep ordinary CI and human review; nothing enables automatic merge, bumps Hibiscus's own version or publishes a package. The npm installer currently has no package dependencies, so it needs no npm update entry.

GitHub security fixes use the default branch, `main`, even when version updates target `dev`. Separate security-only entries disable main's version PRs while grouping security fixes there. GitHub applies its own security-PR limit, independently of the version-PR limits. Both main and dev PRs run normal CI and the dependency audit. After accepting a security fix on main, merge it back into dev to prevent regression in the next promotion. Required checks and branch-review enforcement remain a separate repository-rules task.

The read-only dependency-security workflow runs on both branches, their PRs, a daily schedule and manual dispatch. It installs cargo-audit 0.22.2 with its lockfile and fetches the official RustSec database for the real Cargo.lock. It never uses `--stale` or `--no-fetch` for production scans. Tool errors, missing database evidence, inconsistent reports or unexpected platform/severity/ignore filters fail the check. Warnings need explicit advisory review as well. Reports are retained for five days; failed scans never print a successful result or reuse an older clean report. A scheduled run uses the default branch.

To reproduce with the pinned tool:

```sh
cargo install cargo-audit --locked --version 0.22.2
python3 scripts/audit-dependencies.py --db /tmp/hibiscus-rustsec --report /tmp/hibiscus-audit.json
HIBISCUS_AUDIT_TEST_DB=/tmp/hibiscus-rustsec python3 tests/dependency_audit.py
```

The last command also audits a temporary lockfile containing known-vulnerable time 0.1.43 and requires detection of RUSTSEC-2020-0071. It never downloads, builds or executes that crate. Ordinary unit tests use a synthetic auditor and exercise failed fetches, invalid reports, hidden filters, tool-version mismatch and expired exceptions without network access.

The exception policy is `.github/security/advisory-exceptions.json`, initially empty. A proposed exception must name one exact RUSTSEC advisory ID, a nonempty rationale, a responsible owner and an ISO date in `expires`. Expiry must be later than the current UTC date. Duplicate IDs, wildcard IDs, missing/unknown fields and expired entries fail before auditing. Review and remove exceptions as fixes become available. The wrapper checks the auditor's resulting ignore list, so a separate Cargo audit configuration cannot silently add exclusions. An exception is an accepted temporary risk, not a fixed vulnerability.

Dependabot alerts and automatic security updates are repository settings, verified separately from this configuration. Version-update activation requires this file on the default branch. Advisory audits are detection against published knowledge; passing one is not a claim that all dependencies are vulnerability-free.
