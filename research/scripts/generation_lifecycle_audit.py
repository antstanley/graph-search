#!/usr/bin/env python3
"""Verify artifact applicability for the documented recommendation-28 decision.

The requirement mapping is a reviewed document, not inferred from passing
assertions here. This verifier checks source identity and captured experiment
claims without reclassifying missing measurements as completed benchmarks.
"""
import hashlib
import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[2]
BASE = ROOT / 'research/results/native-implementation'
OUT = BASE / 'generation-lifecycle-audit'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read(path):
    return json.loads(path.read_text())


def main():
    source_record = read(BASE / 'embedded-script-coordinates/sources.json')
    current = source_record['current_crate_sha256']
    assert len(current) == 143
    for name, expected in current.items():
        assert digest(ROOT / name) == expected, name
    actual = {str(p.relative_to(ROOT)) for p in (ROOT / 'crates').rglob('*')
              if p.is_file()}
    assert set(current) == actual, (set(current) - actual, actual - set(current))
    checks = read(BASE / 'embedded-script-coordinates/checks.json')
    assert checks['workspace_exit'] == checks['clippy_exit'] == 0
    workspace = (BASE / 'embedded-script-coordinates/workspace.txt').read_text()
    suites = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed;', workspace)
    assert len(suites) == 39 and sum(int(p) for p, _ in suites) == 484
    assert all(int(f) == 0 for _, f in suites)
    assert 'Finished' in (BASE / 'embedded-script-coordinates/clippy.txt').read_text()
    certificates = {}
    for name, expected_checks in [('generation-churn', 354), ('generation-memory', 42)]:
        cert = read(BASE / name / 'validation.json')
        assert cert['source_unchanged'] and cert['runs'] == 24
        assert cert['reader_checks'] == expected_checks
        runs = read(BASE / name / 'runs.json')
        assert len(runs) == 24
        assert sum(r['reader_checks'] for r in runs) == expected_checks
        assert {(r['files'], r['arm'], r['repeat']) for r in runs} == {
            (f, a, n) for f in (64, 256) for a in ('none', 'one', 'distinct', 'shared') for n in range(3)}
        env = read(BASE / name / 'environment.json')
        for path, expected in env['sources'].items():
            assert digest(ROOT / path) == expected, path
        if name == 'generation-churn':
            assert cert['rebuild_checks'] == sum(r['rebuild_checks'] for r in runs) == 168
            for run in runs:
                assert len(run['rows'][-1]['generations']) == 2
        else:
            assert all(r['final_generations'] == 2 for r in runs)
        certificates[name] = cert
    evidence = [ROOT / 'research/09-native-search-review.md', ROOT / 'SPEC.md',
                pathlib.Path(__file__), OUT / 'README.md',
                BASE / 'metadata-deltas/README.md', BASE / 'reader-retention/README.md']
    for name in ('embedded-script-coordinates', 'generation-churn', 'generation-memory'):
        evidence.extend(p for p in (BASE / name).iterdir() if p.is_file())
    (OUT / 'audit.json').write_text(json.dumps(dict(
        crate_files_verified=len(current), workspace_passed=484, workspace_suites=39,
        workspace_failed=0, source_matched_strict_clippy=True,
        experiment_certificates=certificates,
        evidence_sha256={str(p.relative_to(ROOT)): digest(p) for p in sorted(set(evidence))},
        crate_sha256=current), indent=2) + '\n')
    print('Verified 143 current crate files, 484 passed tests / 39 suites, strict Clippy, 48 experiment runs, and frozen inputs.')


if __name__ == '__main__':
    main()
