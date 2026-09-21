import argparse,hashlib,json,sys,pathlib

parser=argparse.ArgumentParser(description="Inspect raw API package/context byte allocation using frozen paired binaries.")
parser.add_argument("--capture",type=pathlib.Path,required=True)
parser.add_argument("--binaries",type=pathlib.Path,required=True)
parser.add_argument("--all-tasks",action="store_true")
parser.add_argument("--verify-sharing",action="store_true",help="Require equal resolved identities on common result nodes")
args=parser.parse_args()
root=pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0,str(root/'evaluation'))
from taskbench.backends import GraphSearch
from taskbench.provenance import repository_snapshot
from markdown_statistics import indexes
old=args.capture.resolve()
tasks={x['id']:x for x in json.loads((old/'tasks.json').read_text())}
ids=list(tasks) if args.all_tasks else [x['task_id'] for x in json.loads((old/'paired-changes.json').read_text())]
raw=args.binaries.resolve()
build=json.loads((old/'build.json').read_text())
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
assert digest(root/'evaluation/taskbench/backends.py')==build['source_sha256']['evaluation/taskbench/backends.py']
for arm in ['structured','fixed']:
 assert digest(raw/arm)==build['binary_sha256'][arm]
repos=sorted({tasks[t]['repo'] for t in ids})
expected=json.loads((old/'stability.json').read_text())
sources={repo:repository_snapshot(root.parent/repo) for repo in repos}
index_state={repo:indexes(root.parent/repo) for repo in repos}
assert all(sources[repo]==expected['siblings_before'][repo] for repo in repos)
assert all(index_state[repo]==expected['indexes_before'][repo] for repo in repos)
size=lambda x:len(json.dumps(x,ensure_ascii=False,separators=(',',':')).encode())
rows=[]
for repo in repos:
 for arm in ['structured','fixed']:
  b=GraphSearch(root.parent/repo,raw/arm)
  try:
   for tid in ids:
    task=tasks[tid]
    if task['repo']!=repo:continue
    history=json.loads((raw/f'{arm}-{tid}-0.json').read_text())['history']
    query=history[0]['action']['arguments']['query']
    b.process.stdin.write((json.dumps({'query':query,'retrieval':{'ranking':'auto','per_file':0}})+'\n').encode());b.process.stdin.flush();v=b.receive(30)
    assert 'error' not in v,v
    items=[]
    shared=v.get('context',{}).get('packages',{})
    used=set()
    for x in v['items']:
     evidence=x.get('evidence') or {}
     package=evidence.get('package')
     reference=evidence.get('package_ref')
     assert not (package is not None and reference is not None)
     if reference is not None:
      assert reference in shared
      used.add(reference)
     identity=package if package is not None else shared.get(reference)
     if evidence.get('live') or evidence.get('package_scope_incomplete'):
      assert identity is None
     items.append({'node_id':x['node']['id'],'identity':identity,'package_ref':reference,'package_reference_bytes':size(reference)+len(',"package_ref":') if reference is not None else 0,'path':x['node']['path'],'start':x['node']['start_line'],'bytes':size(x),'package_bytes':size(package)+len(',"package":') if package else 0,'snippet':None if not x.get('snippet') else [x['snippet']['start_line'],len(x['snippet']['lines'])], 'excerpts':[{'start':e['snippet']['start_line'],'lines':len(e['snippet']['lines']),'role':e['role'],'bytes':size(e)} for e in x.get('excerpts',[])]})
    assert used==set(shared), 'unreferenced package records survived trimming'
    rows.append({'shared_package_bytes':size(shared)+len(',"packages":') if shared else 0,'shared_packages':shared,'byte_measurement':'UTF-8 compact Python JSON reconstruction of parsed API values; not a literal wire-byte capture','task_id':tid,'arm':arm,'response_bytes':size(v),'items':items,'truncations':v['truncations'],'stats':v['stats'],'other_metadata_bytes':size({k:x for k,x in v.items() if k!='items'})})
  finally:b.close()
common_nodes=0
if args.verify_sharing:
 paired={(row['task_id'],row['arm']):row for row in rows}
 for tid in ids:
  actual={item['node_id']:item['identity'] for item in paired[(tid,'structured')]['items']}
  expected_identities={item['node_id']:item['identity'] for item in paired[(tid,'fixed')]['items']}
  for node in actual.keys() & expected_identities.keys():
   assert actual[node]==expected_identities[node], (tid,node)
   common_nodes+=1
checks={
 'source_snapshots_match_capture':True,
 'index_snapshots_match_capture':True,
 'binaries_match_capture':all(digest(raw/arm)==build['binary_sha256'][arm] for arm in ['structured','fixed']),
 'sources_stable':sources=={repo:repository_snapshot(root.parent/repo) for repo in repos},
 'indexes_stable':index_state=={repo:indexes(root.parent/repo) for repo in repos},
 'backend_matches_capture':digest(root/'evaluation/taskbench/backends.py')==build['source_sha256']['evaluation/taskbench/backends.py'],
}
assert all(checks.values()),checks
(old/'raw-budget-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
(old/'raw-identity-validation.json').write_text(json.dumps({
 'responses':len(rows),'all_references_resolve':True,'no_orphan_entries':True,
 'common_node_identities_verified':common_nodes if args.verify_sharing else None,
 'probe_sha256':digest(pathlib.Path(__file__)),
},indent=2)+'\n')
(old/'raw-budget-breakdown.json').write_text(json.dumps(rows,indent=2)+'\n')
print('captured',len(rows),'responses')
for x in rows:print(x['task_id'],x['arm'],x['response_bytes'],'metadata',x['other_metadata_bytes'],'packages',sum(i['package_bytes'] for i in x['items']),'excerpts',sum(len(i['excerpts']) for i in x['items']),'windows',x['stats'].get('context_windows_examined'))
