"""Reproduce production lexical and selective-sync measurements on disposable stores/copies."""
import json,pathlib,subprocess,sys
repo=pathlib.Path(__file__).resolve().parents[2]
research=repo/'research'
def run(args,**kwargs):
 print('+',' '.join(map(str,args)),flush=True)
 return subprocess.run(list(map(str,args)),cwd=repo,check=True,**kwargs)
run(['cargo','build','--locked','--offline','--manifest-path',research/'harness/Cargo.toml','--bins'])
for script in ['lexical_followup.py','lexical_holdout.py']:
 run([sys.executable,research/'scripts'/script])
for name,edited in [('nanus','crates/nanus-domain/src/context.rs'),('blogwright','packages/pds/src/secret.ts'),('whatsurvey','workspaces/backend/src/core/settings/crypto.ts')]:
 with (research/'results'/f'{name}-incremental-followup.json').open('w') as out:
  run([research/'harness/target/debug/sync_probe',pathlib.Path.home()/'code'/name,edited],stdout=out)
run([sys.executable,research/'scripts/provenance.py'])
