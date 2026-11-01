"""Real offline cryptographic controls using the pinned official GitHub CLI."""
import argparse
import base64
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

FIXTURE = Path(__file__).resolve().parent / 'fixtures/provenance'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verifier', type=Path, required=True)
    args = parser.parse_args()
    version = subprocess.check_output([str(args.verifier), '--version'], text=True)
    if not version.startswith('gh version 2.103.3 '):
        raise RuntimeError('Real controls require checksum-pinned GitHub CLI 2.103.3')
    fixture = json.loads((FIXTURE / 'fixture.json').read_text())
    for name, digest in fixture['files'].items():
        assert hashlib.sha256((FIXTURE / name).read_bytes()).hexdigest() == digest
    artifact_bytes = base64.b64decode((FIXTURE / 'artifact.b64').read_bytes())
    assert hashlib.sha256(artifact_bytes).hexdigest() == fixture['artifact_sha256']
    with tempfile.TemporaryDirectory(prefix='hibiscus-real-provenance-') as temporary:
        root = Path(temporary)
        artifact = root / 'unexecuted-artifact'
        artifact.write_bytes(artifact_bytes)
        bundle = root / 'bundle.jsonl'
        bundle.write_bytes((FIXTURE / 'bundle.jsonl').read_bytes())
        # --bundle disables account authentication; fixed public roots avoid network.
        env = {'PATH': os.defpath, 'HOME': str(root), 'GH_CONFIG_DIR': str(root / 'config'),
               'GH_PROMPT_DISABLED': '1', 'NO_COLOR': '1'}
        flags = {'--repo': fixture['repository'],
                 '--cert-identity': f"https://github.com/{fixture['repository']}/{fixture['workflow']}@{fixture['ref']}",
                 '--cert-oidc-issuer': fixture['issuer'],
                 '--source-ref': fixture['ref'], '--source-digest': fixture['commit'],
                 '--signer-digest': fixture['commit'], '--predicate-type': 'https://slsa.dev/provenance/v1'}

        def verify(overrides=None):
            selected = dict(flags, **(overrides or {}))
            command = [str(args.verifier), 'attestation', 'verify', str(artifact),
                       '--hostname', 'github.com', '--bundle', str(bundle),
                       '--custom-trusted-root', str(FIXTURE / 'trusted-roots.jsonl'),
                       '--deny-self-hosted-runners', '--format', 'json']
            for key, value in selected.items(): command.extend([key, value])
            return subprocess.run(command, env=env, capture_output=True, timeout=30)

        positive = verify()
        assert positive.returncode == 0, positive.stderr.decode(errors='replace')
        result = json.loads(positive.stdout)
        assert len(result) == 1
        assert result[0]['verificationResult']['statement']['subject'][0]['digest']['sha256'] == fixture['artifact_sha256']
        negative_count = 0
        for override in [{'--repo': 'Rekabytes-Enterprise/hibiscus'},
                         {'--cert-identity': 'https://github.com/actions/attest-demo/.github/workflows/wrong.yml@refs/heads/main'},
                         {'--cert-oidc-issuer': 'https://wrong.invalid'},
                         {'--source-ref': 'refs/heads/dev'}, {'--source-digest': '0' * 40},
                         {'--signer-digest': '0' * 40}, {'--predicate-type': 'https://wrong.invalid/provenance'}]:
            assert verify(override).returncode != 0, override
            negative_count += 1
        artifact.write_bytes(artifact_bytes + b'tampered')
        assert verify().returncode != 0
        negative_count += 1
        artifact.write_bytes(artifact_bytes)
        original = json.loads(bundle.read_text())
        changed = copy.deepcopy(original)
        changed['dsseEnvelope']['payload'] = base64.b64encode(b'{"tampered":true}').decode()
        bundle.write_text(json.dumps(changed))
        assert verify().returncode != 0
        negative_count += 1
        changed = copy.deepcopy(original)
        signature = changed['dsseEnvelope']['signatures'][0]['sig']
        changed['dsseEnvelope']['signatures'][0]['sig'] = ('A' if signature[0] != 'A' else 'B') + signature[1:]
        bundle.write_text(json.dumps(changed))
        assert verify().returncode != 0
        negative_count += 1
        bundle.write_text('not JSON')
        assert verify().returncode != 0
        negative_count += 1
        bundle.unlink()
        assert verify().returncode != 0
        negative_count += 1
        print(f'Real provenance verifier passed: one authenticated positive and {negative_count} rejection controls; no credentials or artifact execution.')


if __name__ == '__main__':
    main()
