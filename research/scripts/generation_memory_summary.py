#!/usr/bin/env python3
"""Summarize completed lifecycle RSS samples; sums are not physical memory."""
import json
import pathlib
import statistics
import sys
root = pathlib.Path(sys.argv[1])
runs = json.loads((root / 'runs.json').read_text())
validation = json.loads((root / 'validation.json').read_text())
assert validation['runs'] == len(runs) == 24
rows = []
for files in (64, 256):
    for arm in ('none', 'one', 'distinct', 'shared'):
        selected = [r for r in runs if r['files'] == files and r['arm'] == arm]
        assert len(selected) == 3
        for stage in ('writer_ready', 'after_churn_before_manifest', 'after_manifest_checks', 'all_readers_released'):
            samples = [s for r in selected for s in r['samples'] if s['stage'] == stage]
            assert len(samples) == 3
            rows.append(dict(files=files, arm=arm, stage=stage,
                median_writer_rss_bytes=statistics.median(s['writer_rss_bytes'] for s in samples),
                median_summed_reader_rss_bytes=statistics.median(sum(r['rss_bytes'] for r in s['readers']) for s in samples)))
(root / 'summary.json').write_text(json.dumps(rows, indent=2) + '\n')
print('| Files | Reader arm | Writer RSS MiB | Summed reader RSS before check MiB | After check MiB |')
print('|---:|---|---:|---:|---:|')
for row in rows:
    if row['stage'] != 'after_churn_before_manifest':
        continue
    after = next(r for r in rows if r['files'] == row['files'] and r['arm'] == row['arm'] and r['stage'] == 'after_manifest_checks')
    print(f"| {row['files']} | {row['arm']} | {row['median_writer_rss_bytes']/1048576:.2f} | {row['median_summed_reader_rss_bytes']/1048576:.2f} | {after['median_summed_reader_rss_bytes']/1048576:.2f} |")
