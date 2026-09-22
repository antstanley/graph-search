#!/usr/bin/env python3
"""Release decision gate: correctness, evidence accuracy, performance and resources.

Combines the existing source-valid evidence protocol with Criterion
(https://docs.rs/criterion/latest/criterion/) benchmarks and a resident resource
probe, then applies predeclared thresholds and writes a decision record.

Objectives are kept separate on purpose:
  * correctness        — workspace tests and source-validated task labels;
  * candidate/evidence — required files, complete regions, mean region coverage;
  * performance        — p50/p95/p99 from Criterion's per-benchmark samples;
  * resource envelope  — index bytes and resident set size for one real repository;
  * model task success — external; this environment has no model driver, so it is
                         reported as not measured and the decision stays conditional.

Thresholds are regression ceilings, not service-level objectives. Accuracy
thresholds are frozen against the recorded calibration
(`results/native-implementation/release-gate-v1/calibration-pre21.json`): the
pre-increment commit a0cbf7d measured 28 required files, 12 complete regions and
0.4862 mean region coverage on the same 34 source-valid tasks, and the
recommendation-21 code measured 28/12/0.4869. Tightening or loosening a threshold
requires a new decision record.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
import platform
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'research' / 'scripts'))
sys.path.insert(0, str(ROOT / 'evaluation'))
from native_review import eligible_tasks
from taskbench.backends import GraphSearch
from taskbench.core import coverage, validate
from taskbench.provenance import implementation, repository_snapshot
from taskbench.runner import sanitized, trial

# ---------------------------------------------------------------------------
# Predeclared thresholds (frozen with this decision record).
# ---------------------------------------------------------------------------
REQUIRED_FILES_MIN = 28
COMPLETE_REGIONS_MIN = 12
MEAN_REGION_COVERAGE_MIN = 0.48
# p95 ceilings in milliseconds for `cargo bench -p graph-search --bench search`.
PERFORMANCE_P95_MS = {
    'build/cold_index': 8_000.0,
    'sync/one_file_edit': 4_000.0,
    'lookup/exact_symbol': 50.0,
    'lookup/references': 50.0,
    'explore/body_multiword': 250.0,
    'explore/metadata_single_term': 100.0,
    'explore/positional_route': 250.0,
    'occurrences/by_name': 100.0,
    'scan/text_literal': 500.0,
    'scan/files_glob': 200.0,
    'scan/filtered_explore': 250.0,
}
# A repeated gate run must not be more than this ratio of a supplied baseline.
BASELINE_TOLERANCE = 2.0


def write(path: Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def run(argv, **kwargs):
    return subprocess.run(argv, cwd=ROOT, check=True, **kwargs)


def capture(argv, timeout=None):
    started = time.monotonic()
    completed = subprocess.run(argv, cwd=ROOT, capture_output=True, timeout=timeout)
    return {
        'argv': [str(item) for item in argv],
        'exit': completed.returncode,
        'seconds': round(time.monotonic() - started, 3),
        'stdout': completed.stdout.decode('utf-8', errors='replace'),
        'stderr': completed.stderr.decode('utf-8', errors='replace'),
    }


def percentiles(samples):
    values = sorted(samples)
    if not values:
        return {}
    def at(fraction):
        index = min(int(fraction * len(values)), len(values) - 1)
        return values[index]
    return {
        'samples': len(values),
        'min_ms': round(values[0], 4),
        'p50_ms': round(at(0.50), 4),
        'p95_ms': round(at(0.95), 4),
        'p99_ms': round(at(0.99), 4),
        'max_ms': round(values[-1], 4),
        'mean_ms': round(statistics.mean(values), 4),
    }


def criterion_measurements(output: Path):
    """Reads Criterion's per-benchmark samples and copies them for provenance."""
    base = ROOT / 'target' / 'criterion'
    measurements, copied = {}, []
    for group in sorted(base.glob('*')):
        for bench in sorted(group.glob('*')):
            sample = bench / 'new' / 'sample.json'
            estimates = bench / 'new' / 'estimates.json'
            if not sample.is_file():
                continue
            data = json.loads(sample.read_text())
            iters, times = data.get('iters', []), data.get('times', [])
            durations = [time_ns / count / 1e6
                         for time_ns, count in zip(times, iters, strict=False) if count]
            name = f'{group.name}/{bench.name}'
            measurements[name] = percentiles(durations)
            if estimates.is_file():
                measurements[name]['criterion_median_ms'] = round(
                    json.loads(estimates.read_text())['median']['point_estimate'] / 1e6, 4)
            destination = output / 'criterion' / group.name / bench.name
            destination.mkdir(parents=True, exist_ok=True)
            shutil.copy2(sample, destination / 'sample.json')
            if estimates.is_file():
                shutil.copy2(estimates, destination / 'estimates.json')
            copied.append(name)
    return measurements, copied


def resident_probe(root: Path, output: Path):
    """One real index build: index bytes, resident set size, setup seconds."""
    target = ROOT / 'research/harness/target'
    run(['cargo', 'build', '--release', '--offline', '--locked', '--manifest-path',
         str(ROOT / 'evaluation/harness/Cargo.toml'), '--target-dir', str(target)])
    host = target / 'release/task-eval-host'
    with tempfile.TemporaryDirectory(prefix='graph-search-gate-store-') as store:
        process = subprocess.Popen(
            [str(host), str(root), store], cwd=ROOT, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, text=True, start_new_session=True)
        try:
            line = process.stdout.readline()
            ready = json.loads(line)
            rss = subprocess.run(['ps', '-o', 'rss=', '-p', str(process.pid)],
                                 capture_output=True, text=True).stdout.strip()
            total = sum(path.stat().st_size for path in Path(store).rglob('*') if path.is_file())
            result = {
                'repository': root.name,
                'setup_ms': ready.get('setup_ms'),
                'indexed_files': ready.get('index', {}).get('coverage', {}).get('admitted_files'),
                'index_bytes': total,
                'resident_kib': int(rss) if rss.isdigit() else None,
                'note': 'resident set size is one process sample after setup, not a peak',
            }
            write(output / 'resources.json', result)
            return result
        finally:
            try:
                os.killpg(os.getpgid(process.pid), 15)
            except ProcessLookupError:
                pass
            process.wait(timeout=30)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--roots', type=Path)
    parser.add_argument('--repeats', type=int, default=1)
    parser.add_argument('--suite', type=Path, help='Optional frozen tasks/oracles directory')
    parser.add_argument('--baseline', type=Path, help='Previous measurements.json for a tolerance check')
    parser.add_argument('--skip-tests', action='store_true')
    parser.add_argument('--skip-lint', action='store_true')
    parser.add_argument('--skip-accuracy', action='store_true')
    parser.add_argument('--skip-bench', action='store_true')
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists():
        parser.error(f'output already exists: {output}')
    output.mkdir(parents=True)
    reasons = []

    roots = ({name: Path(value).resolve()
              for name, value in json.loads(args.roots.read_text()).items()}
             if args.roots else {name: ROOT.parent / name
                                 for name in ('nanus', 'blogwright', 'whatsurvey')})

    # ---------------------------------------------------------------- correctness
    lint: dict = {}
    if args.skip_lint:
        lint = {'status': 'skipped'}
    else:
        captured = capture(['cargo', 'clippy', '--workspace', '--all-targets', '--locked',
                            '--', '-D', 'warnings'])
        (output / 'clippy.log').write_text(captured['stdout'] + captured['stderr'])
        lint = {'status': 'pass' if captured['exit'] == 0 else 'fail',
                'seconds': captured['seconds']}
        if lint['status'] != 'pass':
            reasons.append('strict Clippy failed')

    tests: dict = {}
    if args.skip_tests:
        tests = {'status': 'skipped'}
    else:
        captured = capture(['cargo', 'test', '--workspace', '--locked'])
        passed = sum(int(line.split()[3]) for line in captured['stdout'].splitlines()
                     if line.startswith('test result: ok.'))
        failed = sum(int(line.split()[5]) for line in captured['stdout'].splitlines()
                     if line.startswith('test result: FAILED'))
        (output / 'workspace-tests.log').write_text(captured['stdout'] + captured['stderr'])
        tests = {'status': 'pass' if captured['exit'] == 0 and failed == 0 else 'fail',
                 'passed': passed, 'failed': failed, 'seconds': captured['seconds']}
        if tests['status'] != 'pass':
            reasons.append('workspace tests failed')

    if args.suite:
        tasks = json.loads((args.suite / 'tasks.json').read_text())
        oracles = json.loads((args.suite / 'oracles.json').read_text())
        validate(tasks, oracles, roots)
        excluded = []
    else:
        tasks, oracles, excluded = eligible_tasks(roots)
    validation = {'status': 'pass', 'source_valid_tasks': len(tasks),
                  'excluded': excluded}
    if not tasks:
        validation['status'] = 'fail'
        reasons.append('no source-valid tasks')
    write(output / 'task-validation.json', validation)

    # ------------------------------------------------------- candidate/evidence
    evidence: dict = {'status': 'skipped'}
    if not args.skip_accuracy and tasks:
        target = ROOT / 'research/harness/target'
        run(['cargo', 'build', '--release', '--offline', '--locked', '--manifest-path',
             str(ROOT / 'evaluation/harness/Cargo.toml'), '--target-dir', str(target)])
        host = target / 'release/task-eval-host'
        before = {name: repository_snapshot(root) for name, root in roots.items()}
        rows, errors = [], 0
        for repo, root in roots.items():
            cases = [(task, repeat) for task in tasks if task['repo'] == repo
                     for repeat in range(args.repeats)]
            random.Random(20260919).shuffle(cases)
            backend = GraphSearch(root, host)
            try:
                for task, repeat in cases:
                    result = trial(task, backend)
                    result['evidence'] = coverage(oracles[task['id']],
                                                  {tuple(item) for item in result['seen']})
                    result['repeat'] = repeat
                    rows.append(sanitized(result))
                    errors += int(bool(result.get('errors')) or result.get('status') != 'protocol_complete')
            finally:
                backend.close()
        first = [row for row in rows if row.get('repeat') == 0]
        metrics = {
            'tasks': len(first),
            'required_files': sum(row['evidence']['required_file_recall'] == 1 for row in first),
            'complete_regions': sum(row['evidence']['evidence_ready'] for row in first),
            'mean_region_coverage': round(statistics.mean(
                statistics.mean(row['evidence']['region_coverage'].values()) for row in first), 4),
            'mean_response_bytes': round(statistics.mean(row['response_bytes'] for row in first), 1),
            'protocol_errors': errors,
        }
        write(output / 'evidence-results.json', rows)
        evidence = {'status': 'pass', 'metrics': metrics,
                    'thresholds': {'required_files_min': REQUIRED_FILES_MIN,
                                   'complete_regions_min': COMPLETE_REGIONS_MIN,
                                   'mean_region_coverage_min': MEAN_REGION_COVERAGE_MIN}}
        if errors:
            evidence['status'] = 'fail'
            reasons.append(f'{errors} protocol errors')
        if metrics['required_files'] < REQUIRED_FILES_MIN:
            evidence['status'] = 'fail'
            reasons.append(f"required files {metrics['required_files']} < {REQUIRED_FILES_MIN}")
        if metrics['complete_regions'] < COMPLETE_REGIONS_MIN:
            evidence['status'] = 'fail'
            reasons.append(f"complete regions {metrics['complete_regions']} < {COMPLETE_REGIONS_MIN}")
        if metrics['mean_region_coverage'] < MEAN_REGION_COVERAGE_MIN:
            evidence['status'] = 'fail'
            reasons.append('mean region coverage below threshold')
        after = {name: repository_snapshot(root) for name, root in roots.items()}
        write(output / 'source-stability.json', {'before': before, 'after': after,
                                                 'equal': before == after})
        if before != after:
            evidence['status'] = 'fail'
            reasons.append('external repository changed during the gate')

    # ------------------------------------------------------------- performance
    performance: dict = {'status': 'skipped'}
    if not args.skip_bench:
        captured = capture(['cargo', 'bench', '-p', 'graph-search', '--bench', 'search',
                            '--', '--save-baseline', 'release-gate'])
        (output / 'criterion-output.log').write_text(captured['stdout'] + captured['stderr'])
        measurements, copied = criterion_measurements(output)
        missing = [name for name in PERFORMANCE_P95_MS if name not in measurements]
        violations = [name for name, ceiling in PERFORMANCE_P95_MS.items()
                      if name in measurements and measurements[name]['p95_ms'] > ceiling]
        performance = {'status': 'pass' if not missing and not violations else 'fail',
                       'measurements': measurements,
                       'thresholds_p95_ms': PERFORMANCE_P95_MS,
                       'missing': missing, 'violations': violations}
        if missing:
            reasons.append(f'missing benchmarks: {missing}')
        if violations:
            reasons.append(f'p95 over ceiling: {violations}')
        if args.baseline:
            baseline = json.loads(args.baseline.read_text())['objectives']['performance']['measurements']
            regressions = [name for name, value in measurements.items()
                           if name in baseline
                           and value['p50_ms'] > baseline[name]['p50_ms'] * BASELINE_TOLERANCE]
            performance['baseline_regressions'] = regressions
            if regressions:
                performance['status'] = 'fail'
                reasons.append(f'p50 over {BASELINE_TOLERANCE}x baseline: {regressions}')

    # -------------------------------------------------------- resource envelope
    resources: dict = {'status': 'reported'}
    if not args.skip_accuracy and 'nanus' in roots:
        resources = {'status': 'reported', 'probe': resident_probe(roots['nanus'], output)}

    # ---------------------------------------------------------------- provenance
    provenance = {
        'platform': platform.platform(),
        'python': platform.python_version(),
        'git_head': subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT,
                                   capture_output=True, text=True).stdout.strip(),
        'rustc': subprocess.run(['rustc', '--version'], cwd=ROOT, capture_output=True,
                                text=True).stdout.strip(),
        'criterion': 'https://docs.rs/criterion/latest/criterion/',
        'production_sha256': {
            str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted((ROOT / 'crates').rglob('*.rs'))},
        'gate_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    }
    write(output / 'provenance.json', provenance)

    # ------------------------------------------------------------------ decision
    objectives = {
        'exact_matching_and_correctness': {
            'status': 'fail' if 'fail' in (tests.get('status'), lint.get('status'))
            else ('skipped' if tests.get('status') == 'skipped' and lint.get('status') == 'skipped'
                  else 'pass'),
            'tests': tests, 'lint': lint, 'task_validation': validation},
        'candidate_and_evidence': evidence,
        'performance': performance,
        'resource_envelope': resources,
        'model_task_success': {
            'status': 'not_measured',
            'reason': 'no model driver is available in this environment; the agent '
                      'protocol requires an external model with equal budgets and '
                      'blind grading',
        },
    }
    failed = any(value.get('status') == 'fail' for value in objectives.values())
    decision = 'fail' if failed else 'conditional_pass'
    if not failed and objectives['model_task_success']['status'] == 'pass':
        decision = 'pass'
    record = {'protocol': 'release-gate-v1', 'decision': decision, 'reasons': reasons,
              'objectives': objectives,
              'thresholds': {'required_files_min': REQUIRED_FILES_MIN,
                             'complete_regions_min': COMPLETE_REGIONS_MIN,
                             'mean_region_coverage_min': MEAN_REGION_COVERAGE_MIN,
                             'performance_p95_ms': PERFORMANCE_P95_MS,
                             'baseline_tolerance': BASELINE_TOLERANCE}}
    write(output / 'release-decision.json', record)
    (output / 'RELEASE-GATE.md').write_text(render(record))
    print(f'decision: {decision} ({sum(1 for v in objectives.values() if v.get("status") == "pass")} pass)')
    for reason in reasons:
        print(f'  - {reason}')
    return 1 if failed else 0


def render(record) -> str:
    lines = ['# Release gate decision', '',
             f"**Decision: {record['decision']}**", '']
    if record['reasons']:
        lines += ['Reasons:', ''] + [f'- {reason}' for reason in record['reasons']] + ['']
    for name, objective in record['objectives'].items():
        lines.append(f"## {name}: {objective.get('status')}")
        if name == 'candidate_and_evidence' and 'metrics' in objective:
            lines += ['', '| Metric | Value | Threshold |', '| --- | ---: | ---: |']
            metrics, thresholds = objective['metrics'], objective['thresholds']
            lines += [
                f"| Source-valid tasks | {metrics['tasks']} | — |",
                f"| Required files | {metrics['required_files']} | {thresholds['required_files_min']} |",
                f"| Complete regions | {metrics['complete_regions']} | {thresholds['complete_regions_min']} |",
                f"| Mean region coverage | {metrics['mean_region_coverage']:.4f} | {thresholds['mean_region_coverage_min']} |",
                f"| Mean response bytes | {metrics['mean_response_bytes']} | — |",
                f"| Protocol errors | {metrics['protocol_errors']} | 0 |", '']
        if name == 'performance' and 'measurements' in objective:
            lines += ['', '| Benchmark | p50 ms | p95 ms | p99 ms | p95 ceiling |',
                      '| --- | ---: | ---: | ---: | ---: |']
            for bench, value in objective['measurements'].items():
                ceiling = objective['thresholds_p95_ms'].get(bench)
                lines.append(f"| {bench} | {value['p50_ms']} | {value['p95_ms']} | "
                             f"{value['p99_ms']} | {ceiling if ceiling is not None else '—'} |")
            lines.append('')
        if name == 'resource_envelope' and 'probe' in objective:
            probe = objective['probe']
            lines += ['', f"Index bytes: {probe['index_bytes']}; resident KiB: "
                          f"{probe['resident_kib']}; setup ms: {probe['setup_ms']}. "
                          f"{probe['note']}", '']
        if name == 'model_task_success':
            lines += ['', objective['reason'], '']
    lines += ['Criterion reference: https://docs.rs/criterion/latest/criterion/', '']
    return '\n'.join(lines)


if __name__ == '__main__':
    sys.exit(main())
