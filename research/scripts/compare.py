"""Ablations over identical graph-search candidates; existing CodeGraph is a separate comparator."""
import collections,json,pathlib,re,sqlite3,subprocess,time,hashlib,sys
base=pathlib.Path(__file__).resolve().parents[1]
stop=set('how does the a an is are in to of and where what for with added selected verified'.split())
def terms(s):
 s=re.sub(r'([a-z0-9])([A-Z])',r'\1 \2',s)
 s=re.sub(r'([A-Z])([A-Z][a-z])',r'\1 \2',s)
 return [t for t in re.findall(r'[a-z0-9]+',s.lower()) if t not in stop]
def query(s):return ' OR '.join('"'+t+'"' for t in dict.fromkeys(terms(s))) or '"__empty__"'
def rank(paths,expected):return next((i+1 for i,p in enumerate(paths[:8]) if p==expected),None)
for repo in sys.argv[1:] or ['nanus','blogwright','whatsurvey']:
 root=(pathlib.Path.home()/'code')/repo
 data=json.loads(pathlib.Path(f'/private/tmp/{repo}-baseline.json').read_text())
 db=sqlite3.connect(':memory:')
 db.execute('create virtual table docs using fts5(name,path,signature,body)')
 symbols=[n for n in data['nodes'] if n['kind']!='file']
 file_cache={}
 for n in symbols:
  p=n['path']
  if p not in file_cache:
   try:file_cache[p]=(root/p).read_text().splitlines()
   except (OSError,UnicodeError):file_cache[p]=[]
  span=n['span'] or {}; lines=file_cache[p];body='\n'.join(lines[max(0,span.get('start_line',1)-1):span.get('end_line',0)])
  db.execute('insert into docs values (?,?,?,?)',(' '.join(terms(n.get('name') or '')),' '.join(terms(p)),' '.join(terms(n.get('signature') or '')),' '.join(terms(body))))
 cg=None
 if (root/'.codegraph').exists():cg=sqlite3.connect(f'file:{root}/.codegraph/codegraph.db?mode=ro',uri=True)
 out=[]
 for entry in data['queries'][:10]:
  req=entry['request'];q=req['query'];expect=req['expected_path']
  paths=[i['node']['path'] for i in entry['result'].get('items',[])]
  result={'query':q,'expected_path':expect,'category':req['category'],'baseline':{'paths':paths,'rank':rank(paths,expect),'elapsed_us':entry['elapsed_us']}}
  for tag,weights in [('fts_metadata',(8,2,1,0)),('fts_body',(8,2,1,1))]:
   now=time.perf_counter(); rows=db.execute('select rowid,bm25(docs,?,?,?,?) as score from docs where docs match ? order by score,rowid limit 8',(*weights,query(q))).fetchall()
   # Weight zero still matches body, so metadata ablation needs column restriction.
   if tag=='fts_metadata': rows=db.execute('select rowid,bm25(docs,8,2,1,0) score from docs where docs match ? order by score,rowid limit 8',('{name path signature}: ('+query(q)+')',)).fetchall()
   pp=[symbols[r[0]-1]['path'] for r in rows]
   result[tag]={'paths':pp,'rank':rank(pp,expect),'elapsed_us':round((time.perf_counter()-now)*1e6)}
  if cg:
   raw=' OR '.join('"'+x+'"' for x in re.findall(r'\w+',q) if x.lower() not in stop) or '"__empty__"'
   rows=cg.execute('select n.file_path,n.name,bm25(nodes_fts) as score from nodes_fts join nodes n on n.rowid=nodes_fts.rowid where nodes_fts match ? order by score limit 8',(raw,)).fetchall()
   pp=[r[0] for r in rows];result['codegraph_raw_fts']={'paths':pp,'rank':rank(pp,expect)}
   for mode in ['query','explore']:
    cmd=['codegraph',mode,q,'-p',str(root)]+(['--json','--limit','8'] if mode=='query' else [])
    now=time.perf_counter()
    try:
     p=subprocess.run(cmd,capture_output=True,text=True,timeout=45)
     if mode=='query':
      obj=json.loads(p.stdout);pp=[]
      def visit(v):
       if isinstance(v,dict):
        for k,x in v.items():
         if k in ('filePath','file_path','path') and isinstance(x,str):pp.append(x)
         else:visit(x)
       elif isinstance(v,list):
        for x in v:visit(x)
      visit(obj)
     else: pp=re.findall(r'^\*\*`([^`]+)`\*\*',p.stdout,re.M)
     result['codegraph_'+mode]={'paths':pp,'rank':rank(pp,expect),'elapsed_ms':round((time.perf_counter()-now)*1000),'exit_code':p.returncode,'output_bytes':len(p.stdout.encode())}
    except (subprocess.TimeoutExpired,json.JSONDecodeError) as e:result['codegraph_'+mode]={'error':type(e).__name__}
  out.append(result)
  (base/'results'/f'{repo}-retrieval.json').write_text(json.dumps(out,indent=2)+'\n')
 counts=collections.Counter((e['kind'],e['resolved']) for e in data['edges'])
 manifest={ 'repo':repo,'revision':subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip(),'dirty':bool(subprocess.check_output(['git','-C',str(root),'status','--porcelain'],text=True)), 'file_owned_calls':sum(e['kind']=='calls' and e['from'].startswith('file:') for e in data['edges']),'nodes':len(data['nodes']),'files':sum(n['kind']=='file' for n in data['nodes']),'edges':len(data['edges']),'edge_counts':{k+('_resolved' if r else '_unresolved'):v for (k,r),v in counts.items()},'index_ms':data['report']['elapsed_ms'],'quarantined':data['report']['quarantined'],'walked_file_hashes':{n['path']:n['content_hash'] for n in data['nodes'] if n['kind']=='file'},'source_hashes':{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in sorted(file_cache)},'queries':data['queries']}
 if cg:manifest['codegraph']={'nodes':cg.execute('select count(*) from nodes').fetchone()[0],'edges':cg.execute('select count(*) from edges').fetchone()[0],'files':cg.execute('select count(*) from files').fetchone()[0], 'max_indexed_at':cg.execute('select max(indexed_at) from files').fetchone()[0]}
 # Strip snippets/signatures of external code; keep IDs, paths, relationships and metrics.
 def scrub(v):
  if isinstance(v,dict):return {k:scrub(x) for k,x in v.items() if k not in ('snippet','signature')}
  if isinstance(v,list):return [scrub(x) for x in v]
  return v
 (base/'results'/f'{repo}-baseline.json').write_text(json.dumps(scrub(manifest),indent=2)+'\n')
 print(repo,{method:sum(bool(x.get(method,{}).get('rank')) for x in out) for method in ['baseline','fts_metadata','fts_body','codegraph_raw_fts','codegraph_query','codegraph_explore']},flush=True)
