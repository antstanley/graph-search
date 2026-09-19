"""Read-only CodeGraph freshness audit and source stability check."""
import pathlib,sqlite3,hashlib,json,collections,platform,subprocess
base=pathlib.Path(__file__).resolve().parents[1];out={};stability={}
for repo in ['nanus','blogwright','whatsurvey']:
 root=pathlib.Path.home()/'code'/repo
 baseline=json.loads((base/'results'/f'{repo}-baseline.json').read_text())
 differences=[]
 for path,digest in baseline.get('walked_file_hashes',baseline['source_hashes']).items():
  try:actual=hashlib.sha256((root/path).read_bytes()).hexdigest()
  except OSError:actual=None
  if digest!=actual:differences.append(path)
 stability[repo]={'checked_walked_files':len(baseline.get('walked_file_hashes',baseline['source_hashes'])),'changed_since_baseline':differences}
 if not (root/'.codegraph').exists():continue
 c=sqlite3.connect(f'file:{root}/.codegraph/codegraph.db?mode=ro',uri=True)
 counts=collections.Counter();examples=[]
 for path,digest in c.execute('select path,content_hash from files'):
  try:state='matching_sha256' if digest==hashlib.sha256((root/path).read_bytes()).hexdigest() else 'different_hash'
  except OSError:state='missing'
  counts[state]+=1
  if state!='matching_sha256' and len(examples)<10:examples.append({'path':path,'state':state})
 out[repo]={'freshness_check':dict(counts),'examples':examples,'languages':dict(c.execute('select language,count(*) from files group by language')),'fts_schema':c.execute("select sql from sqlite_master where name='nodes_fts'").fetchone()[0]}
out['environment']={'platform':platform.platform(),'python':platform.python_version(),'sqlite':sqlite3.sqlite_version,'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'codegraph':subprocess.check_output(['codegraph','--version'],text=True).strip()}
(base/'results/comparator-provenance.json').write_text(json.dumps(out,indent=2)+'\n')
(base/'results/source-stability.json').write_text(json.dumps(stability,indent=2)+'\n')
print(json.dumps(stability))
