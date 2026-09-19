"""Run the frozen 225-query retrieval set without overwriting historical results."""
import json,pathlib,subprocess,tempfile,sys
base=pathlib.Path(__file__).resolve().parents[1]
binary=base/'harness/target/debug/search-research'
results={}
for repo in ['nanus','blogwright','whatsurvey']:
 records=[]
 for group,suffix in [('discovery','queries'),('expanded','expanded-queries'),('natural','natural-queries')]:
  requests=json.loads((base/'results'/f'{repo}-{suffix}.json').read_text())
  if group=='discovery': requests=requests[:10]
  with tempfile.TemporaryDirectory(prefix='lexical-eval-') as td:
   rp=pathlib.Path(td)/'queries.json';rp.write_text(json.dumps(requests))
   op=pathlib.Path(td)/'output.json'
   with op.open('w') as f:subprocess.run([str(binary),str(pathlib.Path.home()/'code'/repo),str(rp)],stdout=f,check=True,timeout=300)
   data=json.loads(op.read_text())
  for entry in data['queries']:
   req=entry['request'];pairs=[{'path':i['node']['path'],'name':i['node']['name']} for i in entry['result'].get('items',[])[:8]]
   rank=next((i+1 for i,p in enumerate(pairs) if p['path']==req['expected_path'] and ('expected_name' not in req or p['name']==req['expected_name'])),None)
   records.append({'group':group,'request':req,'rank':rank,'items':pairs,'elapsed_us':entry['elapsed_us'],'error':entry['result'].get('error')})
 results[repo]=records
 print(repo,{g:sum(r['rank'] is not None for r in records if r['group']==g) for g in ['discovery','expanded','natural']},flush=True)
 (base/'results'/'lexical-followup.json').write_text(json.dumps(results,indent=2)+'\n')
