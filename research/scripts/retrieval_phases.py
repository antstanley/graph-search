"""Coarse inclusive phase timings in a disposable native build; no production edits.

Baseline/profile outputs must match except context.generation and stats.elapsed_ms.
No model-success claim. All timers buffer until the outer service call completes.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import re
import shutil
import statistics
import tempfile
import time

from markdown_context import build, inventory
from markdown_statistics import digest, indexes, write
from native_review import ROOT
from taskbench.backends import GraphSearch
from taskbench.provenance import repository_snapshot

TIMER = '''//! Disposable research-only inclusive phase recorder.
use std::{cell::RefCell, time::Instant};
thread_local! { static EVENTS: RefCell<Vec<(&'static str, u128)>> = const { RefCell::new(Vec::new()) }; }
pub struct Timer { label: &'static str, start: Instant }
impl Timer {
    pub fn new(label: &'static str) -> Self { Self { label, start: Instant::now() } }
}
impl Drop for Timer {
    fn drop(&mut self) {
        let ns = self.start.elapsed().as_nanos();
        EVENTS.with(|events| {
            let mut events = events.borrow_mut();
            events.push((self.label, ns));
            if self.label == "service" {
                for (label, duration) in events.drain(..) {
                    eprintln!("GS_PHASE\\t{label}\\t{duration}");
                }
            }
        });
    }
}
'''
PHASES = {
    'crates/graph-search/src/service.rs': [('explore', 'service'), ('prepare_context', 'prepare_context'), ('context', 'service_context')],
    'crates/core/src/query.rs': [('explore_with_context', 'query'), ('seed', 'seed'), ('connect', 'connect')],
    'crates/core/src/body.rs': [('collect_scores', 'body_postings'), ('rank', 'body_topk')],
    'crates/core/src/lexical.rs': [('accumulate_inner', 'metadata_postings'), ('accumulate_all_inner', 'metadata_postings')],
    'crates/core/src/metadata.rs': [('search_top_with_policy', 'metadata_retrieval')],
    'crates/core/src/evidence.rs': [('extend_prepared', 'evidence')],
}


def instrument(root):
    changes = {}
    for name, methods in PHASES.items():
        path = root / name
        source = path.read_text()
        qualifier = 'graph_search_core' if '/graph-search/' in name else 'crate'
        for method, label in methods:
            matches = list(re.finditer(r'\bfn ' + method + r'\s*\(', source))
            if len(matches) != 1:
                raise ValueError(f'expected one method: {name}/{method}')
            opening = source.index('{', matches[0].end()) + 1
            source = source[:opening] + f'\n        let _research_phase = {qualifier}::research_profile::Timer::new("{label}");' + source[opening:]
        path.write_text(source)
        changes[name] = digest(path)
    path = root / 'crates/core/src/lib.rs'
    path.write_text(path.read_text() + '\n#[doc(hidden)]\npub mod research_profile;\n')
    changes[str(path.relative_to(root))] = digest(path)
    path = root / 'crates/core/src/research_profile.rs'
    path.write_text(TIMER)
    changes[str(path.relative_to(root))] = digest(path)
    return changes


def normalized(value):
    value = json.loads(json.dumps(value))
    value['context']['generation'] = None
    value['stats']['elapsed_ms'] = 0
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repeats', type=int, default=3)
    args = parser.parse_args()
    if args.repeats < 3:
        parser.error('at least three repeats required')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    roots = {name: ROOT.parent / name for name in ('nanus', 'blogwright', 'whatsurvey')}
    task_path = ROOT / 'research/results/native-implementation/graph-context-native-json/tasks.json'
    tasks = json.loads(task_path.read_text())
    queries = [{'id': t['id'], 'repo': t['repo'], 'query': t['prompt'], 'ranking': 'auto', 'family': 'task'} for t in tasks]
    for repo in roots:
        for query in ('return', 'const', 'error', 'source', 'error response'):
            for ranking in ('auto', 'metadata', 'body'):
                queries.append({'id': f'{repo}.broad.{query}.{ranking}', 'repo': repo, 'query': query, 'ranking': ranking, 'family': 'broad'})
    write(out / 'queries.json', queries)
    before = inventory()
    siblings = {name: repository_snapshot(root) for name, root in roots.items()}
    index_before = {name: indexes(root) for name, root in roots.items()}
    raw = Path(tempfile.mkdtemp(prefix='graph-search-retrieval-phases-'))
    print('Temporary builds:', raw, flush=True)
    copied = raw / 'instrumented'
    for name in before:
        if name.startswith(('crates/', '.cargo/', 'evaluation/harness/')) or name in ('Cargo.toml', 'Cargo.lock'):
            destination = copied / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, destination)
    changes = instrument(copied)
    write(out / 'instrumentation.json', {'phases': PHASES, 'timer': TIMER, 'modified_sha256': changes, 'temporary_root': str(raw)})
    binaries = {arm: raw / f'{arm}-host' for arm in ('baseline', 'profile')}
    for arm, root in [('baseline', ROOT), ('profile', copied)]:
        build(root, ROOT / 'evaluation/harness/target', binaries[arm])
    hashes = {arm: digest(binary) for arm, binary in binaries.items()}
    write(out / 'build.json', {'sources': before, 'binaries': hashes, 'task_sha256': digest(task_path)})
    rows = []
    for number, (repo, root) in enumerate(roots.items()):
        backends = {}
        try:
            for arm in binaries:
                backends[arm] = GraphSearch(root, binaries[arm])
            selected = [query for query in queries if query['repo'] == repo]
            # Equal explicit warmup; discard timings but verify output equivalence.
            for query in selected:
                outputs = []
                for arm, backend in backends.items():
                    backend.retrieval = {'ranking': query['ranking']}
                    outputs.append(normalized(json.loads(backend.search(query['query']))))
                assert outputs[0] == outputs[1], query
            for repeat in range(args.repeats):
                order = selected.copy()
                random.Random(1000 + repeat).shuffle(order)
                for position, query in enumerate(order):
                    arms = list(binaries)
                    if (number + repeat + position) % 2:
                        arms.reverse()
                    for arm in arms:
                        backend = backends[arm]
                        backend.retrieval = {'ranking': query['ranking']}
                        offset = os.fstat(backend.stderr.fileno()).st_size
                        started = time.perf_counter_ns()
                        value = json.loads(backend.search(query['query']))
                        elapsed = time.perf_counter_ns() - started
                        length = os.fstat(backend.stderr.fileno()).st_size - offset
                        # pread never moves the file offset shared with the child.
                        trace = os.pread(backend.stderr.fileno(), length, offset).decode()
                        phases = {}
                        for line in trace.splitlines():
                            if line.startswith('GS_PHASE\t'):
                                _, label, ns = line.split('\t')
                                phases[label] = phases.get(label, 0) + int(ns)
                        assert ('service' in phases) == (arm == 'profile'), (arm, trace)
                        rows.append({'id': query['id'], 'repo': repo, 'family': query['family'], 'ranking': query['ranking'], 'arm': arm, 'repeat': repeat, 'external_ns': elapsed, 'phases_ns': phases, 'result_sha256': normalized(value), 'stats': value['stats']})
                print(repo, 'repeat', repeat, 'complete', flush=True)
            write(out / 'results.json', rows)
        finally:
            for backend in backends.values():
                backend.close()
    stable = {'sources': before == inventory(), 'binaries': hashes == {arm: digest(binary) for arm, binary in binaries.items()}, 'siblings': siblings == {name: repository_snapshot(root) for name, root in roots.items()}, 'indexes': index_before == {name: indexes(root) for name, root in roots.items()}, 'tasks': digest(task_path) == json.loads((out / 'build.json').read_text())['task_sha256']}
    write(out / 'stability.json', {'checks': stable, 'siblings_before': siblings, 'indexes_before': index_before})
    assert all(stable.values()), stable
    paired = []
    for query in queries:
        selected = [r for r in rows if r['id'] == query['id']]
        assert len(selected) == args.repeats * 2
        assert len({r['result_sha256'] for r in selected}) == 1, query
        profiled = [r for r in selected if r['arm'] == 'profile']
        phases = {label: statistics.median(r['phases_ns'].get(label, 0) for r in profiled) for label in {label for r in profiled for label in r['phases_ns']}}
        external = {arm: statistics.median(r['external_ns'] for r in selected if r['arm'] == arm) for arm in binaries}
        posting = phases.get('metadata_postings', 0) + phases.get('body_postings', 0)
        paired.append(dict(query, median_phases_ns=phases, median_external_ns=external, posting_fraction_service=posting / phases['service'], posting_fraction_query=posting / phases['query']))
    write(out / 'comparison.json', paired)
    write(out / 'checks.json', {'queries': len(queries), 'trials': len(rows), 'all_repeat_and_arm_results_equal_except_generation_elapsed': True, 'stability': stable})


if __name__ == '__main__':
    main()
