#!/usr/bin/env python3
"""Build paired native queries, then measure sequentially after other checks stop."""
import hashlib
import json
import os
import pathlib
import shutil
import statistics
import subprocess
import sys
import tempfile

ROOT=pathlib.Path(__file__).resolve().parents[2]
OUT=ROOT/'research/results/native-implementation/caller-counts'
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
if sys.argv[1]=='build':
 before=pathlib.Path(json.loads((OUT/'before.json').read_text())['source_copy'])
 binaries=pathlib.Path(tempfile.mkdtemp(prefix='graph-caller-binaries-'))
 environment=dict(os.environ,CARGO_TARGET_DIR=str(ROOT/'research/harness/target'))
 records={}
 for label,source in [('control',before),('candidate',ROOT)]:
  with (OUT/(label+'-build.log')).open('w') as log:
   subprocess.run(['cargo','build','--release','--offline','--locked','--manifest-path',str(source/'research/harness/Cargo.toml'),'--bin','caller_count_probe'],env=environment,stdout=log,stderr=subprocess.STDOUT,check=True)
  target=binaries/label;shutil.copyfile(ROOT/'research/harness/target/release/caller_count_probe',target);target.chmod(0o755)
  records[label]=dict(path=str(target),sha256=digest(target))
 shutil.copyfile(before/'crates/core/src/query.rs',OUT/'query-before.rs')
 (OUT/'binaries.json').write_text(json.dumps(records,indent=2)+'\n')
 print('Both paired binaries built.')
elif sys.argv[1]=='measure':
 records=json.loads((OUT/'binaries.json').read_text());rows=[]
 for record in records.values():assert digest(pathlib.Path(record['path']))==record['sha256']
 for repeat in range(7):
  for label in (['control','candidate'] if repeat%2==0 else ['candidate','control']):
   values=json.loads(subprocess.check_output([records[label]['path'],'20'],text=True))
   rows.extend(dict(arm=label,repeat=repeat,**row) for row in values)
   (OUT/'trials.json').write_text(json.dumps(rows,indent=2)+'\n')
 summary=[]
 for key in sorted({(r['shape'],r['nodes'],r['seeds'],r['payload']) for r in rows}):
  group=[r for r in rows if (r['shape'],r['nodes'],r['seeds'],r['payload'])==key]
  assert len({r['result_sha256'] for r in group})==1, key
  control=statistics.median(r['native_ns']/r['repeats'] for r in group if r['arm']=='control')
  candidate=statistics.median(r['native_ns']/r['repeats'] for r in group if r['arm']=='candidate')
  single=statistics.median(r['independent_ns']/r['repeats'] for r in group)
  shared=statistics.median(r['shared_ns']/r['repeats'] for r in group)
  summary.append(dict(shape=key[0],nodes=key[1],seeds=key[2],payload=key[3],control_median_ns=control,candidate_median_ns=candidate,candidate_over_control=candidate/control,shared_over_independent=shared/single,independent_arcs=group[0]['independent_arcs'],shared_arcs=group[0]['shared_arcs'],result_sha256=group[0]['result_sha256']))
 for record in records.values():assert digest(pathlib.Path(record['path']))==record['sha256']
 (OUT/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
 print(json.dumps(summary,indent=2))
else:
 raise SystemExit('Expected build or measure')
