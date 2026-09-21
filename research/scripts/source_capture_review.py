"""Strict source capture comparison. Read-only siblings; disposable native stores."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from markdown_context import inventory
from markdown_statistics import digest, indexes, write
from native_review import ROOT
from taskbench.provenance import repository_snapshot


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    roots = {name: ROOT.parent / name for name in ('nanus', 'blogwright', 'whatsurvey')}
    queries = {
        'nanus': ['bounded file reads', 'send', 'cache invalidation'],
        'blogwright': ['upsert secret', 'publish', 'deployment outputs'],
        'whatsurvey': ['storage envelope', 'contact policy', 'deployment outputs'],
    }
    write(out / 'queries.json', queries)
    before = inventory()
    names = subprocess.check_output(['git', 'ls-files', '--cached', '--others',
        '--exclude-standard', '-z', 'research/harness'], cwd=ROOT)
    before.update({name.decode(): digest(ROOT / name.decode()) for name in names.split(b'\0') if name})
    siblings = {name: repository_snapshot(root) for name, root in roots.items()}
    index_before = {name: indexes(root) for name, root in roots.items()}
    raw = Path(tempfile.mkdtemp(prefix='graph-search-source-capture-'))
    print('Temporary builds:', raw, flush=True)
    control = raw / 'control'
    for name in before:
        if name.startswith(('crates/', '.cargo/', 'research/harness/')) or name in ('Cargo.toml', 'Cargo.lock'):
            target = control / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, target)
    path = control / 'crates/graph-search/src/service.rs'
    source = path.read_text()
    old = '        work.enable_source_capture();'
    new = '        // Source capture disabled only in the experimental control.'
    assert source.count(old) == 1
    path.write_text(source.replace(old, new))
    delta = [str(path.relative_to(control)) for path in control.rglob('*')
             if path.is_file() and digest(path) != before[str(path.relative_to(control))]]
    assert delta == ['crates/graph-search/src/service.rs'], delta
    binaries = {}
    for arm, project in [('capture', ROOT), ('control', control)]:
        target = ROOT / 'research/harness/target'
        subprocess.run(['cargo', 'build', '--release', '--offline', '--locked',
            '--manifest-path', str(project / 'research/harness/Cargo.toml'),
            '--bin', 'source_capture_probe', '--target-dir', str(target)], cwd=project, check=True)
        binaries[arm] = raw / arm / 'probe'
        binaries[arm].parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(target / 'release/source_capture_probe', binaries[arm])
    hashes = {arm: digest(binary) for arm, binary in binaries.items()}
    write(out / 'build.json', {'sources': before, 'binaries': hashes,
        'intervention': {'path': delta[0], 'before': old, 'after': new},
        'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
        'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()})
    rows = []
    for i, (repo, root) in enumerate(roots.items()):
        query_file = raw / f'{repo}-queries.json'
        write(query_file, queries[repo])
        for arm in (list(binaries) if i % 2 == 0 else list(reversed(binaries))):
            result = subprocess.check_output([str(binaries[arm]), str(root), str(query_file)], text=True)
            rows.extend(dict(row, repo=repo, arm=arm) for row in json.loads(result))
        print(repo, 'complete', flush=True)
        write(out / 'results.json', rows)
    stable = {'production': all(digest(ROOT / name) == value for name, value in before.items()),
        'binaries': hashes == {arm: digest(binary) for arm, binary in binaries.items()},
        'siblings': siblings == {name: repository_snapshot(root) for name, root in roots.items()},
        'indexes': index_before == {name: indexes(root) for name, root in roots.items()}}
    write(out / 'stability.json', {'checks': stable, 'siblings_before': siblings, 'indexes_before': index_before})
    assert all(stable.values()), stable
    assert len(rows) == 54 and all(row['source_valid'] for row in rows)
    paired = []
    for repo, qs in queries.items():
        for query in qs:
            arms = {arm: [r for r in rows if r['repo'] == repo and r['query'] == query and r['arm'] == arm] for arm in binaries}
            for selected in arms.values():
                assert len(selected) == 3
                assert len({r['result_sha256_without_stats_generation'] for r in selected}) == 1
                assert len({(r['stats']['source_files_attempted'], r['stats']['source_bytes_read']) for r in selected}) == 1
            a, b = arms['capture'][0], arms['control'][0]
            paired.append({'repo': repo, 'query': query,
                'identical_result_without_stats_generation': a['result_sha256_without_stats_generation'] == b['result_sha256_without_stats_generation'],
                'identical_delivered_lines': a['delivered'] == b['delivered'],
                'capture_files': a['stats']['source_files_attempted'], 'control_files': b['stats']['source_files_attempted'],
                'capture_bytes': a['stats']['source_bytes_read'], 'control_bytes': b['stats']['source_bytes_read']})
    write(out / 'comparison.json', paired)
    write(out / 'checks.json', {'trials': len(rows), 'all_source_valid': True,
        'repeat_results_identical': True, 'repeat_source_work_identical': True, 'stability': stable})


if __name__ == '__main__':
    main()
