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

`.github/workflows/release.yml` owns source verification, tests, four explicit compiler jobs, transport and publication. All jobs default to `contents: read`. The publisher alone gets `contents: write`; it has no repository checkout, compiler or source execution. Checkouts do not persist credentials. Source authorization and draft preflight receive a read-only `GH_TOKEN`. Only the actual publishing step receives a repository-write token.

External actions use reviewed full commit references. Rust is fixed at 1.99.1, matching the stable release verified during this change. Cross is installed at 0.2.5 with its lockfile, and its two Linux images are selected by immutable registry digests. Updates require a reviewed change; Dependabot proposes Cargo and Actions updates. Hosted runner images and their preinstalled OS tools remain controlled by GitHub, so these pins do not promise reproducible builds.

Before checkout, verification resolves the requested tag or event commit through GitHub and requires it to be an ancestor of protected `main` or `dev`. The checkout must match that authorized SHA. Tagged builds also check the tag object and Cargo version. The verifier runs no repository programs. It creates a Git archive of that exact commit, disables export attributes so they cannot omit or substitute source bytes, and rejects tracked symlinks and submodules. Working-tree edits, untracked files and Git metadata are absent from the snapshot.

A separate test job and all four compiler jobs consume the snapshot. They never select or check out a Git ref. Each checks the archive against the verifier's SHA-256 before parsing it, then validates every member before writing files into an empty workspace. Absolute/traversing paths, duplicates, file/directory conflicts, links, special files and Git metadata fail. Limits are 64 MiB per archive, 16 MiB per file, 10,000 members and 1,024 characters per path. Only ordinary file modes and the executable bit survive. The unpacker comes from the running workflow, so rebuilding an old tag does not require newer helper files in that tag.

Normal source transport uses the fixed `verified-source` artifact from the current run. Recovery uses an exact run/attempt/commit/digest cache key, without prefix fallback. Both sides use the same relative `release-source-transport/source.tar` cache path because the cache version includes its path string. Consumers stage it outside the workspace before unpacking; absolute runner-temp paths differ across Linux and macOS and cannot share a cache version. A failed-job rerun can consume the original successful verifier's outputs and cache entry. Tests run on their own runner, so repository programs cannot change the verifier's source receipt. Compilers wait for those tests before building.

Each compiler has independent binary archive digest/key outputs. This avoids losing receipts to matrix output ordering. Normal binary transport uses archived Actions artifacts. Recovery saves each archive in an exact run/target/digest cache key and restores that exact entry with no prefix fallback. Cache is transport, not an authenticity authority. Both the read-only transport checker and the publisher verify the expected four regular, nonsymlink files against the separate compiler outputs. Unexpected files, missing entries or differing bytes fail before release upload. No archive contents are executed by the publisher.

Actions cache has its own capacity and retention limits. Recovery still needs available cache storage and fails on a cache miss. It does not provide a fallback when both storage services are unavailable. Archive SHA-256 checks are integrity checks, not cryptographic build provenance; signing is tracked separately in #49.

## Design decision

The earlier workflow compiled and uploaded in the same matrix job. Reducing permissions only for ordinary builds would leave the same write-token exposure during recovery. Duplicating that matrix into a privileged recovery builder was considered and rejected for that reason.

A reusable compiler accepting a source ref exposed a separate checkout boundary for every call. The replacement authorizes and packages source once, then hands checked bytes to explicit read-only test and compiler jobs. The four compiler step bodies must stay identical, enforced by tests. This adds a source transport step and accepts its storage requirement in exchange for removing Git-ref selection from jobs that execute repository programs.
## Verification

`python3 tests/workflows.py` executes the actual boundary scripts with temporary files and a synthetic GitHub CLI. It checks source and binary tampering, unsafe/oversized archives, committed-byte preservation despite export attributes, executable modes, invalid/missing receipts, unexpected files, symlinks, supported target/runner pairs, exact packaging and draft/new-release behavior. It performs no remote writes. actionlint checks workflow syntax, expressions and declared action inputs separately. Before main promotion, explicitly authorized no-tag dispatches should validate all four builds and both transport services. These tests do not establish signed provenance or a sandbox for application tools.

The alert fix was also checked with the action-linked CodeQL bundle 2.27.3 and unchanged `security-extended` suites. The baseline reproduced four Actions findings and one Python finding. The complete modified source produced zero findings across the same 23 Actions and 50 Python query rules. This local result does not close GitHub alerts; GitHub must scan the merged fix on its tracked branches. No alert dismissal, query exclusion or analysis-scope reduction is part of the change.
