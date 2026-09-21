#!/usr/bin/env python3
"""Run read-only sibling workloads with native temporary stores and frozen provenance."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'evaluation'))
from taskbench.provenance import repository_snapshot


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def capture(host):
    roots = {name: ROOT.parent / name for name in ('nanus', 'blogwright', 'whatsurvey')}
    sources = list((ROOT / 'crates').rglob('*.rs'))
    sources += list((ROOT / 'research/harness/src').rglob('*.rs'))
    sources += [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'research/harness/Cargo.toml',
                ROOT / 'research/harness/Cargo.lock', Path(__file__),
                ROOT / 'evaluation/taskbench/provenance.py', ROOT / 'evaluation/taskbench/core.py']
    return {'production_and_driver_sha256': {str(p.relative_to(ROOT)): digest(p) for p in sorted(sources)},
            'binary_sha256': digest(host),
            'repositories': {name: repository_snapshot(root) for name, root in roots.items()},
            'codegraph_sha256': {name: {str(p.relative_to(root)): digest(p)
                                      for p in sorted((root / '.codegraph').rglob('*')) if p.is_file()}
                                for name, root in roots.items()}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    host = ROOT / 'research/harness/target/release/positional_probe'
    if not host.is_file():
        raise FileNotFoundError('build positional_probe --release --offline --locked first')
    before = capture(host)
    write(out / 'provenance-before.json', before)
    rows = {}
    errors = {}
    for name in ('nanus', 'blogwright', 'whatsurvey'):
        result = subprocess.run([str(host), str(ROOT.parent / name)], cwd=ROOT,
                                capture_output=True, text=True)
        (out / f'{name}.stderr').write_text(result.stderr)
        if result.returncode:
            errors[name] = {'exit_code': result.returncode, 'stderr': result.stderr}
        else:
            report = json.loads(result.stdout)
            write(out / f'{name}.json', report)
            rows[name] = [{'query': q['query'], 'mode': q['mode'], 'median_ms': q['median_ms'],
                          'files_scanned': q['stats'].get('files_scanned', 0),
                          'oracle_files': q['oracle_files'], 'returned_files': q['returned_files'],
                          'verified_evidence': q['verified_evidence'],
                          'truncations': q['truncations']} for q in report['queries']]
        print(name, 'ok' if not result.returncode else f'failed ({result.returncode})', flush=True)
    after = capture(host)
    write(out / 'provenance-after.json', after)
    write(out / 'summary.json', {'stable_provenance': before == after, 'errors': errors, 'results': rows,
                               'timing': 'release, in-process warmup then five repetitions per query; sequential repository workloads; no causal comparison',
                               'scope': 'explicit positional predicates, source verification and work; not conceptual relevance or model success'})
    if before != after:
        raise RuntimeError('source, driver, binary, repository or CodeGraph state changed during capture')
    if errors:
        raise RuntimeError('one or more native workloads failed; inspect retained diagnostics')

if __name__ == '__main__':
    main()
