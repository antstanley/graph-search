"""Exercise CLI-specific forwarding, validation and envelope budget contracts."""
import json,pathlib,subprocess,tempfile
base=pathlib.Path(__file__).resolve().parents[1]
binary=(base.parent/'target/debug/graph-search').resolve()
with tempfile.TemporaryDirectory(prefix='graph-search-cli-check-') as tmp:
 root=pathlib.Path(tmp)/'root';root.mkdir();store=pathlib.Path(tmp)/'store'
 (root/'a.rs').write_text('fn leaf() {} fn entry() { leaf(); }\n')
 (root/'.hidden.rs').write_text('fn hidden() {}\n')
 prefix=[str(binary),'--root',str(root),'--store',str(store),'--json']
 checks=[]
 def invoke(args):return subprocess.run(prefix+args,capture_output=True,text=True,check=False)
 p=invoke(['search','explore','leaf','--max-bytes','1000']);assert p.returncode==0,p.stderr;json.loads(p.stdout);assert len(p.stdout.encode())<=1000
 checks.append({'check':'explore CLI entire JSON <= 1000 bytes','bytes':len(p.stdout.encode()),'passed':True})
 p=invoke(['search','symbol','leaf','--lang','not-a-language']);assert p.returncode==2
 checks.append({'check':'invalid CLI language rejected','exit_code':p.returncode,'passed':True})
 p=invoke(['--hidden','search','files','*.rs']);assert p.returncode==0;assert '.hidden.rs' in p.stdout
 checks.append({'check':'CLI files forwards hidden flag','passed':True})
 (base/'results/cli-smoke.json').write_text(json.dumps(checks,indent=2)+'\n')
 print(json.dumps(checks))
