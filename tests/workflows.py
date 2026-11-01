"""Exercise the actual release boundary scripts without credentials or publication.

This uses the fixed workflow indentation to extract shell blocks, not a YAML
parser. actionlint checks YAML, expressions and action schemas independently.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[1]
RELEASE = ROOT / '.github/workflows/release.yml'
BUILD = ROOT / '.github/workflows/release-build.yml'
TARGETS = {
    'linux_x64': ('x86_64-unknown-linux-musl', 'ubuntu-latest'),
    'linux_arm64': ('aarch64-unknown-linux-musl', 'ubuntu-latest'),
    'macos_x64': ('x86_64-apple-darwin', 'macos-15-intel'),
    'macos_arm64': ('aarch64-apple-darwin', 'macos-15'),
}


def job(source, name):
    return re.search(r'^  ' + name + r':\n(.*?)(?=^  \w+:\n|\Z)', source, re.M | re.S)[1]


def shell(path, name):
    lines = path.read_text().splitlines()
    start = lines.index('      - name: ' + name)
    run = next(i for i in range(start, len(lines)) if lines[i] == '        run: |')
    end = run + 1
    while end < len(lines) and (not lines[end] or lines[end].startswith('          ')):
        end += 1
    return textwrap.dedent('\n'.join(lines[run + 1:end])) + '\n'


def run(script, directory, env=None):
    values = {'PATH': os.environ['PATH'], 'HOME': str(directory), 'LC_ALL': 'C'}
    values.update(env or {})
    return subprocess.run(['/bin/bash', '-euo', 'pipefail', '-c', script], cwd=directory,
                          env=values, capture_output=True, text=True, timeout=15)


class WorkflowPolicy(unittest.TestCase):
    def test_all_external_actions_are_immutable(self):
        for path in (ROOT / '.github/workflows').glob('*.yml'):
            for line in path.read_text().splitlines():
                if not re.match(r'\s*(?:-\s+)?uses:', line):
                    continue
                ref = line.split('uses:', 1)[1].strip().split()[0]
                self.assertTrue(ref.startswith('./.github/workflows/') or
                                re.fullmatch(r'[\w./-]+@[0-9a-f]{40}', ref), (path, ref))

    def test_only_publisher_has_repository_write_access(self):
        source = RELEASE.read_text()
        self.assertIn('\npermissions:\n  contents: read\n', source)
        self.assertEqual(len(re.findall(r'contents: write', source)), 1)
        publisher = job(source, 'publish')
        self.assertIn('permissions:\n      contents: write', publisher)
        for unsafe in ['actions/checkout@', 'cargo ', 'cross build', 'secrets: inherit']:
            self.assertNotIn(unsafe, publisher)
        self.assertIn("if: github.event_name == 'push' || inputs.release_tag != ''", publisher)
        for path in [BUILD, ROOT / '.github/workflows/ci.yml']:
            self.assertNotIn(': write', path.read_text())
        self.assertNotIn('GH_TOKEN', BUILD.read_text())
        self.assertNotIn('secrets: inherit', source)

    def test_checkouts_do_not_persist_credentials(self):
        for path in (ROOT / '.github/workflows').glob('*.yml'):
            lines = path.read_text().splitlines()
            for i, line in enumerate(lines):
                if 'uses: actions/checkout@' in line:
                    self.assertIn('persist-credentials: false', '\n'.join(lines[i + 1:i + 6]))

    def test_four_receipts_do_not_use_matrix_last_outputs(self):
        source = RELEASE.read_text()
        self.assertEqual(set(re.findall(r'^      target: (.+)$', source, re.M)),
                         {t for t, _ in TARGETS.values()})
        for name, (target, runner) in TARGETS.items():
            block = job(source, name)
            self.assertIn('uses: ./.github/workflows/release-build.yml', block)
            self.assertIn('target: ' + target, block)
            self.assertIn('runner: ' + runner, block)
            self.assertIn('contents: read', block)
            self.assertIn('source_ref: ${{ needs.verify.outputs.source_commit }}', block)
            self.assertIn('needs.' + name + '.outputs.sha256', source)
            self.assertIn('needs.' + name + '.outputs.cache_key', source)

    def test_transport_validation_and_publisher_collect_identically(self):
        source = RELEASE.read_text()
        readonly = job(source, 'verify_transport').split('    steps:\n', 1)[1].strip()
        publisher = job(source, 'publish').split('    steps:\n', 1)[1]
        self.assertEqual(readonly, publisher.split('      - name: Publish verified release assets')[0].strip())
        self.assertEqual(readonly.count('fail-on-cache-miss: true'), 4)
        self.assertNotIn('restore-keys:', readonly)

    def test_toolchain_images_and_cross_are_pinned(self):
        source = BUILD.read_text()
        self.assertEqual(source.count('@sha256:'), 2)
        for value in re.findall(r'IMAGE: (.+)', source):
            self.assertRegex(value, r'^ghcr.io/cross-rs/[\w-]+@sha256:[0-9a-f]{64}$')
        self.assertIn('cargo install cross --locked --version 0.2.5', source)
        for path in [BUILD, RELEASE, ROOT / '.github/workflows/ci.yml']:
            text = path.read_text()
            self.assertEqual(text.count('uses: dtolnay/rust-toolchain@'), text.count('toolchain: 1.99.1'))
        self.assertIn('archive: true', source)

    def test_exact_tag_verification_and_readonly_draft_preflight_remain(self):
        source = RELEASE.read_text()
        self.assertIn('git rev-parse "$RELEASE_TAG^{commit}"', source)
        self.assertIn('test "v$version" = "$RELEASE_TAG"', source)
        self.assertIn("if: inputs.direct_upload && inputs.release_tag != ''", source)
        self.assertNotIn('contents: write', job(source, 'verify'))


class ReleaseShell(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='hibiscus-workflow-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / 'dist').mkdir()
        self.env = {}
        for name, (target, _) in TARGETS.items():
            value = ('fixture-' + target).encode()
            (self.root / 'dist' / ('hibiscus-' + target + '.tar.gz')).write_bytes(value)
            self.env[name.upper() + '_SHA'] = hashlib.sha256(value).hexdigest()
        self.verify = shell(RELEASE, 'Verify compiler receipts')

    def test_exact_four_receipts_pass(self):
        self.assertEqual(run(self.verify, self.root, self.env).returncode, 0)

    def test_modified_archive_fails(self):
        next((self.root / 'dist').iterdir()).write_bytes(b'tampered')
        self.assertNotEqual(run(self.verify, self.root, self.env).returncode, 0)

    def test_missing_archive_fails(self):
        next((self.root / 'dist').iterdir()).unlink()
        self.assertNotEqual(run(self.verify, self.root, self.env).returncode, 0)

    def test_unexpected_or_hidden_files_fail(self):
        for name in ['unexpected.tar.gz', '.hidden']:
            p = self.root / 'dist' / name
            p.write_bytes(b'extra')
            self.assertNotEqual(run(self.verify, self.root, self.env).returncode, 0)
            p.unlink()

    def test_symlink_archive_fails_even_with_matching_content(self):
        p = next((self.root / 'dist').iterdir())
        outside = self.root / 'original'
        p.rename(outside)
        p.symlink_to(outside)
        self.assertNotEqual(run(self.verify, self.root, self.env).returncode, 0)

    def test_missing_or_malformed_expected_digest_fails(self):
        for value in ['', 'z' * 64, 'a' * 63, 'a' * 64 + '\n*file']:
            env = dict(self.env, LINUX_X64_SHA=value)
            self.assertNotEqual(run(self.verify, self.root, env).returncode, 0)

    def test_target_runner_boundary_accepts_only_supported_pairs(self):
        script = shell(BUILD, 'Validate target and runner')
        for target, runner in TARGETS.values():
            self.assertEqual(run(script, self.root, {'TARGET': target, 'BUILD_RUNNER': runner}).returncode, 0)
        for target, runner in [('x86_64-apple-darwin', 'ubuntu-latest'), ('$(touch injected)', 'ubuntu-latest')]:
            self.assertNotEqual(run(script, self.root, {'TARGET': target, 'BUILD_RUNNER': runner}).returncode, 0)
        self.assertFalse((self.root / 'injected').exists())

    def test_actual_packaging_generates_digest_and_run_bound_cache_key(self):
        target = TARGETS['macos_arm64'][0]
        binary = self.root / 'target' / target / 'release/hibiscus'
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b'not-executed-fixture')
        binary.chmod(0o755)
        output = self.root / 'outputs'
        result = run(shell(BUILD, 'Package binary and receipt'), self.root,
                     {'TARGET': target, 'GITHUB_OUTPUT': str(output), 'GITHUB_RUN_ID': '123'})
        self.assertEqual(result.returncode, 0, result.stderr)
        archive = self.root / 'dist' / ('hibiscus-' + target + '.tar.gz')
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        self.assertIn('sha256=' + digest, output.read_text())
        self.assertIn('cache_key=release-123-' + target + '-' + digest, output.read_text())
        with tarfile.open(archive) as stream:
            self.assertEqual(stream.getnames(), ['hibiscus'])
            self.assertEqual(stream.extractfile('hibiscus').read(), binary.read_bytes())

    def publisher(self, direct=False, exists=True, draft=True, tag='v1.2.3'):
        directory = self.root / 'bin'
        directory.mkdir(exist_ok=True)
        gh = directory / 'gh'
        gh.write_text('''#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
with Path('calls.jsonl').open('a') as out: out.write(json.dumps(sys.argv[1:])+'\\n')
if sys.argv[1:3]==['release','view']:
 if '--json' in sys.argv: print(os.environ['DRAFT'])
 sys.exit(0 if os.environ['EXISTS']=='true' else 1)
if sys.argv[1:3] not in [['release','upload'],['release','create']]: sys.exit(9)
''')
        gh.chmod(0o755)
        env = {'PATH': str(directory) + os.pathsep + os.environ['PATH'],
               'RELEASE_TAG': tag, 'DIRECT_UPLOAD': str(direct).lower(),
               'EXISTS': str(exists).lower(), 'DRAFT': str(draft).lower(),
               'GH_REPO': 'fixture/repo', 'GH_TOKEN': 'synthetic-not-a-credential'}
        result = run(shell(RELEASE, 'Publish verified release assets'), self.root, env)
        calls = self.root / 'calls.jsonl'
        return result, [json.loads(x) for x in calls.read_text().splitlines()] if calls.exists() else []

    def test_existing_release_upload_and_new_release_verified_creation(self):
        for exists in [True, False]:
            with self.subTest(exists=exists):
                (self.root / 'calls.jsonl').unlink(missing_ok=True)
                result, calls = self.publisher(exists=exists)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(calls[-1][:2], ['release', 'upload' if exists else 'create'])
                if not exists:
                    self.assertIn('--verify-tag', calls[-1])
                self.assertEqual(len((self.root / 'dist/SHA256SUMS').read_text().splitlines()), 4)

    def test_direct_upload_requires_draft(self):
        result, calls = self.publisher(direct=True, draft=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[-1][:2], ['release', 'upload'])

    def test_direct_upload_refuses_published_or_missing_release(self):
        for exists, draft in [(True, False), (False, False)]:
            (self.root / 'calls.jsonl').unlink(missing_ok=True)
            result, calls = self.publisher(direct=True, exists=exists, draft=draft)
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(all(x[:2] == ['release', 'view'] for x in calls))

    def test_empty_or_invalid_tag_never_contacts_github(self):
        for tag in ['', 'v1.2.3;touch injected', 'v01.2.3']:
            result, calls = self.publisher(tag=tag)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(calls, [])
        self.assertFalse((self.root / 'injected').exists())


if __name__ == '__main__':
    unittest.main(verbosity=2)
