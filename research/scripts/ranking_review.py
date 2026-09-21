#!/usr/bin/env python3
"""Controlled native channel × diversity ablation using the existing evidence protocol.

Uses source-valid labels only. Raw source transcripts stay in a temporary directory.
The default uses previously exposed labels. --suite accepts a separately frozen new-query set.
Neither mode measures model task success or claims a blinded evaluation.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import random
import statistics
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'evaluation'))
from taskbench.backends import GraphSearch
from taskbench.core import coverage, validate
from taskbench.provenance import implementation, repository_snapshot
from taskbench.runner import sanitized, trial
from native_review import eligible_tasks


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--roots', type=Path)
    parser.add_argument('--suite', type=Path, help='Frozen tasks.json, oracles.json and freeze.json directory')
    parser.add_argument('--variants', help='Optional comma-separated ranking:per_file variants, e.g. auto:0')
    parser.add_argument('--analysis', choices=('split', 'identifiers'), help='Explicit analyzer override for a controlled comparison')
    parser.add_argument('--query-policy', choices=('verbatim', 'task'), help='Explicit procedural prompt policy; defaults remain unchanged')
    parser.add_argument('--normalization', choices=('combined', 'bm25f'), help='Explicit metadata normalization override; body scoring stays fixed')
    parser.add_argument('--graph-context', choices=('semantic', 'none', 'calls', 'imports', 'types'), help='Explicit graph enrichment policy; lexical ranking stays fixed')
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error('--repeats must be positive')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    roots = ({k: Path(v).resolve() for k, v in json.loads(args.roots.read_text()).items()}
             if args.roots else {name: ROOT.parent / name for name in ('nanus', 'blogwright', 'whatsurvey')})
    if args.suite:
        freeze = json.loads((args.suite / 'freeze.json').read_text())
        for name in ('tasks.json', 'oracles.json'):
            if hashlib.sha256((args.suite / name).read_bytes()).hexdigest() != freeze['artifacts'][name]:
                raise ValueError(f'frozen suite changed: {name}')
        tasks = json.loads((args.suite / 'tasks.json').read_text())
        oracles = json.loads((args.suite / 'oracles.json').read_text())
        validate(tasks, oracles, roots)
        excluded = []
        write(out / 'suite-freeze.json', freeze)
    else:
        tasks, oracles, excluded = eligible_tasks(roots)
    write(out / 'label-validation.json', {'valid_ids': [task['id'] for task in tasks], 'excluded': excluded})
    if not tasks:
        raise ValueError('no source-valid tasks')
    target = ROOT / 'research/harness/target'
    subprocess.run(['cargo', 'build', '--release', '--offline', '--locked', '--manifest-path',
                    str(ROOT / 'evaluation/harness/Cargo.toml'), '--target-dir', str(target)], cwd=ROOT, check=True)
    host = target / 'release/task-eval-host'
    if not host.is_file():
        raise FileNotFoundError(host)
    raw = Path(tempfile.mkdtemp(prefix='graph-search-ranking-'))
    print('Raw transcripts:', raw, flush=True)
    before = {name: repository_snapshot(root) for name, root in roots.items()}
    tracked = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z', 'crates'], cwd=ROOT)
    paths = [ROOT / item.decode() for item in tracked.split(b'\0') if item]
    paths += [Path(__file__).resolve(), ROOT / 'Cargo.toml', ROOT / 'Cargo.lock']
    provenance = implementation(ROOT / 'evaluation', host)
    provenance['production_source_sha256'] = {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}
    write(out / 'provenance.json', provenance)
    rows, summary = [], {}
    variants = [(ranking, per_file) for ranking in ('metadata', 'body', 'fusion') for per_file in (0, 1, 2)]
    if args.variants:
        variants = [(name, int(limit)) for name, limit in (value.split(':') for value in args.variants.split(','))]
        if any(name not in ('auto', 'metadata', 'body', 'fusion') or not 0 <= limit <= 65535 for name, limit in variants):
            parser.error('invalid ranking or per_file variant')
    for ranking, per_file in variants:
        variant = f'{ranking}-diversity-{per_file}'
        options = {'ranking': ranking, 'per_file': per_file}
        if args.query_policy:
            options['query_policy'] = args.query_policy
            variant += '-query-policy-' + args.query_policy
        if args.analysis:
            options['analysis'] = args.analysis
            variant += '-analysis-' + args.analysis
        if args.normalization:
            options['normalization'] = args.normalization
            variant += '-normalization-' + args.normalization
        if args.graph_context:
            options['graph_context'] = args.graph_context
            variant += '-graph-context-' + args.graph_context
        for repo, root in roots.items():
            cases = [(task, repeat) for task in tasks if task['repo'] == repo for repeat in range(args.repeats)]
            random.Random(20260919).shuffle(cases)
            backend = GraphSearch(root, host, retrieval=options)
            try:
                for task, repeat in cases:
                    result = trial(task, backend)
                    result['evidence'] = coverage(oracles[task['id']], {tuple(item) for item in result['seen']})
                    result.update(variant=variant, retrieval=options, repeat=repeat)
                    write(raw / f"{variant}-{task['id']}-{repeat}.json", result)
                    rows.append(sanitized(result))
            finally:
                backend.close()
            print(variant, repo, 'complete', len(rows), flush=True)
        selected = [row for row in rows if row['variant'] == variant and row['repeat'] == 0]
        summary[variant] = {
            'options': options, 'tasks': len(selected),
            'required_files': sum(row['evidence']['required_file_recall'] == 1 for row in selected),
            'complete_regions': sum(row['evidence']['evidence_ready'] for row in selected),
            'mean_per_task_region_coverage': statistics.mean(statistics.mean(row['evidence']['region_coverage'].values()) for row in selected),
            'mean_response_bytes': statistics.mean(row['response_bytes'] for row in selected),
        }
        write(out / 'results.json', rows)
        write(out / 'summary.json', summary)
    after = {name: repository_snapshot(root) for name, root in roots.items()}
    write(out / 'source-stability.json', {'before': before, 'after': after, 'equal': before == after})
    if before != after:
        raise ValueError('external source changed during ablation')
    source_after = {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}
    write(out / 'production-stability.json', {'before': provenance['production_source_sha256'], 'after': source_after,
                                            'equal': provenance['production_source_sha256'] == source_after})
    if provenance['production_source_sha256'] != source_after:
        raise ValueError('production source or ranking driver changed during ablation')
    errors = [row for row in rows if row['errors'] or row['status'] != 'protocol_complete']
    write(out / 'manifest.json', {
        'protocol': 'evidence-v1', 'repeats': args.repeats, 'variants': len(variants),
        'followup_variant_override': args.variants, 'analysis_override': args.analysis,
        'normalization_override': args.normalization, 'graph_context_override': args.graph_context,
        'query_policy_override': args.query_policy,
        'suite': str(args.suite) if args.suite else 'previously-exposed-source-valid-subset',
        'trials': len(rows), 'protocol_errors': len(errors),
        'budgets': rows[0]['budgets'], 'note': __doc__,
    })
    if errors:
        raise ValueError(f'{len(errors)} trial errors; inspect results.json')


if __name__ == '__main__':
    main()
