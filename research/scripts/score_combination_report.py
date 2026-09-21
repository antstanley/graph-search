"""Interpret generic capture arm names and audit every normalized-policy loss."""
import json
import pathlib
import statistics
import sys

OUT = pathlib.Path(__file__).resolve().parents[1] / 'results/native-implementation/score-combination-context'
capture = OUT / 'capture'
raw = pathlib.Path(sys.argv[1])
tasks = {t['id']: t for t in json.loads((capture / 'tasks.json').read_text())}
rows = json.loads((capture / 'results.json').read_text())
first = {(r['task_id'], r['arm']): r for r in rows if r['repeat'] == 0}
summary, losses, changes = [], [], []
for group in ['all', 'repo', 'kind']:
    values = ['all'] if group == 'all' else sorted({t[group] for t in tasks.values()})
    for value in values:
        selected = [t for t in tasks.values() if group == 'all' or t[group] == value]
        for arm, label in [('structured', 'baseline'), ('fixed', 'normalized')]:
            records = [first[t['id'], arm]['evidence'] for t in selected]
            summary.append(dict(group=group, value=value, arm=label, tasks=len(records),
                complete_regions=sum(r['evidence_ready'] for r in records),
                all_required_files=sum(r['required_file_recall'] == 1 for r in records),
                mean_region_coverage=statistics.mean(statistics.mean(r['region_coverage'].values()) for r in records)))
for task in tasks:
    baseline, candidate = first[task, 'structured']['evidence'], first[task, 'fixed']['evidence']
    if baseline != candidate:
        changes.append(dict(task=task, baseline=baseline, normalized=candidate))
    if any(candidate['region_coverage'][key] < value for key, value in baseline['region_coverage'].items()):
        arms = {}
        for arm, label in [('structured', 'baseline'), ('fixed', 'normalized')]:
            document = json.loads((raw / f'{arm}-{task}-0.json').read_text())
            response = json.loads(document['history'][0]['response'])
            arms[label] = dict(evidence=document['evidence'],
                followups=[h['action'] for h in document['history'][1:]],
                first_items=[dict(id=i['node']['id'], name=i['node']['name'], path=i['node']['path'], span=i['node'].get('span')) for i in response['items']],
                first_response_bytes=document['history'][0]['response_bytes'],
                total_response_bytes=document['response_bytes'])
        losses.append(dict(task=task, **arms))
base = {(r['group'], r['value']): r for r in summary if r['arm'] == 'baseline'}
gains = [t for t in tasks if first[t, 'fixed']['evidence']['evidence_ready'] and not first[t, 'structured']['evidence']['evidence_ready']]
complete_losses = [t for t in tasks if first[t, 'structured']['evidence']['evidence_ready'] and not first[t, 'fixed']['evidence']['evidence_ready']]
group_losses = [dict(group=r['group'], value=r['value']) for r in summary if r['arm'] == 'normalized' and r['mean_region_coverage'] < base[r['group'], r['value']]['mean_region_coverage']]
gate = dict(complete_region_gains=gains, complete_region_losses=complete_losses,
            lower_mean_coverage_groups=group_losses,
            promote=bool(gains) and not complete_losses and not group_losses,
            decision='retain production default; normalized candidate not integrated')
for name, value in [('summary.json', summary), ('changes.json', changes), ('losses.json', losses), ('decision.json', gate)]:
    (OUT / name).write_text(json.dumps(value, indent=2) + '\n')
print(json.dumps(gate, indent=2))
