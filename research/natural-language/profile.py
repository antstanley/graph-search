"""Small sequential debug profile of the public Index; no model or gold-driven actions."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import sys
import time
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'evaluation'))
from taskbench.backends import GraphSearch
from taskbench.provenance import repository_snapshot
from experiment import BASE, ROOT


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--baseline', type=Path, default=BASE / 'private/baseline-task-eval-host')
    parser.add_argument('--current', type=Path, default=Path('/private/tmp/graph-search-natural-language-target/debug/task-eval-host'))
    args = parser.parse_args()
    binaries = {'baseline': args.baseline, 'current': args.current}
    result = {'scope': 'Sequential debug public Index; 3 exact explore prompts and 3 development task prompts per repo, each repeated 3 times; not a release benchmark',
              'binaries': {k: hashlib.sha256(p.read_bytes()).hexdigest() for k, p in binaries.items()}, 'repos': {}}
    tasks = json.loads((ROOT / 'evaluation/tasks.json').read_text())
    for position, repo in enumerate(['nanus', 'blogwright', 'whatsurvey']):
        root = Path.home() / 'code' / repo
        before = repository_snapshot(root)
        exact = [q['query'] for q in json.loads((ROOT / 'research/results' / f'{repo}-expanded-queries.json').read_text()) if q['category'] == 'exact'][:3]
        natural = [t['prompt'] for t in tasks if t['repo'] == repo and t['split'] == 'dev'][:3]
        result['repos'][repo] = {'before': before, 'arms': {}}
        # Reverse arm order for the middle repository to expose order assumptions.
        for arm in (['baseline', 'current'] if position != 1 else ['current', 'baseline']):
            backend = GraphSearch(root, binaries[arm])
            records = []
            try:
                for repeat in range(3):
                    for group, queries in [('exact-explore', exact), ('natural-explore', natural)]:
                        for query in queries:
                            start = time.perf_counter()
                            backend.search(query)
                            records.append({'group': group, 'query': query, 'repeat': repeat,
                                            'elapsed_ms': (time.perf_counter() - start) * 1000})
                sizes = {str(p.relative_to(backend.tmp.name)): p.stat().st_size
                         for p in Path(backend.tmp.name).rglob('*') if p.is_file()}
                result['repos'][repo]['arms'][arm] = {
                    'setup_ms': backend.setup_ms, 'live_store_bytes': sum(sizes.values()),
                    'store_files': sizes, 'records': records,
                    'median_ms': {g: statistics.median(r['elapsed_ms'] for r in records if r['group'] == g)
                                  for g in ['exact-explore', 'natural-explore']}}
            finally:
                backend.close()
        after = repository_snapshot(root)
        assert before == after, repo
        result['repos'][repo]['after'] = after
        print(repo, {a: {'latency': x['median_ms'], 'store': x['live_store_bytes']}
                     for a, x in result['repos'][repo]['arms'].items()}, flush=True)
    (BASE / 'results/public-index-profile.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
