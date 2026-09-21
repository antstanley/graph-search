"""Paired public-API source representation and context-budget experiments.

No model task-success or latency claim. A disposable control changes source
structure, returned package identity/sharing, source admission, or residual fragment allocation. Exact
interventions are recorded with each capture. Production and sibling indexes
are read-only.
"""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import random
import shutil
import statistics
import subprocess
import sys
import tempfile

from markdown_statistics import digest, indexes, write
from native_review import ROOT, eligible_tasks
from score_combination_patch import PATCHES as SCORE_COMBINATION_PATCHES
sys.path.insert(0, str(ROOT / 'evaluation'))
from taskbench.backends import GraphSearch
from taskbench.core import coverage, validate
from taskbench.provenance import repository_snapshot
from taskbench.runner import sanitized, trial

OLD = 'let markdown = if source_kind == SourceUnitKind::Markdown {'
NEW = 'let markdown = if false {'
INTERVENTIONS = {
    'score_combination': SCORE_COMBINATION_PATCHES,
    'markdown': [(OLD, NEW)],
    'documentation': [('        documentation.comments,', '        &[],')],
    'graph_context': [('            graph_context: GraphContext::Semantic,',
                       '            graph_context: GraphContext::None,')],
    'graph_neighborhoods': [('        self.neighborhoods.borrow_mut().read(\n            self.snapshot,\n            id,\n            kinds,\n            dir,\n            &mut self.work.borrow_mut(),\n        )', '        self.snapshot.edges_bounded(id, kinds, dir, &mut self.work.borrow_mut())')],
    'context_proximity': [('pub(crate) fn bonus(positions: &[(u32, u128)]) -> u64 {',
        'pub(crate) fn bonus(positions: &[(u32, u128)]) -> u64 {\n    if true { return 0; }')],
    'source_admission': [
        ('            if total_bytes.saturating_add(metadata_bytes) > max {',
         '            if false {'),
        ('            let materialize = query.context_lines > 0\n                && total_bytes\n                    .saturating_add(metadata_bytes)\n                    .saturating_add(minimum_snippet_growth)\n                    <= max\n                && minimum_result_bytes\n                    .saturating_add(metadata_bytes_total)\n                    .saturating_add(metadata_bytes)\n                    .saturating_add(items.len()) // Commas between admitted items.\n                    .saturating_add(minimum_snippet_growth)\n                    <= max;',
         '            let materialize = query.context_lines > 0;'),
    ],
    'primary_dedup': [('    let sources = sources(result);',
        '    let sources = sources(result);\n    if true { return Prepared { sources, shared: BTreeSet::new() }; }')],
    'package_sharing': [('        if ids.len() < 2 {', '        if true {')],
    'context_fragments': [('    if length <= 5 {', '    if true {')],
    'package_metadata': [(
        '                evidence: body.cloned(),',
        '                evidence: body.cloned().map(|mut evidence| { evidence.package = None; evidence }),',
    )],
    'code': [
        ('let mut boundaries = symbol_boundaries(text, symbols);',
         'let mut boundaries = symbol_boundaries(text, &[]);'),
        ('        documentation.comments,', '        &[],'),
    ],
}



def inventory():
    names = subprocess.check_output(['git', 'ls-files', '--cached', '--others',
        '--exclude-standard', '-z', 'crates', '.cargo', 'Cargo.toml', 'Cargo.lock',
        'evaluation/harness', 'evaluation/taskbench', 'research/scripts'], cwd=ROOT)
    return {name.decode(): digest(ROOT / name.decode())
            for name in names.split(b'\0') if name and (ROOT / name.decode()).is_file()}


def build(root, target, destination):
    subprocess.run(['cargo', 'build', '--release', '--offline', '--locked',
        '--manifest-path', str(root / 'evaluation/harness/Cargo.toml'),
        '--target-dir', str(target)], cwd=root, check=True)
    shutil.copy2(target / 'release/task-eval-host', destination)


def compare_trials(raw, rows):
    """Validate deterministic evidence/actions and retain every paired line delta."""
    documents = {
        (row['task_id'], row['arm'], row['repeat']): json.loads(
            (raw / f"{row['arm']}-{row['task_id']}-{row['repeat']}.json").read_text())
        for row in rows
    }
    if len(documents) != len(rows):
        raise ValueError('duplicate trial identity')
    def actions(document):
        return [entry['action'] for entry in document['history']]
    checks = {
        'repeat_evidence_identical': True,
        'repeat_delivered_lines_identical': True,
        'repeat_actions_identical': True,
        'paired_actions_identical': True,
    }
    evidence_changes, line_changes = [], []
    for (task, arm, repeat), document in sorted(documents.items()):
        first = documents[task, arm, 0]
        checks['repeat_evidence_identical'] &= document['evidence'] == first['evidence']
        checks['repeat_delivered_lines_identical'] &= document['seen'] == first['seen']
        checks['repeat_actions_identical'] &= actions(document) == actions(first)
        if arm != 'structured':
            continue
        control = documents[task, 'fixed', repeat]
        checks['paired_actions_identical'] &= actions(document) == actions(control)
        if repeat:
            continue
        if document['evidence'] != control['evidence']:
            evidence_changes.append({'task_id': task, 'control': control['evidence'],
                                     'candidate': document['evidence']})
        candidate_lines = set(map(tuple, document['seen']))
        control_lines = set(map(tuple, control['seen']))
        if candidate_lines != control_lines:
            line_changes.append({'task_id': task,
                                 'added': sorted(candidate_lines - control_lines),
                                 'removed': sorted(control_lines - candidate_lines)})
    checks['changed_task_evidence'] = len(evidence_changes)
    checks['changed_full_protocol_line_sets'] = len(line_changes)
    return checks, evidence_changes, line_changes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--representation', choices=tuple(INTERVENTIONS), default='markdown',
                        help='Code removes declaration/comment boundaries; package_metadata omits identity; context_fragments disables residual splitting')
    parser.add_argument('--suite', type=Path, action='append', default=[],
                        help='Additional frozen suite; may be repeated')
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error('--repeats must be positive')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    roots = {name: ROOT.parent / name for name in ('nanus', 'blogwright', 'whatsurvey')}
    tasks, oracles, excluded = eligible_tasks(roots)
    suites = {task['id']: 'established' for task in tasks}
    for directory in args.suite:
        freeze = json.loads((directory / 'freeze.json').read_text())
        for name in ('tasks.json', 'oracles.json'):
            if digest(directory / name) != freeze['artifacts'][name]:
                raise ValueError(f'frozen suite changed: {directory}/{name}')
        extra = json.loads((directory / 'tasks.json').read_text())
        labels = json.loads((directory / 'oracles.json').read_text())
        validate(extra, labels, roots)
        if set(oracles) & set(labels):
            raise ValueError('duplicate task IDs across suites')
        tasks.extend(extra)
        oracles.update(labels)
        suites.update({task['id']: directory.name for task in extra})
    validate(tasks, oracles, roots)
    write(out / 'tasks.json', tasks)
    write(out / 'label-validation.json', {'excluded': excluded, 'suites': suites})
    write(out / 'oracles.json', oracles)
    before = inventory()
    siblings = {name: repository_snapshot(root) for name, root in roots.items()}
    index_before = {name: indexes(root) for name, root in roots.items()}
    raw = Path(tempfile.mkdtemp(prefix='graph-search-markdown-context-'))
    print('Temporary builds and raw transcripts:', raw, flush=True)
    control = raw / 'control'
    for name in before:
        if name.startswith(('crates/', '.cargo/', 'evaluation/harness/')) or name in ('Cargo.toml', 'Cargo.lock'):
            destination = control / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, destination)
    intervention_path = {
        'score_combination': 'crates/core/src/query.rs',
        'package_metadata': 'crates/core/src/query.rs',
        'source_admission': 'crates/core/src/query.rs',
        'context_proximity': 'crates/core/src/context_proximity.rs',
        'graph_neighborhoods': 'crates/core/src/query.rs',
        'graph_context': 'crates/types/src/retrieval.rs',
        'package_sharing': 'crates/core/src/packages.rs',
        'primary_dedup': 'crates/core/src/context_dedup.rs',
        'context_fragments': 'crates/core/src/evidence.rs',
    }.get(args.representation, 'crates/core/src/units.rs')
    unit_path = control / intervention_path
    text = unit_path.read_text()
    patches = INTERVENTIONS[args.representation]
    for old, new in patches:
        if text.count(old) != 1:
            raise ValueError(f'control intervention no longer matches exactly once: {old!r}')
        text = text.replace(old, new)
    unit_path.write_text(text)
    delta = [str(path.relative_to(control)) for path in control.rglob('*')
             if path.is_file() and digest(path) != before[str(path.relative_to(control))]]
    if delta != [intervention_path]:
        raise ValueError(f'unexpected control delta: {delta}')
    binaries = {arm: raw / arm for arm in ('structured', 'fixed')}
    target = ROOT / 'research/harness/target'
    build(ROOT, target, binaries['structured'])
    build(control, target, binaries['fixed'])
    hashes = {arm: digest(binary) for arm, binary in binaries.items()}
    write(out / 'build.json', {'source_sha256': before, 'binary_sha256': hashes,
        'representation': args.representation,
        'control_delta': {'path': delta[0], 'patches': [{'before': old, 'after': new} for old, new in patches],
                          'sha256': digest(unit_path)},
        'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
        'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'profile': 'release offline locked', 'note': __doc__})
    rows = []
    for repo, root in roots.items():
        backends = {}
        try:
            for arm, binary in binaries.items():
                backends[arm] = GraphSearch(root, binary, retrieval={'ranking': 'auto', 'per_file': 0})
            cases = [(task, repeat) for task in tasks if task['repo'] == repo for repeat in range(args.repeats)]
            random.Random(20260920).shuffle(cases)
            for number, (task, repeat) in enumerate(cases):
                arms = list(backends)
                if number % 2:
                    arms.reverse()
                for arm in arms:
                    result = trial(task, backends[arm])
                    result['evidence'] = coverage(oracles[task['id']], {tuple(item) for item in result['seen']})
                    result.update(arm=arm, repeat=repeat, suite=suites[task['id']])
                    write(raw / f"{arm}-{task['id']}-{repeat}.json", result)
                    rows.append(sanitized(result))
            write(out / 'results.json', rows)
            print(repo, 'complete:', len(rows), 'trials', flush=True)
        finally:
            for backend in backends.values():
                backend.close()
    summary = {}
    for suite in sorted(set(suites.values())):
        summary[suite] = {}
        for arm in binaries:
            selected = [r for r in rows if r['suite'] == suite and r['arm'] == arm and r['repeat'] == 0]
            summary[suite][arm] = {'tasks': len(selected),
                'required_files': sum(r['evidence']['required_file_recall'] == 1 for r in selected),
                'complete_regions': sum(r['evidence']['evidence_ready'] for r in selected),
                'mean_region_coverage': statistics.mean(statistics.mean(r['evidence']['region_coverage'].values()) for r in selected),
                'mean_response_bytes': statistics.mean(r['response_bytes'] for r in selected)}
    write(out / 'summary.json', summary)
    stable = {'production': before == inventory(),
              'binaries': hashes == {arm: digest(binary) for arm, binary in binaries.items()},
              'siblings': siblings == {name: repository_snapshot(root) for name, root in roots.items()},
              'indexes': index_before == {name: indexes(root) for name, root in roots.items()}}
    write(out / 'stability.json', {'checks': stable, 'siblings_before': siblings, 'indexes_before': index_before})
    errors = [r for r in rows if r['errors'] or r['status'] != 'protocol_complete']
    write(out / 'manifest.json', {'trials': len(rows), 'errors': len(errors), 'repeats': args.repeats,
        'representation': args.representation,
        'budgets': rows[0]['budgets'], 'protocol': 'evidence-v1', 'note': __doc__})
    checks, evidence_changes, line_changes = compare_trials(raw, rows)
    checks.update(trials=len(rows), errors=len(errors), stability=stable)
    write(out / 'checks.json', checks)
    write(out / 'paired-changes.json', evidence_changes)
    write(out / 'protocol-line-changes.json', line_changes)
    deterministic = all(checks[name] for name in (
        'repeat_evidence_identical', 'repeat_delivered_lines_identical',
        'repeat_actions_identical'))
    if not deterministic:
        raise ValueError('experiment produced nondeterministic evidence, lines or actions')
    if not all(stable.values()) or errors:
        raise ValueError('experiment failed stability or protocol checks')


if __name__ == '__main__':
    main()
