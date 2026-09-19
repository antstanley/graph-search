"""Small hand-labelled task-language challenge; labels are intended files, not exhaustive relevance judgements."""
import os
import json,pathlib,re,sqlite3,subprocess,time
base=pathlib.Path(__file__).resolve().parents[1]
sets={
'nanus':[
('Where are old conversation turns removed to stay within the model context window?','crates/nanus-domain/src/context.rs'),
('How does a shell command get classified as destructive before execution?','crates/nanus-bundle/src/guard.rs'),
('Where is a user asked for permission before a tool executes?','crates/nanus-bundle/src/agent_loop.rs'),
('How are file name patterns used to narrow literal text search results?','crates/nanus-bundle/src/tools/grep.rs'),
('How are simultaneous tool calls limited during an agent turn?','crates/nanus-bundle/src/agent_loop.rs')],
'blogwright':[
('Where are draft articles excluded from publishing to the personal data server?','packages/pds/src/content.ts'),
('How are local articles converted into records and sent to the personal data server?','packages/pds/src/sync.ts'),
('Where are stored credentials loaded for an authenticated publishing session?','packages/pds/src/secret.ts'),
('How is a default secret name chosen when a site has no explicit override?','packages/pds/src/config.ts'),
('Where are publication metadata records constructed for the remote server?','packages/pds/src/sync.ts')],
'whatsurvey':[
('How is an incoming WhatsApp request checked for a forged message authentication code?','workspaces/backend/src/core/whatsapp/signature.ts'),
('Where are confidential configuration values encrypted before storage?','workspaces/backend/src/core/settings/crypto.ts'),
('Where is the editable version of a survey saved before it is published?','workspaces/backend/src/core/db/survey-versions.ts'),
('How can an administrator copy the webhook address in the settings screen?','workspaces/frontend/src/lib/components/WhatsAppConfigurations.svelte'),
('Where is the shared credential for internal API authentication read from the secrets service?','workspaces/backend/src/core/auth-internal.ts')]
}
def tokens(s):
 s=re.sub(r'([a-z0-9])([A-Z])',r'\1 \2',s);s=re.sub(r'([A-Z])([A-Z][a-z])',r'\1 \2',s)
 return [x for x in re.findall('[a-z0-9]+',s.lower()) if x not in set('how does the a an is are in to of and where what for with'.split())]
for repo,rows in sets.items():
 root=(pathlib.Path.home()/'code')/repo
 req=[{'mode':'explore','query':q,'expected_path':p} for q,p in rows];rp=base/'results'/f'{repo}-natural-queries.json';rp.write_text(json.dumps(req,indent=2)+'\n')
 runs={}
 for phase,binary in [('baseline','/private/tmp/search-research-baseline'),('fixed',os.environ.get('GRAPH_SEARCH_FIXED_BINARY', str(base/'harness/target/debug/search-research')))]:
  out=pathlib.Path(f'/private/tmp/{repo}-natural-{phase}.json')
  with out.open('w') as f:subprocess.run([binary,str(root),str(rp)],stdout=f,check=True,timeout=300)
  runs[phase]=json.loads(out.read_text())
 symbols=[n for n in runs['baseline']['nodes'] if n['kind']!='file'];cache={};db=sqlite3.connect(':memory:');db.execute('create virtual table docs using fts5(name,path,signature,body)')
 for n in symbols:
  if n['path'] not in cache:
   try:cache[n['path']]=(root/n['path']).read_text().splitlines()
   except (OSError,UnicodeError):cache[n['path']]=[]
  span=n['span'] or {};body='\n'.join(cache[n['path']][max(0,span.get('start_line',1)-1):span.get('end_line',0)])
  db.execute('insert into docs values (?,?,?,?)',tuple(' '.join(tokens(s or '')) for s in [n['name'],n['path'],n['signature'],body]))
 records=[]
 for idx,(q,expected) in enumerate(rows):
  record={'query':q,'expected_path':expected}
  for phase in runs:record[phase]=[i['node']['path'] for i in runs[phase]['queries'][idx]['result'].get('items',[])[:8]]
  match=' OR '.join('"'+t+'"' for t in dict.fromkeys(tokens(q)))
  for tag,scope in [('fts_metadata','{name path signature}: ('+match+')'),('fts_body',match)]:
   found=db.execute('select rowid,bm25(docs,8,2,1,1) score from docs where docs match ? order by score,rowid limit 8',(scope,)).fetchall();record[tag]=[symbols[r[0]-1]['path'] for r in found]
  if (root/'.codegraph').exists():
   p=subprocess.run(['codegraph','explore',q,'-p',str(root)],capture_output=True,text=True,timeout=60);record['codegraph_explore']=re.findall(r'^\*\*`([^`]+)`\*\*',p.stdout,re.M)
  records.append(record)
 (base/'results'/f'{repo}-natural.json').write_text(json.dumps(records,indent=2)+'\n')
 print(repo,{method:sum(r['expected_path'] in r.get(method,[]) for r in records) for method in ['baseline','fixed','fts_metadata','fts_body','codegraph_explore']},flush=True)
