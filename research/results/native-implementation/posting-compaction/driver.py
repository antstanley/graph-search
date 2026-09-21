"""Compare frozen storage_composition builds, including metadata maintenance.

The two composition captures must already be complete and source-stable. Reuses
their disposable source trees, compiles both before measuring, then alternates
process order. Synthetic ranked-result hashes must match every arm and repeat.
"""
import argparse
import json
from pathlib import Path
import shutil
import statistics
import subprocess

from markdown_context import inventory
from markdown_statistics import digest, write
from native_review import ROOT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('baseline', type=Path)
    parser.add_argument('candidate', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    before = inventory()
    captures = {'baseline': args.baseline.resolve(), 'candidate': args.candidate.resolve()}
    provenance = {arm: json.loads((folder / 'provenance.json').read_text()) for arm, folder in captures.items()}
    for folder in captures.values():
        assert all(json.loads((folder / 'checks.json').read_text())['stability'].values())
    old, new = (provenance[arm]['sources'] for arm in ('baseline', 'candidate'))
    changed = [name for name in sorted(set(old) | set(new))
               if name.startswith('crates/') and old.get(name) != new.get(name)]
    assert changed == ['crates/core/src/body.rs', 'crates/core/src/lexical.rs', 'crates/core/src/lexical_update.rs'], changed
    driver = ROOT / 'research/harness/src/bin/metadata_update_probe.rs'
    source = driver.read_text()
    old_line = 'rows.push(json!({"symbols":count,"mutation":mutation,"pairs":pairs}));'
    assert source.count(old_line) == 1
    source = source.replace(old_line,
        'rows.push(json!({"symbols":count,"mutation":mutation,"pairs":pairs, "ranking_sha256":graph_search_core::hash::content_hash(&serde_json::to_vec(&ranked(&old.updated(changed.clone()))).unwrap())}));')
    binaries = {}
    for arm, record in provenance.items():
        temporary = Path(record['temporary_root'])
        copied = temporary / 'build'
        for name, expected in record['injected'].items():
            assert digest(copied / name) == expected, name
        path = copied / 'research/harness/src/bin/metadata_update_probe.rs'
        path.write_text(source)
        with (out / f'{arm}-build.txt').open('w') as log:
            subprocess.run(['cargo', 'build', '--release', '--offline', '--locked',
                '--manifest-path', str(copied / 'research/harness/Cargo.toml'),
                '--target-dir', str(ROOT / 'research/harness/target'), '--bin', 'metadata_update_probe'],
                cwd=copied, stdout=log, stderr=subprocess.STDOUT, check=True)
        binary = temporary / 'metadata-compaction-probe'
        shutil.copy2(ROOT / 'research/harness/target/release/metadata_update_probe', binary)
        binaries[arm] = binary
    hashes = {arm: digest(binary) for arm, binary in binaries.items()}
    rows = []
    for repeat in range(3):
        for arm in (('baseline', 'candidate') if repeat % 2 == 0 else ('candidate', 'baseline')):
            print(arm, 'maintenance repeat', repeat, flush=True)
            data = json.loads(subprocess.check_output([str(binaries[arm])], cwd=ROOT))
            write(out / f'{arm}-{repeat}.json', data)
            assert data['score_and_order_equal'] and data['previous_generation_unchanged']
            rows.extend(dict(row, arm=arm, repeat=repeat) for row in data['rows'])
    comparisons = []
    for count in (1000, 50000):
        for mutation in sorted({row['mutation'] for row in rows}):
            selected = [row for row in rows if row['symbols'] == count and row['mutation'] == mutation]
            assert len(selected) == 6
            assert len({row['ranking_sha256'] for row in selected}) == 1
            times = {arm: {kind: statistics.median(pair[kind] for row in selected if row['arm'] == arm for pair in row['pairs'])
                           for kind in ('full_ms', 'delta_ms')} for arm in binaries}
            comparisons.append({'symbols': count, 'mutation': mutation, 'median_ms': times})
    write(out / 'maintenance-comparison.json', comparisons)
    checks = {'sources_stable': before == inventory(),
        'binaries_stable': hashes == {arm: digest(binary) for arm, binary in binaries.items()},
        'rankings_equal_across_arms_repeats_full_delta': True}
    write(out / 'checks.json', checks)
    write(out / 'provenance.json', {'capture_paths': {arm: str(path.relative_to(ROOT)) for arm, path in captures.items()},
        'changed_production_files': changed, 'driver_sha256': digest(driver),
        'instrumented_driver': source, 'binaries': hashes, 'sources': before,
        'note': 'Three process repeats per arm; five alternating full/delta pairs per case in each process. Synthetic maintenance only; process repeats are not independent task samples.'})
    assert all(checks.values())


if __name__ == '__main__':
    main()
