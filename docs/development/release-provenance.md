# Release provenance design

## Problem

The installer verifies archives against SHA256SUMS from the same release. This
detects corruption but cannot independently establish build origin. The owner
selected a staged rollout: sign releases and verify them before publication,
provide explicit user verification, and keep normal installation lightweight.
The default shell installer and embedded updater remain checksum-only.

Release source and workflow source can differ. An authorized recovery rebuild
uses a current protected workflow to build an older immutable tag. Provenance
must record the actual frozen build source separately from the running workflow.

## Usage

An explicit check from a trusted source checkout uses a locally installed GitHub
CLI and Python, without running the downloaded Hibiscus binary:

```sh
python3 scripts/release_provenance.py verify \
  --archive downloads/hibiscus-aarch64-apple-darwin.tar.gz \
  --bundle downloads/hibiscus-provenance.sigstore.json \
  --tag vMAJOR.MINOR.PATCH --source-commit EXPECTED_40_HEX_TAG_COMMIT
```

For a recovery build, also specify the independently reviewed workflow commit
and its protected ref with `--workflow-commit` and `--workflow-ref`. These flags
describe the signer, while `--source-commit` describes the compiled source.
Missing or invalid provenance fails this explicit command. It never falls back
to checksums or installs an executable.

The workflow calls the same policy owner:

```text
release_provenance.py prepare --directory dist --output provenance/predicate.json
release_provenance.py verifier --directory RUNNER_TEMP/verifier
release_provenance.py verify --directory dist --bundle BUNDLE --context-env
```

## Shape

```python
ExpectedRelease = tag + source_commit + workflow_ref + workflow_commit
BuildContext = ExpectedRelease + source_archive_sha256 + event + run_id + attempt
Artifact = fixed_archive_name + sha256

def predicate(context: BuildContext) -> dict:
    # Canonical SLSA v1 claims, with separate source and workflow dependencies.

def verify_release(artifacts, bundle, expected, verifier) -> None:
    # Cryptographic verification first, then strict signed-statement policy.
```

`scripts/release_provenance.py` owns the metadata schema, archive-name/digest
policy and the official verifier invocation. It uses the standard library and
does not implement cryptography. GitHub CLI verifies Sigstore certificates,
signatures and witnessed timestamps. Verification runs with an isolated CLI
profile and a filtered environment, without account/provider credentials. The policy then checks the verified
statement's source/tag and exact subjects.

The producer, compilers and existing binary receipt checker stay readonly. A
separate attestor restores and verifies the four compiler archives. Only it gets
OIDC and attestation-write permissions. It loads the policy script by sparse
checkout of `github.workflow_sha`, after a literal workflow-context guard. This
is trusted orchestration code, not code from the historical release checkout or
its dependencies. It preserves one policy implementation and old-tag rebuilds.
The existing publisher remains checkout-free and the sole repository-content
writer. It verifies both compiler receipts and the attestor's exact bundle
digest before upload; it does not execute either archives or policy source.

Normal bundle transport uses an artifact outside `release-*`. Recovery uses an
exact run/attempt/bundle-digest cache key and no prefix fallback. No-tag build
checks skip signing and publishing; testing real OIDC signing needs an explicitly
authorized draft tag. Existing published tags/assets stay immutable.

## Build type v1

The predicate uses SLSA provenance v1 and GitHub's supported
[`https://actions.github.io/buildtypes/workflow/v1`](https://github.com/actions/buildtypes/tree/main/workflow/v1)
build type. `externalParameters.workflow` identifies the executing repository,
workflow path and ref. Required internal GitHub parameters bind the numeric
repository/owner IDs, event and hosted runner. `internalParameters.hibiscus`
records the immutable release tag, supported targets and run/attempt. The first
resolved dependency is the executing workflow commit/ref; the second is the
actual compiled release source. Both are required and checked independently. `runDetails` records the signer workflow, invocation and verified source
archive SHA-256. These are builder claims authenticated by the signature. The
certificate's immutable identity claims are checked separately by GitHub CLI.

For tag pushes, source and workflow must have the same tag and commit. Recovery
uses only `refs/heads/dev` or `refs/heads/main` as signer refs. CI also requires
the exact current run, attempt and source-archive digest. A manual check pins the
expected tag, actual source and reviewed signer identity. The signed subject set
must contain exactly the four supported archive basenames and SHA-256 digests;
selected local archives must match their signed digests.

## Synthesis decision

Two read-only designs were compared. The base is a separate GitHub attestor with
a strictly checked SLSA v1 predicate using the supported GitHub workflow build type, adapted to the owner's staged-install choice. A
shared policy script avoids duplicating identity rules in workflow literals and
manual commands. Its fixed-workflow checkout changes the proposed entirely
checkout-free attestor into one that executes only reviewed orchestration code;
the publisher and compiler credential boundaries remain intact.

Bootstrapping pinned Cosign into every installation was rejected for this stage
because its current platform binaries add roughly 100–146 MB. The alternative
SLSA generic generator/verifier route is no longer recommended for new adoption
by its maintainers, and its required tag refs conflict with this repository's
full-commit action pins. Its verifier also does not accept arbitrary Hibiscus
builder identities without additional policy.

## Tradeoffs and verification limits

- CI may download a pinned GitHub CLI verifier. Normal installs gain no verifier
  dependency, download, account requirement or additional prompt.
- Explicit provenance verification requires a trusted policy checkout and GitHub
  CLI. This stage does not authenticate default install/update downloads.
- The bundle binds all four archive digests to the actual source and signer.
  It does not establish reproducible builds, a SLSA assurance level, or that the
  source and dependencies are free of vulnerabilities.
- Real GitHub OIDC generation, both signing transports and publication must be
  exercised before closing #49 or claiming a measured Scorecard increase.

## Measure after publication

Release assets appear after the main-push Scorecard run has usually finished.
After an authorized release, re-run that commit's existing default-branch push
Scorecard run and compare the retained report with the public API. GitHub reruns
retain the original event SHA/ref. Avoid adding marker commits just to refresh
the badge, and do not assume a successful job proves public publication.
Historical unsigned releases can still limit Signed-Releases scoring; report
only the newly observed result, without altering older tags or assets.

## References

- [GitHub artifact attestations](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)
- [Attestation action inputs](https://github.com/actions/attest/tree/1e69f48acb82d1966a394da916b4c1698aa569d6)
- [GitHub CLI verification](https://cli.github.com/manual/gh_attestation_verify)
- [SLSA provenance v1](https://slsa.dev/provenance/v1)

## Hosted compatibility check

The first draft signing run rejected the original custom build-type URI at
GitHub's attestation storage boundary. The corrected schema follows the supported
GitHub workflow build type, including its required workflow and internal GitHub
parameters. Hibiscus-specific release claims remain in internal parameters and a
separate source dependency. Offline tests cannot establish service acceptance;
actual draft signing and recovery remain required. Never move a frozen tag to
repair orchestration.
