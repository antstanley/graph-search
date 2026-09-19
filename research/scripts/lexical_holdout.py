"""Freeze and evaluate 20 previously unsampled split-name targets per repository.
Uses retained baseline graph dumps; labels are chosen before querying the new ranker.
"""
import collections,hashlib,json,pathlib,re,subprocess,tempfile
base=pathlib.Path(__file__).resolve().parents[1]
results={}
def tokens(s):
 s=re.sub(r'([a-z0-9])([A-Z])',r'\1 \2',s)
 s=re.sub(r'([A-Z])([A-Z][a-z])',r'\1 \2',s)
 return [t for t in re.findall('[a-z0-9]+',s.lower()) if t not in set('how does the a an is are in to of and where what for with'.split())]
for repo in ['nanus','blogwright','whatsurvey']:
 rp=base/'results'/f'{repo}-heldout-queries.json'
 if not rp.exists():
  data=json.loads(pathlib.Path(f'/private/tmp/{repo}-baseline.json').read_text())
  previous={r['expected_name'] for r in json.loads((base/'results'/f'{repo}-expanded-queries.json').read_text())}
  counts=collections.Counter(n['name'] for n in data['nodes'] if n['kind']=='function')
  pool=[n for n in data['nodes'] if n['kind']=='function' and counts[n['name']]==1 and n['name'] not in previous and len(tokens(n['name']))>=2 and not re.search('(test|spec|fixture|example)',n['path'])]
  chosen=sorted(pool,key=lambda n:hashlib.sha256(('heldout-v1:'+n['id']).encode()).hexdigest())[:20]
  assert len(chosen)==20
  requests=[{'mode':'explore','query':' '.join(tokens(n['name'])),'expected_name':n['name'],'expected_path':n['path']} for n in chosen]
  rp.write_text(json.dumps(requests,indent=2)+'\n')
 with tempfile.TemporaryDirectory() as td:
  op=pathlib.Path(td)/'out.json'
  with op.open('w') as f:subprocess.run([str(base/'harness/target/debug/search-research'),str(pathlib.Path.home()/'code'/repo),str(rp)],stdout=f,check=True,timeout=300)
  data=json.loads(op.read_text())
 rows=[]
 for entry in data['queries']:
  req=entry['request'];pairs=[{'path':i['node']['path'],'name':i['node']['name']} for i in entry['result'].get('items',[])[:8]]
  rank=next((i+1 for i,p in enumerate(pairs) if p['path']==req['expected_path'] and p['name']==req['expected_name']),None)
  rows.append({'request':req,'rank':rank,'items':pairs,'elapsed_us':entry['elapsed_us'],'error':entry['result'].get('error')})
 results[repo]=rows;print(repo,sum(r['rank'] is not None for r in rows),len(rows),flush=True)
 (base/'results/lexical-heldout.json').write_text(json.dumps(results,indent=2)+'\n')
