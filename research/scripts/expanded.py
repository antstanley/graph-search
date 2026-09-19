"""Deterministically sample production functions for exact/split-identifier retrieval."""
import json,pathlib,hashlib,re,subprocess,sqlite3,collections,sys,time
base=pathlib.Path(__file__).resolve().parents[1]
stop=set('how does the a an is are in to of and where what for with'.split())
def tokens(s):
 s=re.sub(r'([a-z0-9])([A-Z])',r'\1 \2',s);s=re.sub(r'([A-Z])([A-Z][a-z])',r'\1 \2',s)
 return [x for x in re.findall('[a-z0-9]+',s.lower()) if x not in stop]
phase=sys.argv[1] if len(sys.argv)>1 else 'baseline'
binary='/private/tmp/search-research-baseline' if phase=='baseline' else str(base/'harness/target/debug/search-research')
for repo in ['nanus','blogwright','whatsurvey']:
 root=(pathlib.Path.home()/'code')/repo
 original=json.loads(pathlib.Path(f'/private/tmp/{repo}-baseline.json').read_text())
 names=collections.Counter(n['name'] for n in original['nodes'] if n['kind']=='function')
 pool=[n for n in original['nodes'] if n['kind']=='function' and names[n['name']]==1 and len(tokens(n['name']))>=2 and not re.search(r'(test|spec|fixture|example)',n['path'])]
 chosen=sorted(pool,key=lambda n:hashlib.sha256(n['id'].encode()).hexdigest())[:30]
 requests=[{'mode':'explore','query':q,'expected_path':n['path'],'expected_name':n['name'],'category':tag} for n in chosen for q,tag in [(n['name'],'exact'),(' '.join(tokens(n['name'])),'split')]]
 requests_path=base/'results'/f'{repo}-expanded-queries.json';requests_path.write_text(json.dumps(requests,indent=2)+'\n')
 outpath=pathlib.Path(f'/private/tmp/{repo}-expanded-{phase}.json')
 with outpath.open('w') as f:subprocess.run([binary,str(root),str(requests_path)],stdout=f,check=True,timeout=300)
 data=json.loads(outpath.read_text()); symbols=[n for n in data['nodes'] if n['kind']!='file']
 db=sqlite3.connect(':memory:');db.execute('create virtual table docs using fts5(name,path,signature)')
 for n in symbols:db.execute('insert into docs values (?,?,?)',tuple(' '.join(tokens(n.get(k) or '')) for k in ['name','path','signature']))
 results=[]
 cg=sqlite3.connect(f'file:{root}/.codegraph/codegraph.db?mode=ro',uri=True) if (root/'.codegraph').exists() else None
 for entry in data['queries']:
  req=entry['request'];items=entry['result'].get('items',[])
  pairs=[(i['node']['path'],i['node']['name']) for i in items[:8]]
  record={'request':req,'elapsed_us':entry['elapsed_us'],'error':entry['result'].get('error'),'graph_search':pairs}
  match=' OR '.join('"'+t+'"' for t in tokens(req['query']))
  rows=db.execute('select rowid,bm25(docs,8,2,1) as score from docs where docs match ? order by score,rowid limit 8',(match,)).fetchall()
  record['fts_metadata']=[(symbols[r[0]-1]['path'],symbols[r[0]-1]['name']) for r in rows]
  if cg:
   match=' OR '.join('"'+t+'"' for t in re.findall(r'\w+',req['query']))
   rows=cg.execute('select n.file_path,n.name,bm25(nodes_fts) as score from nodes_fts join nodes n on n.rowid=nodes_fts.rowid where nodes_fts match ? order by score limit 8',(match,)).fetchall();record['codegraph_raw_fts']=[r[:2] for r in rows]
  results.append(record)
 (base/'results'/f'{repo}-expanded-{phase}.json').write_text(json.dumps(results,indent=2)+'\n')
 print(repo,phase,[(tag,method,sum(any(p==r['request']['expected_path'] and n==r['request']['expected_name'] for p,n in r[method]) for r in results if r['request']['category']==tag)) for tag in ['exact','split'] for method in ['graph_search','fts_metadata']],flush=True)
