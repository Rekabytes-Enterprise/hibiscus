#!/usr/bin/env python3
"""Run the pinned RustSec auditor with explicit, expiring advisory exceptions."""
import argparse
import datetime as dt
import json
from pathlib import Path
import re
import subprocess
import sys

VERSION = '0.22.2'
ROOT = Path(__file__).resolve().parents[1]


def exceptions(path, today):
    value = json.loads(path.read_text())
    if not isinstance(value, dict) or set(value) != {'version', 'exceptions'} or type(value['version']) is not int or value['version'] != 1:
        raise ValueError('Invalid advisory exception policy')
    if not isinstance(value['exceptions'], list):
        raise ValueError('Exceptions must be a list')
    ids = []
    for entry in value['exceptions']:
        if not isinstance(entry, dict) or set(entry) != {'id', 'rationale', 'owner', 'expires'}:
            raise ValueError('Each exception needs exactly id, rationale, owner and expires')
        if not all(isinstance(v, str) and v.strip() for v in entry.values()):
            raise ValueError('Exception fields must be nonempty strings')
        identifier = entry['id']
        if not re.fullmatch(r'RUSTSEC-[0-9]{4}-[0-9]{4}', identifier) or identifier in ids:
            raise ValueError('Exception advisory ID is invalid or duplicated')
        expiry = dt.date.fromisoformat(entry['expires'])
        if expiry.isoformat() != entry['expires'] or expiry <= today:
            raise ValueError('Exception has expired: ' + identifier)
        ids.append(identifier)
    return ids


def validate_report(report, ignored):
    if not isinstance(report, dict):
        raise ValueError('Audit report is not an object')
    database, lockfile = report['database'], report['lockfile']
    if (type(database['advisory-count']) is not int or database['advisory-count'] <= 0
            or not re.fullmatch(r'(?:[0-9a-f]{40}|[0-9a-f]{64})', database['last-commit'])
            or not isinstance(database['last-updated'], str)
            or type(lockfile['dependency-count']) is not int or lockfile['dependency-count'] <= 0):
        raise ValueError('Missing database or lockfile evidence')
    settings = report['settings']
    if (any(not isinstance(settings[k], list) for k in ['ignore', 'target_arch', 'target_os'])
            or settings['severity'] is not None or settings['target_arch'] or settings['target_os']
            or sorted(settings['ignore']) != sorted(ignored)):
        raise ValueError('Unexpected audit filters or ignored advisories')
    findings = report['vulnerabilities']
    if (type(findings['found']) is not bool or type(findings['count']) is not int
            or not isinstance(findings['list'], list)
            or findings['count'] != len(findings['list'])
            or findings['found'] != bool(findings['list'])):
        raise ValueError('Inconsistent vulnerability report')
    if findings['found']:
        raise ValueError('Unresolved vulnerable dependencies')
    warnings = report['warnings']
    if not isinstance(warnings, dict) or any(not isinstance(v, list) for v in warnings.values()):
        raise ValueError('Invalid audit warnings')
    for entries in warnings.values():
        for item in entries:
            if item['advisory']['id'] not in ignored:
                raise ValueError('Unreviewed advisory warning: ' + item['advisory']['id'])
    return database['last-commit'], lockfile['dependency-count']


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lockfile', type=Path, default=ROOT / 'Cargo.lock')
    parser.add_argument('--exceptions', type=Path, default=ROOT / '.github/security/advisory-exceptions.json')
    parser.add_argument('--db', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args(argv)
    # Remove old output before work, so a failed fetch cannot leave a clean report.
    args.report.unlink(missing_ok=True)
    try:
        ignored = exceptions(args.exceptions, dt.datetime.now(dt.timezone.utc).date())
        version = subprocess.run(['cargo', 'audit', '--version'], check=True,
                                 capture_output=True, text=True, timeout=30)
        if not re.search(r'\b' + re.escape(VERSION) + r'\s*$', version.stdout):
            raise ValueError('Install cargo-audit ' + VERSION + ' with --locked')
        command = ['cargo', 'audit', '--json', '--file', str(args.lockfile),
                   '--db', str(args.db), '--url', 'https://github.com/RustSec/advisory-db.git']
        for identifier in ignored:
            command += ['--ignore', identifier]
        result = subprocess.run(command, capture_output=True, text=True, timeout=300)
        if result.stderr:
            print(result.stderr, file=sys.stderr, end='')
        report = json.loads(result.stdout)
        # Preserve real findings for investigation, never call a nonzero scan clean.
        with args.report.open('x') as output:
            args.report.chmod(0o600)
            json.dump(report, output, indent=2)
            output.write('\n')
        if result.returncode != 0:
            raise ValueError('cargo-audit failed with status ' + str(result.returncode))
        commit, count = validate_report(report, ignored)
        print('Audited %s dependencies against RustSec %s; %s approved exceptions.' %
              (count, commit, len(ignored)))
        return 0
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        print('Dependency audit failed: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
