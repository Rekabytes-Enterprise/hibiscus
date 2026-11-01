"""Real pinned-detector controls. All credential-shaped data stays temporary."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('secret_policy', ROOT / 'scripts/secret-scanning.py')
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)
SCANNER = None


class SecretScanning(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='hibiscus-secret-controls-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def cli(self, mode='dir', source=None, scanner=None, env=None):
        return subprocess.run([sys.executable, str(ROOT / 'scripts/secret-scanning.py'), mode,
                               '--source', str(source or self.root), '--scanner', str(scanner or SCANNER)],
                              capture_output=True, text=True, env=env, timeout=90)

    def dummy(self):
        # Not issued by any provider and never used in an authentication request.
        return 'unissued-' + hashlib.sha256(b'hibiscus-local-detector-control').hexdigest()

    def git(self, *args):
        env = dict(os.environ, GIT_AUTHOR_NAME='Fixture', GIT_COMMITTER_NAME='Fixture',
                   GIT_AUTHOR_EMAIL='fixture@example.invalid', GIT_COMMITTER_EMAIL='fixture@example.invalid')
        return subprocess.check_output(['git', '-c', 'init.templateDir=', '-c', 'core.hooksPath=/dev/null',
                                        *args], cwd=self.root, env=env, stderr=subprocess.PIPE).decode().strip()

    def test_native_provider_and_opaque_codex_credentials_are_detected_and_redacted(self):
        value = self.dummy()
        (self.root / 'auth.json').write_text(json.dumps({'custom-provider': value}))
        (self.root / 'codex-auth.json').write_text(json.dumps({'refresh': value, 'access': value}))
        result = self.cli()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn(value, result.stdout + result.stderr)
        rules = {f['RuleID'] for f in json.loads(result.stdout)['locations']}
        self.assertTrue({'hibiscus-saved-api-key', 'hibiscus-codex-credential'} <= rules, rules)

    def test_public_rfc7519_example_is_detected_without_an_account(self):
        # RFC 7519 section 3.1, expired 2011 example, not a service credential.
        parts = ['eyJ0eXAiOiJKV1QiLA0KICJhbGciOiJIUzI1NiJ9',
                 'eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ',
                 'dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk']
        token = '.'.join(parts)
        (self.root / 'standard-example.txt').write_text(token)
        result = self.cli()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn(token, result.stdout + result.stderr)
        self.assertIn('jwt', {f['RuleID'] for f in json.loads(result.stdout)['locations']})

    def test_noncredential_configuration_and_short_native_fixtures_are_preserved(self):
        (self.root / 'config.json').write_text(json.dumps({'model': self.dummy()}))
        (self.root / 'fixtures.rs').write_text('refresh: "old-refresh", access: "FAKE-ACCESS"')
        result = self.cli()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)['finding_count'], 0)

    def test_inline_suppression_and_environment_config_cannot_hide_a_match(self):
        (self.root / 'auth.json').write_text(json.dumps({'provider': self.dummy()}) + ' // gitleaks:allow')
        (self.root / 'ignore.toml').write_text('[allowlist]\npaths = [".*"]\n')
        env = dict(os.environ, GITLEAKS_CONFIG=str(self.root / 'ignore.toml'),
                   OPENAI_API_KEY='synthetic-inherited-value')
        result = self.cli(env=env)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn('synthetic-inherited-value', result.stdout + result.stderr)

    def test_deleted_secret_on_another_ref_is_still_detected(self):
        self.git('init', '-q')
        (self.root / 'readme').write_text('clean')
        self.git('add', 'readme'); self.git('commit', '-qm', 'clean')
        base = self.git('rev-parse', 'HEAD')
        (self.root / 'auth.json').write_text(json.dumps({'provider': self.dummy()}))
        self.git('add', 'auth.json'); self.git('commit', '-qm', 'temporary dummy')
        self.git('branch', 'other-ref')
        self.git('rm', '-q', 'auth.json'); self.git('commit', '-qm', 'removed dummy')
        self.git('checkout', '--detach', '-q', base)
        result = self.cli('git')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('hibiscus-saved-api-key', result.stdout)
        self.assertNotIn(self.dummy(), result.stdout + result.stderr)

    def test_merge_only_secret_is_detected(self):
        self.git('init', '-q')
        (self.root / 'readme').write_text('clean')
        self.git('add', 'readme'); self.git('commit', '-qm', 'base')
        base = self.git('rev-parse', 'HEAD')
        self.git('commit', '--allow-empty', '-qm', 'second clean parent')
        second = self.git('rev-parse', 'HEAD')
        (self.root / 'auth.json').write_text(json.dumps({'provider': self.dummy()}))
        self.git('add', 'auth.json')
        tree = self.git('write-tree')
        merge = self.git('commit-tree', tree, '-p', base, '-p', second, '-m', 'merge-only dummy')
        self.git('update-ref', 'refs/heads/merge-control', merge)
        result = self.cli('git')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(merge, result.stdout)

    def test_shallow_history_cannot_report_clean(self):
        self.git('init', '-q')
        (self.root / 'readme').write_text('clean')
        self.git('add', 'readme'); self.git('commit', '-qm', 'base')
        (self.root / '.git/shallow').write_text(self.git('rev-parse', 'HEAD') + '\n')
        result = self.cli('git')
        self.assertEqual(result.returncode, 2)
        self.assertNotIn('finding_count', result.stdout)

    def test_failed_tool_does_not_print_stderr_or_claim_clean(self):
        fake = self.root / 'failed-scanner'
        fake.write_text('#!' + sys.executable + '\nimport sys\n'
                        'if sys.argv[1:] == ["version"]: print("8.30.1")\n'
                        'else: print("PRIVATE-SENTINEL", file=sys.stderr); sys.exit(42)\n')
        fake.chmod(0o700)
        result = self.cli(scanner=fake)
        self.assertEqual(result.returncode, 2)
        self.assertNotIn('PRIVATE-SENTINEL', result.stdout + result.stderr)
        self.assertNotIn('finding_count', result.stdout)

    def test_bad_download_is_rejected_before_extraction(self):
        destination = self.root / 'scanner'
        with self.assertRaises(policy.ScanError):
            policy.unpack_verified(b'not an executable', '0' * 64, destination)
        self.assertFalse(destination.exists())


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--scanner', type=Path, required=True)
    args, rest = parser.parse_known_args()
    SCANNER = args.scanner.resolve()
    policy.verify_version(SCANNER)
    unittest.main(argv=[sys.argv[0], *rest], verbosity=2)
