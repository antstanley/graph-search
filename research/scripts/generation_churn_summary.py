#!/usr/bin/env python3
"""Summarize completed churn captures without treating timings as causal wins."""
import json,pathlib,statistics,sys
root=pathlib.Path(sys.argv[1]); runs=json.loads((root/'runs.json').read_text())
validation=json.loads((root/'validation.json').read_text()); assert validation['runs']==len(runs)==24
rows=[]
for files in (64,256):
    for arm in ('none','one','distinct','shared'):
        selected=[r for r in runs if r['files']==files and r['arm']==arm]; assert len(selected)==3
        states=[s for r in selected for s in r['rows']]
        row=dict(files=files,arm=arm,maximum_generations=max(len(s['generations']) for s in states),median_sync_ms=statistics.median(s['sync_ns']/1e6 for s in states if 1<=s['step']<=24),median_final_logical_bytes=statistics.median(r['rows'][24]['logical_bytes'] for r in selected),median_final_unique_inode_bytes=statistics.median(r['rows'][24]['unique_inode_bytes'] for r in selected),median_final_allocated_inode_bytes=statistics.median(r['rows'][24]['allocated_inode_bytes'] for r in selected))
        rows.append(row)
(root/'summary.json').write_text(json.dumps(rows,indent=2)+'\n')
print('| Files | Reader arm | Max generations | Median sync ms | Step 24 unique inode MiB |')
print('|---:|---|---:|---:|---:|')
for r in rows: print(f"| {r['files']} | {r['arm']} | {r['maximum_generations']} | {r['median_sync_ms']:.2f} | {r['median_final_unique_inode_bytes']/1048576:.2f} |")
