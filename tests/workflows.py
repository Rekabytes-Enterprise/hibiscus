"""Exercise the actual release boundary scripts without credentials or publication.

This uses the fixed workflow indentation to extract shell blocks, not a YAML
parser. actionlint checks YAML, expressions and action schemas independently.
"""
import hashlib
import io
import ast
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
    def test_required_checks_cover_both_branches_without_secret_pr_execution(self):
        paths = ['ci.yml', 'dependency-security.yml', 'codeql.yml']
        for name in paths:
            text = (ROOT / '.github/workflows' / name).read_text()
            for event in ['push', 'pull_request']:
                section = re.search(r'^  ' + event + r':\n(.*?)(?=^  \w|\npermissions:)',
                                    text, re.M | re.S)[1]
                self.assertIn('dev', section)
                self.assertIn('main', section)
                self.assertNotIn('paths:', section)
                self.assertNotIn('paths-ignore:', section)
            for unsafe in ['pull_request_target:', 'secrets.', 'secrets: inherit',
                           'contents: write', 'id-token: write']:
                self.assertNotIn(unsafe, text)

    def test_codeql_uses_isolated_upload_permissions_and_actual_rust_probe(self):
        text = (ROOT / '.github/workflows/codeql.yml').read_text()
        self.assertIn('\npermissions:\n  contents: read\n', text)
        self.assertNotIn('security-events:', text.split('\njobs:')[0])
        self.assertEqual(text.count('security-events: write'), 1)
        self.assertIn('language: [rust, javascript-typescript, python, actions]', text)
        for required in ['build-mode: none', 'tools: linked', 'queries: security-extended',
                         'wait-for-processing: true', 'python3 scripts/verify-codeql.py']:
            self.assertIn(required, text)
        self.assertNotIn('continue-on-error:', text)
        self.assertNotIn('upload: never', text)

    def test_branch_rules_have_no_bypass_and_preserve_merge_protections(self):
        rules = json.loads((ROOT / '.github/security/branch-rules.json').read_text())
        self.assertEqual(rules['enforcement'], 'active')
        self.assertEqual(rules['bypass_actors'], [])
        self.assertEqual(set(rules['conditions']['ref_name']['include']),
                         {'refs/heads/main', 'refs/heads/dev'})
        self.assertEqual(rules['conditions']['ref_name']['exclude'], [])
        by_type = {rule['type']: rule.get('parameters', {}) for rule in rules['rules']}
        self.assertTrue({'deletion', 'non_fast_forward', 'pull_request',
                         'required_status_checks'} <= by_type.keys())
        self.assertTrue(by_type['pull_request']['required_review_thread_resolution'])
        checks = by_type['required_status_checks']
        self.assertTrue(checks['strict_required_status_checks_policy'])
        self.assertFalse(checks['do_not_enforce_on_create'])
        self.assertEqual(len(checks['required_status_checks']), 8)
        self.assertTrue(all(c['integration_id'] == 15368
                            for c in checks['required_status_checks']))

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
        for name in TARGETS:
            self.assertNotIn(': write', job(source, name))
            self.assertNotIn('GH_TOKEN', job(source, name))
        self.assertNotIn(': write', (ROOT / '.github/workflows/ci.yml').read_text())
        self.assertNotIn('secrets: inherit', source)

    def test_checkouts_do_not_persist_credentials(self):
        for path in (ROOT / '.github/workflows').glob('*.yml'):
            lines = path.read_text().splitlines()
            for i, line in enumerate(lines):
                if 'uses: actions/checkout@' in line:
                    end = next((j for j in range(i + 1, len(lines)) if lines[j].startswith('      - ')), len(lines))
                    self.assertIn('persist-credentials: false', '\n'.join(lines[i + 1:end]))

    def test_four_receipts_do_not_use_matrix_last_outputs(self):
        source = RELEASE.read_text()
        self.assertEqual(set(re.findall(r'^      TARGET: (.+)$', source, re.M)),
                         {t for t, _ in TARGETS.values()})
        for name, (target, runner) in TARGETS.items():
            block = job(source, name)
            self.assertIn('needs: [verify, test_source]', block)
            self.assertNotIn('uses: ./.github/workflows/', block)
            self.assertIn('TARGET: ' + target, block)
            self.assertIn('runs-on: ' + runner, block)
            self.assertIn('BUILD_RUNNER: ' + runner, block)
            self.assertIn('contents: read', block)
            self.assertNotIn('actions/checkout@', block)
            self.assertIn('SOURCE_SHA256: ${{ needs.verify.outputs.source_sha256 }}', block)
            self.assertIn('needs.' + name + '.outputs.sha256', source)
            self.assertIn('needs.' + name + '.outputs.cache_key', source)

    def test_transport_validation_and_publisher_collect_identically(self):
        source = RELEASE.read_text()
        readonly = job(source, 'verify_transport').split('    steps:\n', 1)[1].strip()
        publisher = job(source, 'publish').split('    steps:\n', 1)[1]
        self.assertEqual(readonly, publisher.split('      - name: Download verified provenance')[0].strip())
        self.assertEqual(readonly.count('fail-on-cache-miss: true'), 4)
        self.assertNotIn('restore-keys:', readonly)

    def test_compiler_steps_share_one_contract_without_callable_source_inputs(self):
        source = RELEASE.read_text()
        steps = [job(source, name).split('    steps:\n', 1)[1] for name in TARGETS]
        self.assertEqual(len(set(steps)), 1, 'Compiler steps must change together')
        self.assertNotIn('workflow_call:', source)
        self.assertNotIn('inputs.source_ref', source)
        self.assertFalse((ROOT / '.github/workflows/release-build.yml').exists())

    def test_source_receipt_is_produced_before_any_repository_program_runs(self):
        source = RELEASE.read_text()
        verifier = job(source, 'verify')
        for executable in ['cargo ', 'cross ', 'node ', 'sh tests/', 'python3 tests/']:
            self.assertNotIn(executable, verifier)
        self.assertIn('Package authorized source snapshot', verifier)
        self.assertIn('source_sha256: ${{ steps.snapshot.outputs.sha256 }}', verifier)
        consumers = [job(source, name) for name in ['test_source'] + list(TARGETS)]
        for consumer in consumers:
            self.assertNotIn('actions/checkout@', consumer)
            self.assertNotIn('restore-keys:', consumer)
            self.assertIn('fail-on-cache-miss: true', consumer)
            self.assertIn('name: verified-source', consumer)
            self.assertIn('key: ${{ needs.verify.outputs.source_cache_key }}', consumer)
            self.assertIn('path: release-source-transport/source.tar', consumer)
            self.assertNotIn('path: ${{ runner.temp }}', consumer)
            self.assertIn('name: Stage source archive outside workspace', consumer)
            self.assertIn('python3 -c "$HIBISCUS_SOURCE_UNPACK_SCRIPT"', consumer)
            self.assertNotIn('run-id:', consumer)
            self.assertNotIn('github-token:', consumer)
        self.assertEqual(source.count('HIBISCUS_SOURCE_UNPACK_SCRIPT: |'), 1)
        self.assertEqual(source.count('actions/checkout@'), 2)

    def test_signing_authority_is_isolated_from_builds_and_publisher(self):
        source = RELEASE.read_text()
        signer = job(source, 'provenance')
        self.assertEqual(source.count('id-token: write'), 1)
        self.assertEqual(source.count('attestations: write'), 1)
        for permission in ['contents: read', 'id-token: write', 'attestations: write']:
            self.assertIn(permission, signer)
        self.assertIn("if: github.event_name == 'push' || inputs.release_tag != ''", signer)
        self.assertIn('verify_transport', signer.split('    steps:')[0])
        self.assertIn('SOURCE_COMMIT: ${{ needs.verify.outputs.source_commit }}', signer)
        self.assertIn('SOURCE_SHA256: ${{ needs.verify.outputs.source_sha256 }}', signer)
        self.assertIn('ref: ${{ github.workflow_sha }}', signer)
        self.assertIn('sparse-checkout: scripts/release_provenance.py', signer)
        self.assertLess(signer.index('name: Authorize signer workflow'), signer.index('uses: actions/checkout@'))
        self.assertLess(signer.index('name: Verify compiler receipts'), signer.index('name: Sign release archives'))
        self.assertLess(signer.index('name: Verify signed release identity'), signer.index('name: Record verified provenance receipt'))
        for unsafe in ['cargo ', 'cross ', 'npm ', 'secrets: inherit', 'contents: write']:
            self.assertNotIn(unsafe, signer)
        self.assertIn('--context-env --verifier "$VERIFIER_PATH"', signer)
        publisher = job(source, 'publish')
        self.assertIn('needs: [provenance, verify_transport,', publisher)
        for unsafe in ['id-token:', 'attestations:', 'python3 ', 'actions/checkout@']:
            self.assertNotIn(unsafe, publisher)

    def test_provenance_transport_is_distinct_and_exact(self):
        source = RELEASE.read_text()
        for name in ['provenance', 'publish']:
            block = job(source, name)
            self.assertIn('name: verified-provenance', block)
            self.assertNotIn('name: release-provenance', block)
            self.assertIn('path: provenance/hibiscus-provenance.sigstore.json', block)
            self.assertNotIn('restore-keys:', block)
        publisher = job(source, 'publish')
        self.assertIn('key: ${{ needs.provenance.outputs.cache_key }}', publisher)
        self.assertIn('PROVENANCE_SHA256: ${{ needs.provenance.outputs.sha256 }}', publisher)
        self.assertLess(publisher.index('name: Verify attestor receipt'), publisher.index('name: Publish verified release assets'))
        signer = job(source, 'provenance')
        self.assertIn('cache_key=provenance-%s-%s-%s', signer)
        self.assertIn('"$GITHUB_RUN_ID" "$GITHUB_RUN_ATTEMPT" "$digest"', signer)

    def test_provenance_targets_and_required_ci_match_release_contract(self):
        tree = ast.parse((ROOT / 'scripts/release_provenance.py').read_text())
        definition = next(node.value for node in tree.body if isinstance(node, ast.Assign)
                          and any(isinstance(t, ast.Name) and t.id == 'TARGETS' for t in node.targets))
        self.assertEqual(set(ast.literal_eval(definition)), {target for target, _ in TARGETS.values()})
        ci = (ROOT / '.github/workflows/ci.yml').read_text()
        self.assertIn('python3 tests/release_provenance.py', ci)
        self.assertIn('python3 tests/provenance_verifier.py --verifier "$verifier"', ci)
        self.assertIn('scripts/release_provenance.py verifier --directory "$RUNNER_TEMP/provenance-verifier"', ci)

    def test_toolchain_images_and_cross_are_pinned(self):
        source = RELEASE.read_text()
        for name in TARGETS:
            compiler = job(source, name)
            self.assertEqual(compiler.count('@sha256:'), 2)
            for value in re.findall(r'IMAGE: (.+)', compiler):
                self.assertRegex(value, r'^ghcr.io/cross-rs/[\w-]+@sha256:[0-9a-f]{64}$')
            self.assertIn('cargo install cross --locked --version 0.2.5', compiler)
            self.assertIn('archive: true', compiler)
        for path in [RELEASE, ROOT / '.github/workflows/ci.yml']:
            text = path.read_text()
            self.assertEqual(text.count('uses: dtolnay/rust-toolchain@'), text.count('toolchain: 1.99.1'))

    def test_exact_tag_verification_and_publisher_owned_draft_enforcement(self):
        source = RELEASE.read_text()
        self.assertIn('git rev-parse "$RELEASE_TAG^{commit}"', source)
        self.assertIn('test "v$version" = "$RELEASE_TAG"', source)
        self.assertNotIn("gh release view", job(source, 'verify'))
        self.assertIn("REBUILD: ${{ github.event_name == 'workflow_dispatch' }}", job(source, 'publish'))
        self.assertNotIn('contents: write', job(source, 'verify'))
        self.assertIn("inputs.release_tag || (github.ref_type == 'tag' && github.ref) || github.sha", source)
        self.assertLess(source.index('name: Authorize merged release source'),
                        source.index('uses: actions/checkout@'))
        self.assertIn('test "$(git rev-parse HEAD)" = "$AUTHORIZED_COMMIT"', source)


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

    def test_release_compiler_accepts_only_a_frozen_commit(self):
        script = shell(RELEASE, 'Validate frozen source')
        self.assertEqual(run(script, self.root, {'SOURCE_REF': 'a' * 40}).returncode, 0)
        for value in ['', 'main', 'refs/pull/1/head', 'v1.2.3', 'a' * 39,
                      'a' * 41, '$(touch injected)', 'a' * 40 + '\nmain']:
            self.assertNotEqual(run(script, self.root, {'SOURCE_REF': value}).returncode, 0)
        self.assertFalse((self.root / 'injected').exists())
        self.assertIn("if: github.event_name == 'push' || github.event_name == 'workflow_dispatch'",
                      job(RELEASE.read_text(), 'linux_x64'))

    def test_release_source_requires_merged_ancestry_and_available_evidence(self):
        directory = self.root / 'bin'
        directory.mkdir()
        gh = directory / 'gh'
        gh.write_text('''#!/usr/bin/env python3
import os,sys
if os.environ.get('API_FAIL') == 'true': sys.exit(1)
route = sys.argv[2]
if '/commits/' in route: print('a' * 40)
elif '/compare/main...' in route: print(os.environ['MAIN_STATE'])
elif '/compare/dev...' in route: print(os.environ['DEV_STATE'])
else: sys.exit(2)
''')
        gh.chmod(0o755)
        script = shell(RELEASE, 'Authorize merged release source')
        output = self.root / 'authorized-output'
        env = {'PATH': str(directory) + os.pathsep + os.environ['PATH'],
               'SOURCE_REF': 'v1.2.3', 'GH_REPO': 'fixture/repo',
               'GITHUB_OUTPUT': str(output)}
        for main, dev in [('identical', 'behind'), ('ahead', 'identical'),
                          ('behind', 'diverged')]:
            result = run(script, self.root, dict(env, MAIN_STATE=main, DEV_STATE=dev))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('commit=' + 'a' * 40, output.read_text())
            output.unlink()
        for extra in [{'MAIN_STATE': 'ahead', 'DEV_STATE': 'diverged'},
                      {'MAIN_STATE': 'unknown', 'DEV_STATE': 'unknown'},
                      {'MAIN_STATE': 'identical', 'DEV_STATE': 'identical', 'API_FAIL': 'true'},
                      {'MAIN_STATE': 'identical', 'DEV_STATE': 'identical', 'SOURCE_REF': 'main'}]:
            self.assertNotEqual(run(script, self.root, dict(env, **extra)).returncode, 0)
            self.assertFalse(output.exists())

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
        script = shell(RELEASE, 'Validate target and runner')
        for target, runner in TARGETS.values():
            self.assertEqual(run(script, self.root, {'TARGET': target, 'BUILD_RUNNER': runner}).returncode, 0)
        for target, runner in [('x86_64-apple-darwin', 'ubuntu-latest'), ('$(touch injected)', 'ubuntu-latest')]:
            self.assertNotEqual(run(script, self.root, {'TARGET': target, 'BUILD_RUNNER': runner}).returncode, 0)
        self.assertFalse((self.root / 'injected').exists())

    def test_annotated_tag_checkout_preserves_ref_for_exact_release_verification(self):
        source = self.root / 'source'
        checkout = self.root / 'checkout'
        source.mkdir()
        checkout.mkdir()
        env = {'PATH': os.environ['PATH'], 'HOME': str(self.root)}
        options = ['git', '-c', 'init.templateDir=', '-c', 'core.hooksPath=/dev/null',
                   '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid']
        def git(directory, *args):
            return subprocess.check_output(options + list(args), cwd=directory,
                                           env=env, text=True, stderr=subprocess.PIPE).strip()
        git(source, 'init', '-q')
        (source / 'Cargo.toml').write_text('[package]\nversion = "1.2.3"\n')
        git(source, 'add', 'Cargo.toml')
        git(source, 'commit', '-qm', 'fixture')
        git(source, 'tag', '-a', 'v1.2.3', '-m', 'fixture release')
        commit = git(source, 'rev-parse', 'HEAD')
        git(checkout, 'init', '-q')
        git(checkout, 'fetch', '--no-tags', '--depth=1', str(source), commit)
        git(checkout, 'checkout', '--detach', '-q', 'FETCH_HEAD')
        verification = shell(RELEASE, 'Check release tag matches Cargo version')
        self.assertNotEqual(run(verification, checkout, {'RELEASE_TAG': 'v1.2.3'}).returncode, 0)
        git(checkout, 'fetch', '--no-tags', '--depth=1', str(source),
            '+refs/tags/v1.2.3:refs/tags/v1.2.3')
        result = run(verification, checkout, {'RELEASE_TAG': 'v1.2.3'})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(commit, result.stdout)

    def test_actual_packaging_generates_digest_and_run_bound_cache_key(self):
        target = TARGETS['macos_arm64'][0]
        binary = self.root / 'target' / target / 'release/hibiscus'
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b'not-executed-fixture')
        binary.chmod(0o755)
        output = self.root / 'outputs'
        result = run(shell(RELEASE, 'Package binary and receipt'), self.root,
                     {'TARGET': target, 'GITHUB_OUTPUT': str(output), 'GITHUB_RUN_ID': '123'})
        self.assertEqual(result.returncode, 0, result.stderr)
        archive = self.root / 'dist' / ('hibiscus-' + target + '.tar.gz')
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        self.assertIn('sha256=' + digest, output.read_text())
        self.assertIn('cache_key=release-123-' + target + '-' + digest, output.read_text())
        with tarfile.open(archive) as stream:
            self.assertEqual(stream.getnames(), ['hibiscus'])
            self.assertEqual(stream.extractfile('hibiscus').read(), binary.read_bytes())

    def publisher(self, direct=False, exists=True, draft=True, tag='v1.2.3', rebuild=False):
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
               'RELEASE_TAG': tag, 'DIRECT_UPLOAD': str(direct).lower(), 'REBUILD': str(rebuild).lower(),
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
                    self.assertIn('--draft', calls[-1])
                self.assertEqual(len((self.root / 'dist/SHA256SUMS').read_text().splitlines()), 4)

    def test_normal_rebuild_also_refuses_an_already_published_release(self):
        result, calls = self.publisher(direct=False, exists=True, draft=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(all(x[:2] == ['release', 'view'] for x in calls))

    def test_provenance_receipt_rejects_tampering_missing_extra_and_symlink(self):
        folder = self.root / 'provenance'
        folder.mkdir()
        bundle = folder / 'hibiscus-provenance.sigstore.json'
        bundle.write_bytes(b'verified-fixture')
        digest = hashlib.sha256(bundle.read_bytes()).hexdigest()
        script = shell(RELEASE, 'Verify attestor receipt')
        env = {'PROVENANCE_SHA256': digest}
        self.assertEqual(run(script, self.root, env).returncode, 0)
        bundle.write_bytes(b'tampered')
        self.assertNotEqual(run(script, self.root, env).returncode, 0)
        bundle.unlink()
        self.assertNotEqual(run(script, self.root, env).returncode, 0)
        original = self.root / 'original-bundle'
        original.write_bytes(b'verified-fixture')
        bundle.symlink_to(original)
        self.assertNotEqual(run(script, self.root, env).returncode, 0)
        bundle.unlink()
        bundle.write_bytes(original.read_bytes())
        extra = folder / '.hidden'
        extra.write_bytes(b'extra')
        self.assertNotEqual(run(script, self.root, env).returncode, 0)
        extra.unlink()
        for malformed in ['', 'z' * 64, 'a' * 63, 'a' * 64 + '\nfile']:
            self.assertNotEqual(run(script, self.root, {'PROVENANCE_SHA256': malformed}).returncode, 0)

    def test_signer_context_rejects_untrusted_workflows_before_checkout(self):
        script = shell(RELEASE, 'Authorize signer workflow')
        env = {'GITHUB_REPOSITORY': 'Rekabytes-Enterprise/hibiscus', 'RELEASE_TAG': 'v1.2.3',
               'GITHUB_WORKFLOW_SHA': 'b' * 40, 'GITHUB_SHA': 'b' * 40,
               'SOURCE_COMMIT': 'a' * 40, 'GITHUB_EVENT_NAME': 'workflow_dispatch',
               'GITHUB_REF': 'refs/heads/dev',
               'GITHUB_WORKFLOW_REF': 'Rekabytes-Enterprise/hibiscus/.github/workflows/release.yml@refs/heads/dev'}
        self.assertEqual(run(script, self.root, env).returncode, 0)
        for change in [{'GITHUB_EVENT_NAME': 'pull_request'}, {'GITHUB_REF': 'refs/heads/feature'},
                       {'GITHUB_REPOSITORY': 'attacker/hibiscus'}, {'GITHUB_WORKFLOW_SHA': 'dev'},
                       {'GITHUB_SHA': 'c' * 40}, {'GITHUB_WORKFLOW_REF': 'other/workflow'}]:
            self.assertNotEqual(run(script, self.root, dict(env, **change)).returncode, 0)
        env.update(GITHUB_EVENT_NAME='push', GITHUB_REF='refs/tags/v1.2.3',
                   GITHUB_WORKFLOW_REF='Rekabytes-Enterprise/hibiscus/.github/workflows/release.yml@refs/tags/v1.2.3')
        self.assertNotEqual(run(script, self.root, env).returncode, 0)
        env['SOURCE_COMMIT'] = 'b' * 40
        self.assertEqual(run(script, self.root, env).returncode, 0)

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

    def test_artifact_rebuild_requires_existing_draft_before_any_write(self):
        for exists, draft in [(True, False), (False, False), (False, True)]:
            (self.root / 'calls.jsonl').unlink(missing_ok=True)
            result, calls = self.publisher(rebuild=True, exists=exists, draft=draft)
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(all(x[:2] == ['release', 'view'] for x in calls))
        result, calls = self.publisher(rebuild=True, exists=True, draft=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[-1][:2], ['release', 'upload'])

    def test_empty_or_invalid_tag_never_contacts_github(self):
        for tag in ['', 'v1.2.3;touch injected', 'v01.2.3']:
            result, calls = self.publisher(tag=tag)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(calls, [])
        self.assertFalse((self.root / 'injected').exists())


class SourceSnapshot(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='hibiscus-source-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / 'source'
        self.source.mkdir()
        self.workspace = self.root / 'consumer'
        self.workspace.mkdir()
        self.temp = self.root / 'runner'
        self.archive = self.temp / 'verified-source/source.tar'
        self.archive.parent.mkdir(parents=True)
        lines = RELEASE.read_text().splitlines()
        start = lines.index('  HIBISCUS_SOURCE_UNPACK_SCRIPT: |') + 1
        end = lines.index('jobs:')
        self.unpack = textwrap.dedent('\n'.join(lines[start:end]))

    def git(self, *args):
        return subprocess.check_output(
            ['git', '-c', 'init.templateDir=', '-c', 'core.hooksPath=/dev/null',
             '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid'] + list(args),
            cwd=self.source, env={'PATH': os.environ['PATH'], 'HOME': str(self.root)},
            text=True, stderr=subprocess.PIPE).strip()

    def freeze(self):
        self.git('init', '-q')
        (self.source / 'Cargo.toml').write_text('[package]\nversion = "1.2.3"\n')
        executable = self.source / 'script.sh'
        executable.write_text('#!/bin/sh\nexit 0\n')
        executable.chmod(0o755)
        (self.source / 'literal').write_text('$Format:%H$\n')
        (self.source / '.gitattributes').write_text(
            'Cargo.toml export-ignore\nliteral export-subst\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'fixture')
        return self.git('rev-parse', 'HEAD')

    def package(self, commit):
        output = self.root / 'outputs'
        result = run(shell(RELEASE, 'Package authorized source snapshot'), self.source,
                     {'SOURCE_COMMIT': commit, 'RUNNER_TEMP': str(self.temp),
                      'GITHUB_OUTPUT': str(output), 'GITHUB_RUN_ID': '123',
                      'GITHUB_RUN_ATTEMPT': '2'})
        if result.returncode == 0:
            transport = self.source / 'release-source-transport'
            transport.rename(self.workspace / 'release-source-transport')
            staged = run(shell(RELEASE, 'Stage source archive outside workspace'),
                         self.workspace, {'RUNNER_TEMP': str(self.temp)})
            self.assertEqual(staged.returncode, 0, staged.stderr)
        return result, output

    def restore(self, digest=None):
        expected = digest if digest is not None else hashlib.sha256(self.archive.read_bytes()).hexdigest()
        return run('python3 -c "$SCRIPT"', self.root,
                   {'SCRIPT': self.unpack, 'RUNNER_TEMP': str(self.temp),
                    'GITHUB_WORKSPACE': str(self.workspace), 'SOURCE_SHA256': expected})

    def write_archive(self, entries):
        with tarfile.open(self.archive, 'w') as stream:
            for name, kind, data in entries:
                info = tarfile.TarInfo(name)
                info.type = kind
                if kind in [tarfile.SYMTYPE, tarfile.LNKTYPE]:
                    info.linkname = '../outside'
                if kind == tarfile.REGTYPE:
                    info.size = len(data)
                    stream.addfile(info, io.BytesIO(data))
                else:
                    stream.addfile(info)

    def test_committed_bytes_and_executable_modes_survive_export_attributes(self):
        commit = self.freeze()
        # Never build modified working-tree files or metadata needed only by a newer tag.
        (self.source / 'Cargo.toml').write_text('uncommitted')
        (self.source / 'untracked').write_text('untracked')
        result, output = self.package(commit)
        self.assertEqual(result.returncode, 0, result.stderr)
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.assertIn('sha256=' + digest, output.read_text())
        self.assertIn('cache_key=source-123-2-' + commit + '-' + digest, output.read_text())
        result = self.restore()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.workspace / 'Cargo.toml').read_text(), '[package]\nversion = "1.2.3"\n')
        self.assertEqual((self.workspace / 'literal').read_text(), '$Format:%H$\n')
        self.assertEqual((self.workspace / 'script.sh').stat().st_mode & 0o777, 0o755)
        self.assertEqual((self.workspace / 'Cargo.toml').stat().st_mode & 0o777, 0o644)
        self.assertFalse((self.workspace / '.git').exists())
        self.assertFalse((self.workspace / 'untracked').exists())

    def test_tracked_symlinks_are_rejected_before_creating_a_receipt(self):
        self.freeze()
        (self.source / 'link').symlink_to('Cargo.toml')
        self.git('add', 'link')
        self.git('commit', '-qm', 'symlink')
        result, output = self.package(self.git('rev-parse', 'HEAD'))
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(output.exists())

    def test_invalid_source_identity_is_rejected(self):
        self.freeze()
        for value in ['main', 'v1.2.3', 'a' * 39, 'a' * 40 + '\nmain', '$(touch injected)']:
            result, output = self.package(value)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(output.exists())
        self.assertFalse((self.source / 'injected').exists())

    def test_tampered_archive_and_invalid_receipts_are_rejected(self):
        self.write_archive([('file', tarfile.REGTYPE, b'original')])
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.write_archive([('file', tarfile.REGTYPE, b'tampered')])
        for expected in [digest, '', 'z' * 64, 'a' * 63, 'a' * 64 + '\nfile']:
            self.assertNotEqual(self.restore(expected).returncode, 0)
            self.assertEqual(list(self.workspace.iterdir()), [])

    def test_all_members_are_validated_before_any_files_are_written(self):
        unsafe = [('/absolute', tarfile.REGTYPE), ('../outside', tarfile.REGTYPE),
                  ('dir/../outside', tarfile.REGTYPE), ('dir//file', tarfile.REGTYPE),
                  ('.git/config', tarfile.REGTYPE), ('back\\slash', tarfile.REGTYPE),
                  ('link', tarfile.SYMTYPE), ('hardlink', tarfile.LNKTYPE),
                  ('fifo', tarfile.FIFOTYPE), ('device', tarfile.CHRTYPE),
                  ('valid', tarfile.REGTYPE), ('valid/child', tarfile.REGTYPE)]
        for name, kind in unsafe:
            with self.subTest(name=name, kind=kind):
                self.write_archive([('valid', tarfile.REGTYPE, b'first'), (name, kind, b'bad')])
                self.assertNotEqual(self.restore().returncode, 0)
                self.assertEqual(list(self.workspace.iterdir()), [])
        self.assertFalse((self.root / 'outside').exists())

    def test_source_staging_rejects_extra_files_and_symlinks(self):
        transport = self.workspace / 'release-source-transport'
        transport.mkdir()
        archive = transport / 'source.tar'
        extra = transport / 'extra'
        archive.write_bytes(b'fixture')
        extra.write_bytes(b'extra')
        script = shell(RELEASE, 'Stage source archive outside workspace')
        self.assertNotEqual(run(script, self.workspace, {'RUNNER_TEMP': str(self.temp)}).returncode, 0)
        self.assertTrue(extra.exists())
        extra.unlink()
        outside = self.root / 'outside.tar'
        outside.write_bytes(b'fixture')
        archive.symlink_to(outside)
        self.assertNotEqual(run(script, self.workspace, {'RUNNER_TEMP': str(self.temp)}).returncode, 0)
        self.assertEqual(outside.read_bytes(), b'fixture')

    def test_archive_member_and_path_bounds_are_enforced(self):
        self.write_archive([('x' * 1025, tarfile.REGTYPE, b'')])
        self.assertNotEqual(self.restore().returncode, 0)
        self.assertEqual(list(self.workspace.iterdir()), [])
        self.write_archive([('large', tarfile.REGTYPE, b'x' * (16 * 1024 * 1024 + 1))])
        self.assertNotEqual(self.restore().returncode, 0)
        self.assertEqual(list(self.workspace.iterdir()), [])
        self.write_archive([(str(i), tarfile.REGTYPE, b'') for i in range(10001)])
        self.assertNotEqual(self.restore().returncode, 0)
        self.assertEqual(list(self.workspace.iterdir()), [])
        with self.archive.open('wb') as output:
            output.truncate(64 * 1024 * 1024 + 1)
        self.assertNotEqual(self.restore('a' * 64).returncode, 0)
        self.assertEqual(list(self.workspace.iterdir()), [])

    def test_symlink_archive_nonempty_workspace_and_empty_archive_are_rejected(self):
        self.write_archive([])
        self.assertNotEqual(self.restore().returncode, 0)
        self.write_archive([('file', tarfile.REGTYPE, b'valid')])
        (self.workspace / 'keep').write_text('existing')
        self.assertNotEqual(self.restore().returncode, 0)
        self.assertEqual((self.workspace / 'keep').read_text(), 'existing')
        (self.workspace / 'keep').unlink()
        actual = self.archive.with_name('actual.tar')
        self.archive.rename(actual)
        self.archive.symlink_to(actual)
        self.assertNotEqual(self.restore().returncode, 0)
        self.assertEqual(list(self.workspace.iterdir()), [])


if __name__ == '__main__':
    unittest.main(verbosity=2)
