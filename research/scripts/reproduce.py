"""Reproduce locally against ~/code/{nanus,blogwright,whatsurvey}.
Builds an isolated archived baseline; no external repository is indexed or changed.
"""
import pathlib,subprocess,tempfile,shutil,tarfile
repo=pathlib.Path(__file__).resolve().parents[2]
research=repo/'research'
def run(args,**kw):
 print('+',' '.join(map(str,args)),flush=True)
 return subprocess.run(list(map(str,args)),check=True,cwd=repo,**kw)
with tempfile.TemporaryDirectory(prefix='graph-search-baseline-') as tmp:
 baseline=pathlib.Path(tmp)
 archive=baseline/'baseline.tar'
 with archive.open('wb') as f:run(['git','archive','4dd6af6'],stdout=f)
 with tarfile.open(archive) as t:t.extractall(baseline,filter='data')
 shutil.copytree(research/'harness',baseline/'research/harness',ignore=shutil.ignore_patterns('target'))
 run(['cargo','build','--locked','--offline','--manifest-path',baseline/'research/harness/Cargo.toml'])
 shutil.copy2(baseline/'research/harness/target/debug/search-research','/private/tmp/search-research-baseline')
run(['python3',research/'scripts/prepare.py'])
for name in ['semantics','nanus','blogwright','whatsurvey']:
 root=research/'fixtures/semantics' if name=='semantics' else pathlib.Path.home()/'code'/name
 with open(f'/private/tmp/{name}-baseline.json','w') as f:run(['/private/tmp/search-research-baseline',root,research/'results'/f'{name}-queries.json'],stdout=f)
shutil.copy2('/private/tmp/semantics-baseline.json',research/'results/semantics-baseline.json')
run(['python3',research/'scripts/compare.py'])
run(['python3',research/'scripts/expanded.py','baseline'])
run(['cargo','build','--locked','--offline','--manifest-path',research/'harness/Cargo.toml'])
for args in [['fixed.py'],['expanded.py','fixed'],['natural.py'],['application_edges.py'],['provenance.py'],['summarize.py']]:
 run(['python3',research/'scripts'/args[0],*args[1:]])
