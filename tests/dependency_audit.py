"""Audit policy, failed-fetch behavior and optional real RustSec positive control."""
import contextlib
import datetime as dt
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('dependency_audit', ROOT / 'scripts/audit-dependencies.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def report():
    return {'database': {'advisory-count': 10, 'last-commit': 'a' * 40,
                         'last-updated': '2026-01-01T00:00:00Z'},
            'lockfile': {'dependency-count': 2},
            'settings': {'ignore': [], 'severity': None, 'target_arch': [], 'target_os': []},
            'vulnerabilities': {'found': False, 'count': 0, 'list': []}, 'warnings': {}}


class AuditBoundary(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='hibiscus-audit-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.policy = self.root / 'exceptions.json'
        self.policy.write_text('{"version":1,"exceptions":[]}')
        self.output = self.root / 'report.json'
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.cargo = self.bin / 'cargo'
        self.cargo.write_text('''#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
with Path(os.environ['CALLS']).open('a') as f: f.write(json.dumps(sys.argv[1:])+'\\n')
if '--version' in sys.argv:
 print(os.environ['VERSION']);sys.exit(0)
print(os.environ['REPORT'])
sys.exit(int(os.environ['STATUS']))
''')
        self.cargo.chmod(0o755)
        self.calls = self.root / 'calls.jsonl'

    def invoke(self, data=None, status=0, version=None, raw=None):
        values = {'PATH': str(self.bin) + os.pathsep + os.environ['PATH'],
                  'HOME': str(self.root), 'CALLS': str(self.calls),
                  'VERSION': version or 'cargo-audit ' + audit.VERSION,
                  'REPORT': raw if raw is not None else json.dumps(data if data is not None else report()),
                  'STATUS': str(status)}
        with mock.patch.dict(os.environ, values, clear=True), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return audit.main(['--db', str(self.root / 'db'), '--report', str(self.output),
                               '--exceptions', str(self.policy), '--lockfile', str(self.root / 'Cargo.lock')])

    def entry(self, **updates):
        value = {'id': 'RUSTSEC-2020-0071', 'rationale': 'Reviewed temporary exception',
                 'owner': 'fixture-maintainer', 'expires': '2026-02-01'}
        value.update(updates)
        return value

    def test_clean_report_requires_fresh_fetch_command_and_private_output(self):
        self.assertEqual(self.invoke(), 0)
        calls = [json.loads(x) for x in self.calls.read_text().splitlines()]
        self.assertIn('https://github.com/RustSec/advisory-db.git', calls[1])
        self.assertNotIn('--no-fetch', calls[1])
        self.assertNotIn('--stale', calls[1])
        self.assertEqual(self.output.stat().st_mode & 0o777, 0o600)

    def test_vulnerability_is_a_failure_even_if_tool_exits_zero(self):
        value = report()
        value['vulnerabilities'] = {'found': True, 'count': 1, 'list': [{'advisory': {'id': 'RUSTSEC-2020-0071'}}]}
        self.assertEqual(self.invoke(value), 1)
        self.assertTrue(self.output.exists())

    def test_tool_failure_cannot_be_a_clean_scan(self):
        self.assertEqual(self.invoke(status=1), 1)

    def test_failed_fetch_removes_previous_clean_report(self):
        self.output.write_text(json.dumps(report()))
        self.assertEqual(self.invoke(status=1, raw=''), 1)
        self.assertFalse(self.output.exists())

    def test_wrong_tool_version_fails_before_audit(self):
        self.assertEqual(self.invoke(version='cargo-audit 0.1.0'), 1)
        self.assertEqual(len(self.calls.read_text().splitlines()), 1)

    def test_incomplete_database_and_report_fail(self):
        for value in [{}, [], {'database': None}, dict(report(), database={'advisory-count': 0}),
                      dict(report(), vulnerabilities={'found': False, 'count': 1, 'list': []})]:
            self.assertEqual(self.invoke(value), 1)

    def test_unexpected_implicit_filters_fail(self):
        for key, value in [('ignore', ['RUSTSEC-2020-0071']), ('severity', 'high'),
                           ('target_arch', ['x86_64']), ('target_os', ['linux'])]:
            data = report()
            data['settings'][key] = value
            self.assertEqual(self.invoke(data), 1)

    def test_warnings_need_an_explicit_advisory_exception(self):
        data = report()
        data['warnings'] = {'unmaintained': [{'advisory': {'id': 'RUSTSEC-2020-0071'}}]}
        self.assertEqual(self.invoke(data), 1)
        self.assertEqual(audit.validate_report(dict(data, settings=dict(data['settings'], ignore=['RUSTSEC-2020-0071'])), ['RUSTSEC-2020-0071']), ('a' * 40, 2))

    def test_policy_requires_specific_id_owner_reason_and_future_expiry(self):
        today = dt.date(2026, 1, 1)
        self.policy.write_text(json.dumps({'version': 1, 'exceptions': [self.entry()]}))
        self.assertEqual(audit.exceptions(self.policy, today), ['RUSTSEC-2020-0071'])
        for changes in [{'id': '*'}, {'owner': ''}, {'rationale': ' '}, {'expires': '2026-01-01'},
                        {'expires': '2025-12-31'}, {'expires': 'not-a-date'}, {'unknown': 'field'}]:
            self.policy.write_text(json.dumps({'version': 1, 'exceptions': [self.entry(**changes)]}))
            with self.assertRaises(ValueError):
                audit.exceptions(self.policy, today)

    def test_duplicate_or_blanket_exceptions_fail(self):
        for data in [{'version': 1, 'exceptions': [self.entry(), self.entry()]},
                     {'version': True, 'exceptions': []}, {'version': 1, 'exceptions': '*'}, {'version': 1, 'exceptions': [], 'ignore': '*'}]:
            self.policy.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                audit.exceptions(self.policy, dt.date(2026, 1, 1))

    def test_expired_policy_stops_before_tool_execution(self):
        self.policy.write_text(json.dumps({'version': 1, 'exceptions': [self.entry(expires='2000-01-01')]}))
        self.assertEqual(self.invoke(), 1)
        self.assertFalse(self.calls.exists())


class LiveAudit(unittest.TestCase):
    @staticmethod
    def lockfile(directory):
        path = Path(directory) / 'Cargo.lock'
        path.write_text('version = 3\n[[package]]\nname = "time"\nversion = "0.1.43"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n')
        return path

    @unittest.skipUnless(os.environ.get('HIBISCUS_AUDIT_TEST_DB'), 'requires fetched RustSec database and pinned auditor')
    def test_real_tool_detects_known_vulnerable_lockfile_without_building_it(self):
        with tempfile.TemporaryDirectory(prefix='hibiscus-audit-positive-') as directory:
            path = self.lockfile(directory)
            result = subprocess.run(['cargo', 'audit', '--json', '--no-fetch', '--db',
                                     os.environ['HIBISCUS_AUDIT_TEST_DB'], '--file', str(path)],
                                    capture_output=True, text=True, timeout=90)
            data = json.loads(result.stdout)
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(data['vulnerabilities']['found'])
            self.assertIn('RUSTSEC-2020-0071', [x['advisory']['id'] for x in data['vulnerabilities']['list']])

    @unittest.skipUnless(os.environ.get('HIBISCUS_AUDIT_TEST_DB'), 'requires fetched RustSec database and pinned auditor')
    def test_real_tool_honors_only_the_specific_reviewed_temporary_exception(self):
        with tempfile.TemporaryDirectory(prefix='hibiscus-audit-exception-') as directory:
            root = Path(directory)
            lock = self.lockfile(directory)
            policy = root / 'exceptions.json'
            expiry = (dt.datetime.now(dt.timezone.utc).date() + dt.timedelta(days=7)).isoformat()
            policy.write_text(json.dumps({'version': 1, 'exceptions': [
                {'id': 'RUSTSEC-2020-0071', 'owner': 'test-fixture',
                 'rationale': 'Temporary test only, crate never compiled', 'expires': expiry}]}))
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                result = audit.main(['--lockfile', str(lock), '--exceptions', str(policy),
                                     '--db', os.environ['HIBISCUS_AUDIT_TEST_DB'],
                                     '--report', str(root / 'report.json')])
            self.assertEqual(result, 0)
            self.assertEqual(json.loads((root / 'report.json').read_text())['settings']['ignore'], ['RUSTSEC-2020-0071'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
