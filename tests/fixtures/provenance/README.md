# Public signature fixture

`artifact.b64` encodes a small wheel from the GitHub CLI's own integration tests.
Tests decode it into a temporary file for hashing; they never install, import or
execute it. Keeping it as text avoids adding an executable/binary to source.

`bundle.jsonl` is its original signed attestation. Both originate from
[GitHub CLI v2.103.3 test data](https://github.com/cli/cli/tree/v2.103.3/pkg/cmd/attestation/test/data):

- `github_provenance_demo-0.0.12-py3-none-any.whl`
- `github_provenance_demo-0.0.12-py3-none-any-bundle.jsonl`

The copied fixture is covered by the upstream [MIT license](UPSTREAM-LICENSE.txt),
retained verbatim with its copyright and permission notice.

`trusted-roots.jsonl` is a public trust-root snapshot obtained on 2026-11-01
through checksum-pinned GitHub CLI v2.103.3's `attestation trusted-root` command.
The same CLI successfully verified the fixture with both its normal trust-root
lookup and this snapshot. The snapshot makes cryptographic tests independent of
live services. Production verification uses the CLI's normal trust mechanism;
the production policy exposes no custom-root or verification-bypass option.

`fixture.json` pins the public files and expected identity. These files contain
public certificates, signatures and metadata, not private keys or credentials.
The fixture proves verifier operation, not a Hibiscus signing run. Actual
Hibiscus OIDC generation and recovery still need authorized hosted tests.
