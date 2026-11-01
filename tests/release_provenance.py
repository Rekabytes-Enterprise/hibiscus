"""Offline release-policy controls; real cryptographic controls run separately."""
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('release_provenance_policy', ROOT / 'scripts/release_provenance.py')
policy = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = policy
spec.loader.exec_module(policy)


class ReleasePolicy(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='hibiscus-provenance-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.dist = self.root / 'dist'
        self.dist.mkdir()
        for name in policy.ARCHIVES:
            (self.dist / name).write_bytes(('unexecuted fixture: ' + name).encode())
        self.artifacts = policy.collect(directory=self.dist)
        self.release = policy.expected_release('v1.2.3', 'a' * 40)
        self.context = policy.build_context(self.release, 'c' * 64, 'push', '123', '1')
        self.bundle = self.root / 'bundle.sigstore.json'
        self.bundle.write_text('fixture only, not a signature')

    def output(self, context=None):
        return [{'verificationResult': {'statement': {
            '_type': 'https://in-toto.io/Statement/v1',
            'predicateType': 'https://slsa.dev/provenance/v1',
            'subject': [{'name': a.name, 'digest': {'sha256': a.sha256}}
                        for a in self.artifacts.values()],
            'predicate': policy.predicate(context or self.context)}}}]

    def check(self, value, expected=None):
        return policy.verified_statement(json.dumps(value).encode(), expected or self.release, self.artifacts)

    def test_tag_source_and_signer_are_bound_to_one_commit(self):
        value = policy.predicate(self.context)
        self.assertEqual(value['buildDefinition']['resolvedDependencies'][0], {
            'name': 'source', 'uri': 'git+https://github.com/Rekabytes-Enterprise/hibiscus@refs/tags/v1.2.3',
            'digest': {'gitCommit': 'a' * 40}})
        self.assertEqual(policy.context_from_predicate(value), self.context)
        self.check(self.output())

    def test_recovery_preserves_actual_old_source_and_new_workflow_identity(self):
        expected = policy.expected_release('v1.2.3', 'a' * 40, 'refs/heads/dev', 'b' * 40)
        context = policy.build_context(expected, 'c' * 64, 'workflow_dispatch', '456', '2')
        value = policy.predicate(context)
        deps = value['buildDefinition']['resolvedDependencies']
        self.assertEqual(deps[0]['digest']['gitCommit'], 'a' * 40)
        self.assertEqual(deps[1]['digest']['gitCommit'], 'b' * 40)
        self.check(self.output(context), expected)
        with self.assertRaises(policy.InvalidProvenance):
            self.check(self.output(context), policy.expected_release('v1.2.3', 'b' * 40, 'refs/heads/dev', 'b' * 40))

    def test_invalid_tags_commits_and_workflow_refs_fail_at_boundary(self):
        for value in ['', 'latest', 'v01.2.3', 'v1.2', 'v1.2.3/other', 'v1.2.3\nother']:
            with self.subTest(tag=value), self.assertRaises(policy.InvalidProvenance):
                policy.expected_release(value, 'a' * 40)
        for value in ['', 'main', 'a' * 39, 'A' * 40, 'a' * 40 + '\n']:
            with self.subTest(commit=value), self.assertRaises(policy.InvalidProvenance):
                policy.expected_release('v1.2.3', value)
        for ref in ['refs/heads/feature', 'refs/pull/1/merge', 'refs/tags/v1.2.4']:
            with self.subTest(ref=ref), self.assertRaises(policy.InvalidProvenance):
                policy.expected_release('v1.2.3', 'a' * 40, ref, 'b' * 40)
        with self.assertRaises(policy.InvalidProvenance):
            policy.expected_release('v1.2.3', 'a' * 40, 'refs/heads/dev')
        with self.assertRaises(policy.InvalidProvenance):
            policy.expected_release('v1.2.3', 'a' * 40, 'refs/tags/v1.2.3', 'b' * 40)

    def test_unsupported_events_and_run_ids_are_rejected(self):
        for event in ['pull_request', 'pull_request_target', 'workflow_call', 'workflow_dispatch']:
            with self.subTest(event=event), self.assertRaises(policy.InvalidProvenance):
                policy.build_context(self.release, 'c' * 64, event, '123', '1')
        for value in ['0', '-1', '01', '1\n2', 'x', '9' * 21]:
            with self.subTest(run=value), self.assertRaises(policy.InvalidProvenance):
                policy.build_context(self.release, 'c' * 64, 'push', value, '1')

    def environment(self):
        return {'GITHUB_REPOSITORY': policy.REPOSITORY, 'RELEASE_TAG': 'v1.2.3',
                'SOURCE_COMMIT': 'a' * 40, 'SOURCE_SHA256': 'c' * 64,
                'GITHUB_REF': 'refs/tags/v1.2.3', 'GITHUB_SHA': 'a' * 40,
                'GITHUB_WORKFLOW_SHA': 'a' * 40,
                'GITHUB_WORKFLOW_REF': policy.WORKFLOW + '@refs/tags/v1.2.3',
                'GITHUB_EVENT_NAME': 'push', 'GITHUB_RUN_ID': '123', 'GITHUB_RUN_ATTEMPT': '1'}

    def test_environment_requires_exact_repository_workflow_and_invocation(self):
        with mock.patch.dict(os.environ, self.environment(), clear=True):
            self.assertEqual(policy.context_from_env(), self.context)
        for change in [{'GITHUB_REPOSITORY': 'other/repo'}, {'GITHUB_WORKFLOW_REF': 'other/workflow'},
                       {'GITHUB_SHA': 'b' * 40}, {'GITHUB_EVENT_NAME': 'pull_request'}]:
            with self.subTest(change=change), mock.patch.dict(os.environ, dict(self.environment(), **change), clear=True):
                with self.assertRaises(policy.InvalidProvenance):
                    policy.context_from_env()

    def test_changed_tag_source_workflow_builder_and_snapshot_claims_are_rejected(self):
        for field in ['tag', 'source', 'workflow', 'builder', 'targets', 'uri', 'snapshot', 'run', 'extra']:
            value = self.output()
            predicate = value[0]['verificationResult']['statement']['predicate']
            definition = predicate['buildDefinition']
            if field == 'tag': definition['externalParameters']['releaseTag'] = 'v1.2.4'
            elif field == 'source': definition['resolvedDependencies'][0]['digest']['gitCommit'] = 'b' * 40
            elif field == 'workflow': definition['resolvedDependencies'][1]['digest']['gitCommit'] = 'b' * 40
            elif field == 'builder': predicate['runDetails']['builder']['id'] = 'https://github.com/attacker/workflow'
            elif field == 'targets': definition['externalParameters']['targets'].pop()
            elif field == 'uri': definition['resolvedDependencies'][0]['uri'] = 'git+https://github.com/other/repo'
            elif field == 'snapshot': predicate['runDetails']['byproducts'][0]['digest']['sha256'] = 'invalid'
            elif field == 'run': predicate['runDetails']['metadata']['invocationId'] += '/other'
            else: predicate['unreviewed'] = True
            with self.subTest(field=field), self.assertRaises(policy.InvalidProvenance):
                self.check(value)

    def test_modified_archive_bytes_fail_even_after_signature_verifier_success(self):
        value = self.output()
        path = next(iter(self.artifacts))
        path.write_bytes(b'changed')
        self.artifacts = policy.collect(directory=self.dist)
        with self.assertRaises(policy.InvalidProvenance):
            self.check(value)

    def test_ci_also_requires_exact_run_attempt_and_source_snapshot(self):
        for context in [policy.build_context(self.release, 'd' * 64, 'push', '123', '1'),
                        policy.build_context(self.release, 'c' * 64, 'push', '124', '1'),
                        policy.build_context(self.release, 'c' * 64, 'push', '123', '2')]:
            with self.subTest(context=context), self.assertRaises(policy.InvalidProvenance):
                self.check(self.output(context), self.context)

    def test_subject_count_names_duplicates_and_digest_algorithms_are_exact(self):
        for kind in ['missing', 'extra', 'duplicate', 'path', 'algorithm', 'digest', 'fields']:
            value = self.output()
            subjects = value[0]['verificationResult']['statement']['subject']
            if kind == 'missing': subjects.pop()
            elif kind == 'extra': subjects.append(copy.deepcopy(subjects[0]))
            elif kind == 'duplicate': subjects[1] = copy.deepcopy(subjects[0])
            elif kind == 'path': subjects[0]['name'] = '../' + subjects[0]['name']
            elif kind == 'algorithm': subjects[0]['digest'] = {'sha512': 'a' * 128}
            elif kind == 'digest': subjects[0]['digest']['sha256'] = 'z' * 64
            else: subjects[0]['extra'] = True
            with self.subTest(kind=kind), self.assertRaises(policy.InvalidProvenance):
                self.check(value)

    def test_no_unverified_or_ambiguous_json_can_become_a_valid_statement(self):
        for value in [[], self.output() * 2, {}, [{'verificationResult': {}}]]:
            with self.subTest(value=value), self.assertRaises(policy.InvalidProvenance):
                self.check(value)
        with self.assertRaises(policy.InvalidProvenance):
            policy.verified_statement(b'[{"verificationResult":{},"verificationResult":{}}]', self.release, self.artifacts)

    def test_failed_crypto_never_reaches_statement_policy(self):
        result = subprocess.CompletedProcess([], 1, json.dumps(self.output()).encode(), b'untrusted failure')
        with mock.patch.object(policy.subprocess, 'run', return_value=result), mock.patch.object(policy, 'verified_statement') as check:
            with self.assertRaisesRegex(policy.InvalidProvenance, 'Cryptographic'):
                policy.verify_release(self.artifacts, self.bundle, self.release, Path('gh'))
            check.assert_not_called()

    def test_verifier_arguments_bind_certificate_to_workflow_not_old_build_source(self):
        expected = policy.expected_release('v1.2.3', 'a' * 40, 'refs/heads/dev', 'b' * 40)
        context = policy.build_context(expected, 'c' * 64, 'workflow_dispatch', '123', '2')
        result = subprocess.CompletedProcess([], 0, json.dumps(self.output(context)).encode(), b'')
        with mock.patch.dict(os.environ, {'GH_TOKEN': 'SYNTHETIC-DO-NOT-INHERIT',
                                        'OPENAI_API_KEY': 'SYNTHETIC-DO-NOT-INHERIT',
                                        'ACTIONS_ID_TOKEN_REQUEST_TOKEN': 'SYNTHETIC-DO-NOT-INHERIT'}), \
             mock.patch.object(policy.subprocess, 'run', return_value=result) as run:
            policy.verify_release(self.artifacts, self.bundle, expected, Path('gh'))
        command = run.call_args[0][0]
        for flag in ['--source-digest', '--signer-digest']:
            self.assertEqual(command[command.index(flag) + 1], 'b' * 40)
        self.assertEqual(command[command.index('--cert-identity') + 1],
                         'https://github.com/Rekabytes-Enterprise/hibiscus/.github/workflows/release.yml@refs/heads/dev')
        self.assertIn('--deny-self-hosted-runners', command)
        self.assertIn('--cert-oidc-issuer', command)
        self.assertNotIn('--custom-trusted-root', command)
        self.assertNotIn('--signer-workflow', command)  # Mutually exclusive with exact cert identity.
        self.assertNotIn('GH_TOKEN', run.call_args[1]['env'])
        self.assertNotIn('OPENAI_API_KEY', run.call_args[1]['env'])
        self.assertNotIn('ACTIONS_ID_TOKEN_REQUEST_TOKEN', run.call_args[1]['env'])
        self.assertNotEqual(run.call_args[1]['env']['GH_CONFIG_DIR'], os.environ.get('GH_CONFIG_DIR'))

    def test_bundle_must_exist_and_be_regular_and_bounded(self):
        self.bundle.unlink()
        with self.assertRaises(OSError):
            policy.verify_release(self.artifacts, self.bundle, self.release, Path('gh'))
        target = self.root / 'target'
        target.write_bytes(b'fixture')
        self.bundle.symlink_to(target)
        with self.assertRaises(policy.InvalidProvenance):
            policy.verify_release(self.artifacts, self.bundle, self.release, Path('gh'))
        self.bundle.unlink()
        self.bundle.write_bytes(b'x' * (2 * 1024 * 1024 + 1))
        with self.assertRaises(policy.InvalidProvenance):
            policy.verify_release(self.artifacts, self.bundle, self.release, Path('gh'))

    def test_directory_cannot_hide_missing_extra_or_symlink_archives(self):
        extra = self.dist / '.hidden'
        extra.write_text('unexpected')
        with self.assertRaises(policy.InvalidProvenance): policy.collect(directory=self.dist)
        extra.unlink()
        path = next(iter(self.artifacts))
        path.unlink()
        with self.assertRaises(policy.InvalidProvenance): policy.collect(directory=self.dist)
        target = self.root / 'outside'
        target.write_bytes(b'fixture')
        path.symlink_to(target)
        with self.assertRaises(policy.InvalidProvenance): policy.collect(directory=self.dist)

    def test_prepare_cli_writes_only_validated_claims_without_overwriting(self):
        output = self.root / 'out/predicate.json'
        command = [sys.executable, '-B', str(ROOT / 'scripts/release_provenance.py'),
                   'prepare', '--directory', str(self.dist), '--output', str(output)]
        env = dict(self.environment(), PATH=os.environ['PATH'])
        result = subprocess.run(command, env=env, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(output.read_text()), policy.predicate(self.context))
        output.write_text('keep existing')
        self.assertNotEqual(subprocess.run(command, env=env, capture_output=True).returncode, 0)
        self.assertEqual(output.read_text(), 'keep existing')

    def test_untrusted_verifier_download_is_rejected_before_extraction_or_execution(self):
        directory = self.root / 'verifier'
        with mock.patch.object(policy.platform, 'system', return_value='Linux'), \
             mock.patch.object(policy.platform, 'machine', return_value='x86_64'), \
             mock.patch.object(policy.urllib.request, 'urlopen', return_value=io.BytesIO(b'untrusted')), \
             mock.patch.object(policy.subprocess, 'run') as run:
            with self.assertRaisesRegex(policy.InvalidProvenance, 'digest'):
                policy.install_verifier(directory)
            run.assert_not_called()
        self.assertFalse(directory.exists())


if __name__ == '__main__':
    unittest.main(verbosity=2)
