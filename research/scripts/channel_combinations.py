"""Fixed-pool score-combination diagnostic; no production ranking changes."""
import hashlib
import json
import pathlib
import subprocess
import sys

from native_review import eligible_tasks

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "evaluation"))
from taskbench.core import validate
from taskbench.provenance import repository_snapshot

OUT = ROOT / "research/results/native-implementation/channel-combinations"


def write(name, value):
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def indexes(roots):
    return {name: {str(p.relative_to(root)): digest(p)
                   for p in sorted((root / '.codegraph').rglob('*')) if p.is_file()}
            for name, root in roots.items()}


def rank(row, policy):
    lanes = {lane: {hit['id']: hit for hit in row[lane]} for lane in ('metadata', 'body')}
    assert all(len(lanes[lane]) == len(row[lane]) for lane in lanes), 'duplicate entity in native lane'
    ids = set(lanes['metadata']) | set(lanes['body'])
    exact = {id for id, hit in lanes['metadata'].items() if hit['exact']}
    values = {}
    for lane, hits in lanes.items():
        scores = [hit['score'] for id, hit in hits.items() if id not in exact]
        high, low = max(scores, default=0), min(scores, default=0)
        values[lane] = {}
        for id, hit in hits.items():
            if id in exact:
                continue
            if policy.startswith('max-'):
                score = hit['score'] / high if high else 0
            elif policy.startswith('minmax-'):
                score = (hit['score'] - low) / (high - low) if high > low else 1
            else:
                score = 1 / (60 + hit['rank'])
            values[lane][id] = score
    if policy in ('body', 'metadata'):
        ids = set(lanes[policy]) | exact
        weights = {'body': int(policy == 'body'), 'metadata': int(policy == 'metadata')}
    else:
        body_weight = float(policy.split('-')[1]) if '-' in policy else .5
        weights = {'body': body_weight, 'metadata': 1 - body_weight}
    return sorted(ids, key=lambda id: (
        id not in exact,
        -sum(weights[lane] * values[lane].get(id, 0) for lane in lanes),
        (lanes['metadata'].get(id) or lanes['body'][id])['path'], id,
    ))


def self_check():
    row = {'metadata': [dict(id='exact', path='a', score=2, rank=1, exact=True),
                        dict(id='m', path='b', score=.2, rank=2, exact=False)],
           'body': [dict(id='b', path='c', score=100, rank=1, exact=False)]}
    for policy in policies():
        assert rank(row, policy)[0] == 'exact'
        assert rank({'metadata': [], 'body': []}, policy) == []
    assert rank(row, 'body') == ['exact', 'b']
    assert rank(row, 'metadata') == ['exact', 'm']
    shared = dict(id='m', path='b', score=.2, rank=1, exact=False)
    assert rank({'metadata': [shared], 'body': [shared]}, 'minmax-0.5') == ['m']


def policies():
    return ['body', 'metadata', 'rrf'] + [f'{normalization}-{weight}'
             for normalization in ('max', 'minmax') for weight in (.25, .5, .75)]


def main():
    self_check()
    if sys.argv[1] == 'capture':
        roots = {name: ROOT.parent / name for name in ('nanus', 'blogwright', 'whatsurvey')}
        tasks, oracles, excluded = eligible_tasks(roots)
        for suite in ('fresh-routing-2026-09-19', 'markdown-readme-2026-09-20'):
            directory = ROOT / 'research/fixtures' / suite
            freeze = json.loads((directory / 'freeze.json').read_text())
            for filename in ('tasks.json', 'oracles.json'):
                assert digest(directory / filename) == freeze['artifacts'][filename]
            extra = json.loads((directory / 'tasks.json').read_text())
            labels = json.loads((directory / 'oracles.json').read_text())
            validate(extra, labels, roots)
            tasks.extend(extra)
            oracles.update(labels)
        write('tasks.json', tasks)
        write('oracles.json', oracles)
        write('excluded.json', excluded)
        binary = ROOT / 'research/harness/target/release/channel_scores_probe'
        files = sorted(p for p in (ROOT / 'crates').rglob('*') if p.is_file())
        files += [pathlib.Path(__file__).resolve(), ROOT / 'research/harness/src/bin/channel_scores_probe.rs',
                  OUT / 'PROTOCOL.md', OUT / 'tasks.json', OUT / 'oracles.json', binary]
        before = {str(p.relative_to(ROOT)): digest(p) for p in files}
        siblings = {name: repository_snapshot(root) for name, root in roots.items()}
        index_before = indexes(roots)
        write('before.json', dict(inputs=before, siblings=siblings, indexes=index_before))
        rows = []
        for name, root in roots.items():
            selected = [task for task in tasks if task['repo'] == name]
            requests = OUT / (name + '-requests.json')
            requests.write_text(json.dumps(selected))
            value = json.loads(subprocess.check_output([str(binary), str(root), str(requests)]))
            assert {r['task'] for r in value} == {t['id'] for t in selected}
            rows.extend(value)
            write('scores.json', rows)
            print(name, len(value), 'captured', flush=True)
        after = {str(p.relative_to(ROOT)): digest(p) for p in files}
        assert before == after
        assert siblings == {name: repository_snapshot(root) for name, root in roots.items()}
        assert index_before == indexes(roots)
        write('stability.json', dict(input_hashes_match=True, siblings_match=True, indexes_match=True,
                                     inputs=len(files), tasks=len(rows)))
    elif sys.argv[1] == 'analyze':
        tasks = {t['id']: t for t in json.loads((OUT / 'tasks.json').read_text())}
        oracles = json.loads((OUT / 'oracles.json').read_text())
        results = []
        for row in json.loads((OUT / 'scores.json').read_text()):
            task = tasks[row['task']]
            required = {r['path'] for r in oracles[row['task']]['regions']}
            lookup = {h['id']: h['path'] for lane in ('metadata', 'body') for h in row[lane]}
            for policy in policies():
                ranked = rank(row, policy)
                paths = [lookup[id] for id in ranked]
                results.append(dict(task=task['id'], repo=task['repo'], kind=task['kind'],
                    family=task['family'], policy=policy, top8=ranked[:8],
                    required_files=sorted(required), top8_files=paths[:8],
                    all_files_at8=required <= set(paths[:8]), all_files_at50=required <= set(paths[:50]),
                    file_recall_at8=len(required & set(paths[:8])) / len(required)))
        summary = []
        for group in ('all', 'repo', 'kind'):
            values = ['all'] if group == 'all' else sorted({r[group] for r in results})
            for value in values:
                for policy in policies():
                    rows = [r for r in results if r['policy'] == policy and (group == 'all' or r[group] == value)]
                    summary.append(dict(group=group,value=value,policy=policy,tasks=len(rows),
                        all_files_at8=sum(r['all_files_at8'] for r in rows),
                        all_files_at50=sum(r['all_files_at50'] for r in rows),
                        mean_file_recall_at8=sum(r['file_recall_at8'] for r in rows)/len(rows)))
        write('rankings.json', results)
        write('summary.json', summary)
        print(json.dumps([r for r in summary if r['group'] == 'all'], indent=2))
    else:
        raise SystemExit('expected capture or analyze')


if __name__ == '__main__':
    main()
