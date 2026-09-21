#!/usr/bin/env python3
"""Factorial metadata scoring on source-valid labeled families; no production tuning."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
from native_review import eligible_tasks, ROOT
from positional_review import capture, digest, write


def identity(host, reference=None):
    result = capture(host)
    result['scoring_driver_sha256'] = digest(Path(__file__))
    result['selection_driver_sha256'] = digest(ROOT / 'research/scripts/native_review.py')
    result['labels_sha256'] = {name: digest(ROOT / 'evaluation' / name)
                              for name in ('tasks.json', 'oracles.json')}
    if reference:
        result['reference_files_sha256'] = {p.name: digest(p) for p in sorted(reference.glob('*.json'))}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--split', choices=('dev', 'heldout'), default='dev')
    parser.add_argument('--reference', type=Path, help='Verify unchanged rankings against an earlier capture; retain compact equivalence reports')
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    roots = {name: ROOT.parent / name for name in ('nanus', 'blogwright', 'whatsurvey')}
    tasks, oracles, excluded = eligible_tasks(roots)
    tasks = [task for task in tasks if task['split'] == args.split]
    assert tasks, 'no source-valid tasks in requested split'
    write(out / 'labels.json', {'split': args.split, 'selected': tasks, 'excluded': excluded,
                              'oracles': {task['id']: oracles[task['id']] for task in tasks},
                              'notice': 'Both label sets were exposed in earlier experiments; heldout is the historical split label, not a blinded validation claim.'})
    host = ROOT / 'research/harness/target/release/scoring_probe'
    before = identity(host, args.reference)
    write(out / 'provenance-before.json', before)
    if args.reference:
        prior = json.loads((args.reference / 'provenance-before.json').read_text())
        assert prior['repositories'] == before['repositories'], 'reference repository contents differ'
        assert prior['codegraph_sha256'] == before['codegraph_sha256'], 'reference CodeGraph state differs'
    rows = []
    with tempfile.TemporaryDirectory(prefix='graph-search-scoring-') as tmp:
        for repo, root in roots.items():
            selected = [task for task in tasks if task['repo'] == repo]
            if not selected:
                continue
            task_file = Path(tmp) / f'{repo}.json'
            write(task_file, selected)
            run = subprocess.run([str(host), str(root), str(task_file)], cwd=ROOT,
                                 capture_output=True, text=True)
            (out / f'{repo}.stderr').write_text(run.stderr)
            if run.returncode:
                write(out / 'failure.json', {'repo': repo, 'exit_code': run.returncode})
                raise RuntimeError(f'{repo} scorer failed; diagnostics retained')
            result = json.loads(run.stdout)
            if args.reference:
                reference_path = args.reference / f'{repo}.json'
                prior = json.loads(reference_path.read_text())
                assert result['rows'] == prior['rows'], f'{repo} candidate rankings differ from reference'
                write(out / f'{repo}.json', {'reference_sha256': digest(reference_path),
                      'rankings_equal': True, 'symbols': result['symbols'],
                      'native_bit_exact_checks': result['native_bit_exact_checks']})
            else:
                write(out / f'{repo}.json', result)
            by_id = {task['id']: task for task in selected}
            for query in result['rows']:
                task = by_id[query['task']]
                required = {region['path'] for region in oracles[task['id']]['regions']}
                for variant in query['variants']:
                    key = f"{variant['idf']}-{variant['normalization']}-whole{int(variant['whole'])}-qualified{int(variant['qualified'])}"
                    ranks = {path: next((i+1 for i, hit in enumerate(variant['top']) if hit['path'] == path), None)
                             for path in sorted(required)}
                    row = {'task': task['id'], 'repo': repo, 'family': task['family'],
                           'variant': key, 'target_file_ranks': ranks, 'required_files': len(required)}
                    for k in (8, 50):
                        present = sum(rank is not None and rank <= k for rank in ranks.values())
                        row[f'all_files_at_{k}'] = present == len(required)
                        row[f'file_recall_at_{k}'] = present / len(required)
                    rows.append(row)
            print(repo, 'ok', result['native_bit_exact_checks'], 'bit-exact checks', flush=True)
    after = identity(host, args.reference)
    write(out / 'provenance-after.json', after)
    write(out / 'rows.json', rows)
    summary = {}
    for variant in sorted({row['variant'] for row in rows}):
        values = [row for row in rows if row['variant'] == variant]
        families = sorted({(row['repo'], row['family']) for row in values})
        summary[variant] = {'tasks': len(values), 'families': len(families)}
        for k in (8, 50):
            summary[variant][f'all_files_at_{k}'] = sum(row[f'all_files_at_{k}'] for row in values)
            summary[variant][f'mean_file_recall_at_{k}'] = sum(row[f'file_recall_at_{k}'] for row in values) / len(values)
            summary[variant][f'family_macro_recall_at_{k}'] = sum(
                sum(row[f'file_recall_at_{k}'] for row in values if (row['repo'], row['family']) == family) /
                sum((row['repo'], row['family']) == family for row in values) for family in families) / len(families)
    write(out / 'summary.json', {'stable_provenance': before == after, 'split': args.split,
          'scope': 'metadata candidate file recall only; same top-8/top-50 owner budgets; no body fusion, context-byte delivery, model success or performance claim',
          'variants': summary})
    if before != after:
        raise RuntimeError('frozen sources/labels/drivers/binary/repositories changed during capture')

if __name__ == '__main__':
    main()
