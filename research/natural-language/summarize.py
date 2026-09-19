"""Aggregate committed retrieval/evidence/profile records without hiding regressions."""
import argparse
import json
from pathlib import Path
import statistics

BASE = Path(__file__).resolve().parent / 'results'
ROOT = BASE.parents[2]


def read(name):
    return json.loads((BASE / name).read_text())


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--raw-run-suffix', help='Also export changed evidence actions from evaluation/runs/natural-language-{arm}-{suffix}')
    args=parser.parse_args()
    core = read('frozen-public-core.json')
    assert core['source_validation'] == 'before and after passed'
    result = {'core': {}, 'public_index': {}, 'evidence_changes': [], 'profile': {}}
    for arm in ['baseline', 'current']:
        records = [r for repo in core['repos'].values() for r in repo['arms'][arm]['records']]
        assert len(records) == 354
        assert not any(r['error'] for r in records)
        result['core'][arm] = {}
        for group in sorted({r['request']['group'] for r in records}):
            subset = [r for r in records if r['request']['group'] == group]
            result['core'][arm][group] = {'hits': sum(r['rank'] is not None for r in subset),
                'count': len(subset), 'mrr': statistics.mean(1/r['rank'] if r['rank'] else 0 for r in subset)}
    result['exact_accuracy_gate'] = all(result['core'][a]['exact']['hits'] == 90 for a in ['baseline', 'current'])
    for repo in core['repos'].values():
        for old, new in zip(repo['arms']['baseline']['records'], repo['arms']['current']['records']):
            assert old['request'] == new['request']
            if old['request']['group'] == 'exact':
                assert new['rank'] is not None and new['rank'] <= old['rank']
    assert result['exact_accuracy_gate']
    rows_by_arm = {}
    for arm in ['baseline', 'current']:
        rows = read(f'public-index-{arm}-results.json')
        manifest = read(f'public-index-{arm}-manifest.json')
        assert len(rows) == 60
        assert not any(r['errors'] for r in rows)
        assert manifest['source_validation'] == 'before and after passed'
        assert not manifest['cleanup_errors']
        for repo, source in manifest['roots'].items():
            assert source['source_snapshot_before'] == source['source_snapshot_after'] == core['repos'][repo]['before']
        rows_by_arm[arm] = {r['task_id']: r for r in rows}
        result['public_index'][arm] = {}
        for group in ['all', 'dev', 'heldout', 'nanus', 'blogwright', 'whatsurvey']:
            subset = [r for r in rows if group == 'all' or group in (r['repo'], r['split'])]
            result['public_index'][arm][group] = {
                'count': len(subset), 'required_file_hits': sum(r['evidence']['required_file_recall'] for r in subset),
                'evidence_ready': sum(r['evidence']['evidence_ready'] for r in subset),
                'task_success': None,
                'median_response_bytes': statistics.median(r['response_bytes'] for r in subset)}
    for task, current in rows_by_arm['current'].items():
        baseline = rows_by_arm['baseline'][task]
        if baseline['evidence'] != current['evidence']:
            result['evidence_changes'].append({'task': task, 'baseline': baseline['evidence'], 'current': current['evidence']})
    if args.raw_run_suffix:
        diagnostics=[]
        oracles=json.loads((ROOT/'evaluation/oracles.json').read_text())
        for task,current in rows_by_arm['current'].items():
            baseline=rows_by_arm['baseline'][task]
            if baseline['evidence']['evidence_ready']==current['evidence']['evidence_ready']:
                continue
            item={'task':task,'split':current['split'],'regions':oracles[task]['regions'],'arms':{}}
            for arm in ['baseline','current']:
                raw_path=ROOT/'evaluation/runs'/f'natural-language-{arm}-{args.raw_run_suffix}'/'trials'/(rows_by_arm[arm][task]['trial_id']+'.json')
                raw=json.loads(raw_path.read_text())
                item['arms'][arm]={'evidence':rows_by_arm[arm][task]['evidence'],
                    'actions':[h['action'] for h in raw['history']],
                    'candidates':[line for line in raw['history'][0]['response'].splitlines() if line.startswith('candidate ')]}
            diagnostics.append(item)
        (BASE/'evidence-changes.json').write_text(json.dumps(diagnostics,indent=2)+'\n')
    profile = read('public-index-profile.json')
    for repo, data in profile['repos'].items():
        assert data['before'] == data['after'] == core['repos'][repo]['before']
        result['profile'][repo] = {arm: {k: v for k, v in run.items() if k in ('setup_ms', 'live_store_bytes', 'median_ms')}
                                   for arm, run in data['arms'].items()}
    for arm in ['baseline', 'current']:
        assert profile['binaries'][arm] == read(f'public-index-{arm}-manifest.json')['implementation']['host_sha256']
    (BASE / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'evidence_changes'}, indent=2))


if __name__ == '__main__':
    main()
