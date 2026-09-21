"""Paired release measurements of one request freshness observation versus two.

Uses the exact archived pre-change service, verified against its original capture.
All other production sources are copied unchanged. No sibling/index modifications.
"""
import argparse
import json
from pathlib import Path
import random
import shutil
import statistics
import tempfile
import time

from markdown_context import build, inventory
from markdown_statistics import digest, indexes, write
from native_review import ROOT
from retrieval_phases import normalized
from taskbench.backends import GraphSearch
from taskbench.provenance import repository_snapshot


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    reference = ROOT / 'research/results/native-implementation/retrieval-phases'
    old_service = reference / 'service-before.rs'
    old_hash = json.loads((reference / 'build.json').read_text())['sources']['crates/graph-search/src/service.rs']
    assert digest(old_service) == old_hash
    queries = json.loads((reference / 'queries.json').read_text())
    write(out / 'queries.json', queries)
    before = inventory()
    roots = {name: ROOT.parent / name for name in ('nanus', 'blogwright', 'whatsurvey')}
    siblings = {name: repository_snapshot(root) for name, root in roots.items()}
    index_before = {name: indexes(root) for name, root in roots.items()}
    raw = Path(tempfile.mkdtemp(prefix='graph-search-inspection-review-'))
    print('Temporary builds:', raw, flush=True)
    control = raw / 'control'
    for name in before:
        if name.startswith(('crates/', '.cargo/', 'evaluation/harness/')) or name in ('Cargo.toml', 'Cargo.lock'):
            dest = control / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, dest)
    shutil.copy2(old_service, control / 'crates/graph-search/src/service.rs')
    binaries = {arm: raw / f'{arm}-host' for arm in ('single', 'repeated')}
    for arm, root in [('single', ROOT), ('repeated', control)]:
        build(root, ROOT / 'evaluation/harness/target', binaries[arm])
    hashes = {arm: digest(binary) for arm, binary in binaries.items()}
    write(out / 'build.json', {'sources': before, 'binaries': hashes, 'temporary_root': str(raw), 'control_service_sha256': old_hash, 'queries_sha256': digest(reference / 'queries.json')})
    rows = []
    for repo_number, (repo, root) in enumerate(roots.items()):
        backends = {}
        try:
            for arm in binaries:
                backends[arm] = GraphSearch(root, binaries[arm])
            selected = [q for q in queries if q['repo'] == repo]
            for query in selected:
                results = []
                for backend in backends.values():
                    backend.retrieval = {'ranking': query['ranking']}
                    results.append(normalized(json.loads(backend.search(query['query']))))
                assert results[0] == results[1], query
            for repeat in range(3):
                ordered = selected.copy()
                random.Random(1000 + repeat).shuffle(ordered)
                for position, query in enumerate(ordered):
                    arms = list(binaries)
                    if (repo_number + repeat + position) % 2:
                        arms.reverse()
                    for arm in arms:
                        backend = backends[arm]
                        backend.retrieval = {'ranking': query['ranking']}
                        started = time.perf_counter_ns()
                        value = json.loads(backend.search(query['query']))
                        elapsed = time.perf_counter_ns() - started
                        rows.append({'id': query['id'], 'repo': repo, 'family': query['family'], 'arm': arm, 'repeat': repeat, 'external_ns': elapsed, 'result_sha256': normalized(value), 'stats': value['stats']})
                print(repo, 'repeat', repeat, 'complete', flush=True)
            write(out / 'results.json', rows)
        finally:
            for backend in backends.values():
                backend.close()
    checks = {'sources': before == inventory(), 'binaries': hashes == {arm: digest(binary) for arm, binary in binaries.items()}, 'siblings': siblings == {name: repository_snapshot(root) for name, root in roots.items()}, 'indexes': index_before == {name: indexes(root) for name, root in roots.items()}, 'control': digest(old_service) == old_hash, 'queries': digest(reference / 'queries.json') == json.loads((out / 'build.json').read_text())['queries_sha256']}
    write(out / 'stability.json', {'checks': checks, 'siblings_before': siblings, 'indexes_before': index_before})
    assert all(checks.values()), checks
    comparisons = []
    for query in queries:
        group = [r for r in rows if r['id'] == query['id']]
        assert len(group) == 6
        assert len({r['result_sha256'] for r in group}) == 1, query
        timings = {arm: statistics.median(r['external_ns'] for r in group if r['arm'] == arm) for arm in binaries}
        comparisons.append(dict(query, median_external_ns=timings, single_repeated_ratio=timings['single'] / timings['repeated']))
    write(out / 'comparison.json', comparisons)
    write(out / 'checks.json', {'queries': len(queries), 'trials': len(rows), 'all_normalized_results_equal': True, 'stability': checks})


if __name__ == '__main__':
    main()
