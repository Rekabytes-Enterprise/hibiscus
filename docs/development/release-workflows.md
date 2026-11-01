# Release workflow security

## Usage

Normal tag pushes verify the source, build all four archives, verify the compiler hashes and publish through a separate job. An existing draft receives assets but stays a draft for independent inspection. Existing tags are never moved.

To test builds and the ordinary artifact transport without publishing:

```sh
gh workflow run release.yml --ref dev
```

To test the cache recovery transport without publishing:

```sh
gh workflow run release.yml --ref dev -f direct_upload=true
```

For an explicitly authorized rebuild of an existing immutable tag, provide `release_tag`. The workflow verifies its version and exact checked-out commit. Recovery with `direct_upload=true` requires that tag's GitHub Release to already be a draft. No-tag dispatches never run the writer. Use the current workflow on `dev` even when rebuilding an older source tag.

## Ownership and trust

`.github/workflows/release.yml` owns verification and publication. `release-build.yml` owns the compilation, packaging and compiler receipts for each supported target. All jobs default to `contents: read`. The publisher alone gets `contents: write`; it has no repository checkout, compiler or source execution. Checkouts do not persist credentials. Only the actual publishing step receives `GH_TOKEN` in its environment.

External actions use reviewed full commit references. Rust is fixed at 1.99.1, matching the stable release verified during this change. Cross is installed at 0.2.5 with its lockfile, and its two Linux images are selected by immutable registry digests. Updates require a reviewed change; dependency update automation is tracked separately in #45. Hosted runner images and their preinstalled OS tools remain controlled by GitHub, so these pins do not promise reproducible builds.

Verification freezes the checked-out source commit and passes that exact SHA to every compiler, so a later branch push cannot change build inputs. Each of the four build calls has independent digest/key outputs. This avoids losing receipts to matrix output ordering. Normal transport uses archived Actions artifacts. Recovery saves each archive in an exact run/target/digest cache key and restores that exact entry with no prefix fallback. Cache is transport, not an authenticity authority. Both the read-only transport checker and the publisher verify the expected four regular, nonsymlink files against the separate compiler outputs. Unexpected files, missing entries or differing bytes fail before release upload. No archive contents are executed by the publisher.

Actions cache has its own capacity and retention limits. Recovery still needs available cache storage and fails on a cache miss. It does not provide a fallback when both storage services are unavailable. Archive SHA-256 checks are integrity checks, not cryptographic build provenance; signing is tracked separately in #49.

## Design decision

The earlier workflow compiled and uploaded in the same matrix job. Reducing permissions only for ordinary builds would leave the same write-token exposure during recovery. Duplicating that matrix into a privileged recovery builder was considered and rejected for that reason.

The selected design keeps one reusable read-only compiler and separates transport from publishing. It accepts explicit per-target calls and a cache capacity requirement in exchange for keeping repository write credentials out of dependency build scripts. The supported targets, calls and receipts are checked together by the workflow tests.

## Verification

`python3 tests/workflows.py` executes the actual boundary scripts with temporary files and a synthetic GitHub CLI. It checks tampering, invalid/missing receipts, unexpected files, symlinks, supported target/runner pairs, exact packaging and draft/new-release behavior. It performs no remote writes. actionlint checks workflow syntax, expressions and declared action inputs separately. Real no-tag dispatches validate all four builds and both transport services before main promotion. These tests do not establish signed provenance or a sandbox for application tools.
