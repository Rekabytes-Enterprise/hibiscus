# Secret scanning

GitHub repository secret scanning and push protection are enabled for Hibiscus.
Repository administrators can inspect [secret alerts](https://github.com/Rekabytes-Enterprise/hibiscus/security/secret-scanning).
GitHub blocks pushes for its supported patterns; pattern coverage and scan-size
limits still apply. A settings readback is not an end-to-end blocked-push test.
See [GitHub's detection scope](https://docs.github.com/en/code-security/reference/secret-security/secret-scanning-scope).

The optional non-provider-pattern setting remained disabled after an enablement
request, and GitHub's scan-history endpoint returned unavailable. Do not infer
historical scan completion from an empty alerts list. No paid security feature or
organization-wide setting was enabled as a workaround.

## Local and CI coverage

`scripts/secret-scanning.py` owns a checksum-pinned Gitleaks 8.30.1 bootstrap and
redacted invocation. It is development tooling, with no installed Hibiscus
runtime dependency. Linux and macOS archives are pinned independently. A bad
download fails before extraction; only the named executable is extracted into a
new private directory. A scan requires the pinned version and drops inherited
provider credentials, Git authentication configuration and scanner overrides.

From a trusted checkout:

```sh
secret_scan_dir=$(mktemp -d)
scanner=$(python3 scripts/secret-scanning.py install --directory "$secret_scan_dir/gitleaks")
python3 tests/secret_scanning.py --scanner "$scanner"
python3 scripts/secret-scanning.py git --scanner "$scanner"
```

The existing required `Format, Clippy, and installer` job runs the same controls
and scan. Its checkout fetches full history; scanning refuses shallow clones.
Every reachable ref and merge-parent diff is scanned, including removed
credentials and credentials introduced only by a merge. Scanner failure, missing
or inconsistent reports and timeouts fail the job. Findings return exit 1;
incomplete scans return exit 2. No scan report is uploaded as a public artifact.

`.gitleaks.toml` extends the pinned upstream provider rules and adds two rules:

- `auth.json` string values, where provider IDs are keys and opaque credentials
  need not have a provider-specific prefix.
- `codex-auth.json` access/refresh values, including opaque refresh credentials.

These rules are restricted to the native store filenames and credential fields.
They do not constitute universal credential detection. Provider formats change;
short values, escaped JSON, binary data and unrecognized formats may need review.
Keep actual stores outside version control. Do not scan your real home just to
run tests.

The control suite uses an expired public [RFC 7519 section 3.1 example](https://www.rfc-editor.org/rfc/rfc7519#section-3.1)
and locally generated, unissued dummy values. It sends no authentication requests
and publishes no credential-shaped test file. Existing native synthetic test
credentials remain unchanged. Inline `gitleaks:allow` comments, repository ignore
files and inherited scanner overrides cannot suppress this invocation.

## Findings and false positives

Reports and scanner stderr stay in private temporary storage and are discarded
after parsing. Only rule/path/line/commit metadata and counts are printed. Do not
paste matched values, raw logs or an unredacted report into an issue, task or PR.

For a possible real credential, investigate privately and revoke or rotate with
its provider before considering history cleanup. Deleting the current file does
not revoke a credential or remove its history. Use the private channel described
in [the security policy](../../SECURITY.md).

For a false positive, record the rule and location, establish why the value is
public or synthetic, and add a regression control before changing detection.
Any exception needs review and must be specific to that rule and fixture; never
exclude a whole test tree, provider rule or history baseline. There are no
Hibiscus-specific exclusions today. The upstream default rules retain their
documented handling of known public examples.

During the issue #47 audit, four generic-key log findings were exact public
provenance cache keys. Their digest matched the verified public validation
bundle. They were classified using that evidence, without adding a detector
allowlist. This review covers the selected retained runs, not every historical
log or an assurance that all possible secrets are absent.
