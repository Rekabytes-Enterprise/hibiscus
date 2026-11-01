"""Verify the real Rust extractor/query path using a disposable, harmless fixture.

This does not upload its synthetic finding or execute the fixture. Repository
security analysis is separate. A tool failure or absent finding fails the job.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    codeql = os.environ['CODEQL']

    def run(*arguments):
        subprocess.run([codeql, *arguments], check=True, timeout=600)

    with tempfile.TemporaryDirectory(prefix='hibiscus-codeql-',
                                     dir=os.environ['RUNNER_TEMP']) as temporary:
        root = Path(temporary)
        source = root / 'source'
        (source / 'src').mkdir(parents=True)
        (source / 'Cargo.toml').write_text(
            '[package]\nname = "hibiscus-codeql-probe"\nversion = "0.0.0"\n'
            'edition = "2021"\n', encoding='utf-8')
        (source / 'src/lib.rs').write_text(
            'pub fn probe(flag: bool) {\n'
            '    if flag {}\n'
            '    if flag { let _value = 1; }\n'
            '}\n', encoding='utf-8')
        pack = root / 'query'
        pack.mkdir()
        (pack / 'qlpack.yml').write_text(
            'name: hibiscus/extractor-probe\nversion: 0.0.0\n'
            'dependencies:\n  codeql/rust-all: "*"\n', encoding='utf-8')
        # The action supplies its linked, versioned bundle and library packs.
        # Resolve those packs rather than downloading the latest Rust library.
        libraries = subprocess.check_output(
            [codeql, 'resolve', 'qlpacks', '--format=json'], text=True, timeout=60)
        paths = json.loads(libraries).get('codeql/rust-all', [])
        if len(paths) != 1:
            raise RuntimeError('Expected one Rust library in the linked CodeQL bundle')
        library = Path(paths[0])
        query = pack / 'Probe.ql'
        query.write_text('''/**
 * @name Harmless Rust extraction probe
 * @description Detect one empty if branch in a disposable fixture.
 * @kind problem
 * @problem.severity warning
 * @id hibiscus/extractor-probe
 */
import rust

from IfExpr expression
where expression.getThen().(BlockExpr).getStmtList().getNumberOfStmtOrExpr() = 0
select expression, "Hibiscus extraction probe."
''', encoding='utf-8')
        database = root / 'database'
        run('database', 'create', str(database), '--language=rust', '--build-mode=none',
            '--source-root=' + str(source), '--threads=2', '--ram=2048')
        report = root / 'probe.sarif'
        run('database', 'analyze', str(database), str(query),
            '--additional-packs=' + str(library), '--format=sarif-latest',
            '--output=' + str(report), '--threads=2', '--ram=2048')
        results = [item for entry in json.loads(report.read_text())['runs']
                   for item in entry.get('results', [])]
        if len(results) != 1 or results[0].get('ruleId') != 'hibiscus/extractor-probe':
            raise RuntimeError('CodeQL did not report exactly the expected fixture finding')
        location = results[0]['locations'][0]['physicalLocation']
        if not location['artifactLocation']['uri'].endswith('src/lib.rs'):
            raise RuntimeError('Probe finding does not refer to the disposable Rust source')
        if location['region']['startLine'] != 2:
            raise RuntimeError('Probe did not distinguish the empty and nonempty branches')
        print('CodeQL Rust probe passed: one expected finding; nonempty control excluded.')


if __name__ == '__main__':
    main()
