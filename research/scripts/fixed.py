"""Collect patched-engine measurements in the same format as discovery."""
import os
import json,pathlib,subprocess,collections
base=pathlib.Path(__file__).resolve().parents[1]
for repo in ['semantics','nanus','blogwright','whatsurvey']:
 root=base/'fixtures/semantics' if repo=='semantics' else (pathlib.Path.home()/'code')/repo
 out=pathlib.Path('/private/tmp')/f'{repo}-fixed.json'
 with out.open('w') as f:subprocess.run([os.environ.get('GRAPH_SEARCH_FIXED_BINARY', str(base/'harness/target/debug/search-research')),str(root),str(base/'results'/f'{repo}-queries.json')],stdout=f,check=True,timeout=300)
 data=json.loads(out.read_text())
 def scrub(v):
  if isinstance(v,dict):return {k:scrub(x) for k,x in v.items() if k not in ('snippet','signature')}
  if isinstance(v,list):return [scrub(x) for x in v]
  return v
 counts=collections.Counter((e['kind'],e['resolved']) for e in data['edges']);calls=[e for e in data['edges'] if e['kind']=='calls']
 summary={'repo':repo,'nodes':len(data['nodes']),'files':sum(n['kind']=='file' for n in data['nodes']),'edges':len(data['edges']),'index_ms':data['report']['elapsed_ms'],'quarantined':data['report']['quarantined'],'file_owned_calls':sum(e['from'].startswith('file:') for e in calls),'edge_counts':{k+('_resolved' if r else '_unresolved'):v for (k,r),v in counts.items()},'queries':scrub(data['queries'])}
 (base/'results'/f'{repo}-fixed.json').write_text(json.dumps(summary,indent=2)+'\n')
 print(repo,summary['nodes'],summary['edges'],summary['file_owned_calls'],flush=True)
