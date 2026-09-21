"""Measure native index composition in a disposable instrumented build.

No source text leaves the temporary stores. No production or sibling index edits.
Vector payload/capacity is exact for this ABI; JSON size is NOT heap size. RSS is
sampled after open and before diagnostic allocation. Codec sizes are lower bounds,
not an implementation or speed prediction. Requires macOS /usr/bin/time -l.
"""
from __future__ import annotations

import argparse
import json
import platform
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

from markdown_context import inventory
from markdown_statistics import digest, indexes, write
from native_review import ROOT
from taskbench.provenance import repository_snapshot


def inputs():
    result = inventory()
    for folder in (ROOT / 'research/instrumentation/storage',):
        for path in folder.glob('*.rs'):
            result[str(path.relative_to(ROOT))] = digest(path)
    for path in (ROOT / 'research/instrumentation/posting-codec').glob('*.rs'):
        result[str(path.relative_to(ROOT))] = digest(path)
    result['research/scripts/storage_composition.py'] = digest(Path(__file__))
    for name in ('research/harness/Cargo.toml', 'research/harness/Cargo.lock'):
        result[name] = digest(ROOT / name)
    return result


def disk(store):
    groups = {}
    inodes = set()
    allocated = 0
    for path in store.rglob('*'):
        if not path.is_file():
            continue
        stat = path.stat()
        category = path.suffix or path.name
        group = groups.setdefault(category, {'files': 0, 'logical_bytes': 0})
        group['files'] += 1
        group['logical_bytes'] += stat.st_size
        identity = (stat.st_dev, stat.st_ino)
        if identity not in inodes:
            allocated += stat.st_blocks * 512
            inodes.add(identity)
    return {'by_extension': groups, 'unique_inode_allocated_bytes': allocated,
            'note': 'stat blocks; clone sharing and filesystem metadata not measured'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--posting-codec', action='store_true',
                        help='measure native ordinal delta codec in the disposable build')
    parser.add_argument('--skip-process-memory', action='store_true',
                        help='omit ps/time process-memory measurements (record null, never zero)')
    args = parser.parse_args()
    if platform.system() != 'Darwin' and not args.skip_process_memory:
        parser.error('RSS/peak parser currently requires macOS')
    if args.repeats < 3:
        parser.error('use at least three fresh open processes')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    before = inputs()
    roots = {name: ROOT.parent / name for name in ('nanus', 'blogwright', 'whatsurvey')}
    siblings = {name: repository_snapshot(root) for name, root in roots.items()}
    codegraphs = {name: indexes(root) for name, root in roots.items()}
    temporary = Path(tempfile.mkdtemp(prefix='graph-search-storage-composition-'))
    copied = temporary / 'build'
    for name in before:
        if name.startswith(('crates/', '.cargo/')) or name in (
                'Cargo.toml', 'Cargo.lock', 'research/harness/Cargo.toml', 'research/harness/Cargo.lock'):
            destination = copied / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, destination)
    injected = {}
    templates = ROOT / 'research/instrumentation/storage'
    for module in ('lexical', 'body', 'metadata', 'adjacency', 'store'):
        crate = 'engine' if module == 'store' else 'core'
        path = copied / f'crates/{crate}/src/{module}.rs'
        instrumentation = (templates / f'{module}.rs').read_text()
        if args.posting_codec:
            instrumentation = instrumentation.replace('crate::research_storage::deltas(',
                                                       'crate::research_codec::measure(')
        path.write_text(path.read_text() + '\n' + instrumentation)
        injected[str(path.relative_to(copied))] = digest(path)
    path = copied / 'crates/core/src/research_storage.rs'
    shutil.copy2(templates / 'common.rs', path)
    injected[str(path.relative_to(copied))] = digest(path)
    path = copied / 'crates/core/src/lib.rs'
    path.write_text(path.read_text() + '\n#[doc(hidden)]\npub mod research_storage;\n')
    if args.posting_codec:
        for source, module in [('codec.rs', 'research_codec_impl'), ('metrics.rs', 'research_codec')]:
            destination = copied / f'crates/core/src/{module}.rs'
            shutil.copy2(ROOT / 'research/instrumentation/posting-codec' / source, destination)
            injected[str(destination.relative_to(copied))] = digest(destination)
            path.write_text(path.read_text() + f'\n#[doc(hidden)]\npub mod {module};\n')
    injected[str(path.relative_to(copied))] = digest(path)
    path = copied / 'research/harness/src/bin/storage_composition_probe.rs'
    path.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(templates / 'probe.rs', path)
    injected[str(path.relative_to(copied))] = digest(path)
    print('Temporary stores/build:', temporary, flush=True)
    target = ROOT / 'research/harness/target'
    with (out / 'build.txt').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--locked',
            '--manifest-path', str(copied / 'research/harness/Cargo.toml'),
            '--target-dir', str(target), '--bin', 'storage_composition_probe'],
            cwd=copied, stdout=log, stderr=subprocess.STDOUT, check=True)
    binary = temporary / 'storage-composition'
    shutil.copy2(target / 'release/storage_composition_probe', binary)
    binary_hash = digest(binary)
    write(out / 'provenance.json', {'sources': before, 'injected': injected,
        'binary_sha256': binary_hash, 'temporary_root': str(temporary),
        'platform': platform.platform(), 'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
        'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'posting_codec': args.posting_codec, 'process_memory_measured': not args.skip_process_memory})
    for name, root in roots.items():
        store = temporary / name
        build = json.loads(subprocess.check_output([str(binary), 'build', str(root), str(store)], cwd=ROOT))
        rows = []
        for repeat in range(args.repeats):
            log_path = out / f'{name}-{repeat}-time.txt'
            with log_path.open('w') as stderr:
                command = [str(binary), 'open', str(root), str(store)]
                if not args.skip_process_memory:
                    command = ['/usr/bin/time', '-l', *command]
                child = subprocess.Popen(command,
                    cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, text=True)
                try:
                    ready = json.loads(child.stdout.readline())
                    assert ready['ready']
                    # The child is blocked on stdin, before any diagnostic allocations.
                    rss_bytes = None if args.skip_process_memory else 1024 * int(
                        subprocess.check_output(['ps', '-o', 'rss=', '-p', str(ready['pid'])], text=True))
                    child.stdin.write('measure\n')
                    child.stdin.flush()
                    tail, _ = child.communicate(timeout=300)
                    assert child.returncode == 0, log_path.read_text()
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait()
            data = json.loads(tail)
            if args.posting_codec:
                write(out / f'{name}-{repeat}-codec.json', data)
                # Deterministic composition is checked separately from kernel timing.
                def without_timing(value):
                    if isinstance(value, dict):
                        return {k: without_timing(v) for k, v in value.items() if k != 'timings'}
                    if isinstance(value, list):
                        return [without_timing(v) for v in value]
                    return value
                data = without_timing(data)
            peak = re.search(r'(\d+)\s+maximum resident set size', log_path.read_text())
            assert args.skip_process_memory or peak, log_path
            rows.append({'repeat': repeat, 'open_ns': ready['open_ns'],
                'rss_before_diagnostics_bytes': rss_bytes,
                'peak_including_diagnostics_bytes': int(peak[1]) if peak else None})
            if repeat == 0:
                write(out / f'{name}-composition.json', data)
            else:
                assert data == json.loads((out / f'{name}-composition.json').read_text())
        write(out / f'{name}-process.json', {'build': build, 'opens': rows, 'disk': disk(store)})
        print(name, 'composition stable across', args.repeats, 'fresh processes', flush=True)
    checks = {'sources': before == inputs(), 'binary': binary_hash == digest(binary),
        'siblings': siblings == {name: repository_snapshot(root) for name, root in roots.items()},
        'codegraph_indexes': codegraphs == {name: indexes(root) for name, root in roots.items()}}
    write(out / 'checks.json', {'stability': checks, 'siblings': siblings,
        'codegraph_indexes': codegraphs, 'composition_repeat_equality': True})
    assert all(checks.values()), checks


if __name__ == '__main__':
    main()
