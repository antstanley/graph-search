import pathlib,subprocess,json,hashlib,re,sys,difflib
root=pathlib.Path.cwd();out=root/'research/results/native-implementation/typescript-inheritance'
def sources(): return {str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((root/'crates').rglob('*')) if p.is_file()}
before=sources();checks={}
drivers=['research/scripts/typescript_inheritance_oracle.py','research/harness/src/bin/typescript_inheritance_probe.rs'];driver_before={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in drivers}
commands=[('clippy',['cargo','clippy','--workspace','--all-targets','--','-D','warnings']),('probe-build',['cargo','build','--release','--offline','--locked','--manifest-path','research/harness/Cargo.toml','--bin','typescript_inheritance_probe']),('oracle',['python3','research/scripts/typescript_inheritance_oracle.py',str(out)]),('workspace',['cargo','test','--workspace']),('format',['cargo','fmt','--all','--','--check']),('diff',['git','diff','--check'])]
for name,command in commands:
 with (out/(name+'.txt')).open('w') as log: result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 checks[name+'_exit']=result.returncode
 (out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
 print(name,result.returncode,flush=True)
 if result.returncode: sys.exit(result.returncode)
assert before==sources()
assert driver_before=={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in drivers}
checks['driver_hashes_match']=len(drivers)
checks['crate_hashes_match']=len(before)
suites=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed;', (out/'workspace.txt').read_text())
checks.update(workspace_passed=sum(int(x) for x,y in suites),workspace_failed=sum(int(y) for x,y in suites),workspace_suites=len(suites),source_version=14,parser_version=19)
checks['no_new_dependencies']=subprocess.run(['git','diff','--exit-code','--','Cargo.toml','Cargo.lock','crates/*/Cargo.toml'],stdout=subprocess.DEVNULL).returncode==0
oracle=json.loads((out/'oracle.json').read_text());checks['oracle_cases']=len(oracle['cases']);checks['sibling_sources_unchanged']=json.loads((out/'environment.json').read_text())['sibling_sources_unchanged']
prior=json.loads((out/'before.json').read_text());base=pathlib.Path(prior['source_copy']);patch=[];changed=[]
for name in sorted(set(before)|set(prior['crate_sha256'])):
 p=root/name;q=base/name
 a=q.read_text() if q.exists() else '';b=p.read_text() if p.exists() else ''
 if a!=b: changed.append(name);patch.extend(difflib.unified_diff(a.splitlines(keepends=True),b.splitlines(keepends=True),fromfile='before/'+name,tofile='after/'+name))
(out/'change.patch').write_text(''.join(patch))
(out/'sources.json').write_text(json.dumps(dict(changed_files=changed,current_crate_sha256=before,driver_sha256={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in ['research/scripts/typescript_inheritance_oracle.py','research/harness/src/bin/typescript_inheritance_probe.rs']}),indent=2)+'\n')
(out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
print(json.dumps(checks),flush=True)
