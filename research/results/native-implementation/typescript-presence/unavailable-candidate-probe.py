"""Verify unavailable preferred candidates stop native fallback; not default binding equivalence."""
import json
import hashlib
import pathlib
import subprocess
import sys
import tempfile
root_repo = pathlib.Path(sys.argv[1]).resolve()
compiler = root_repo.parent / 'whatsurvey/node_modules/typescript/lib/typescript.js'
binary = root_repo / 'research/harness/target/release/typescript_file_probe'
with tempfile.TemporaryDirectory(prefix='graph-ts-unavailable-') as tmp:
    root = pathlib.Path(tmp).resolve()
    (root / '.graph-search').mkdir()
    (root / '.graph-search/config.toml').write_text('max_file_bytes = 256\n')
    (root / 'src').mkdir()
    (root / 'tsconfig.json').write_text(json.dumps({'compilerOptions': {'moduleResolution': 'bundler', 'paths': {'@/*': ['./src/*']}}}))
    (root / 'src/entry.ts').write_text('/*' + 'x' * 1024 + '*/\nexport const marker = 1;\n')
    (root / 'src/entry.js').write_text('export const marker = 1;\n')
    query = [dict(config='tsconfig.json',specifier='@/entry.js',mode='bundler')]
    native = json.loads(subprocess.check_output([str(binary),str(root)],input=json.dumps(query),text=True))[0]
    node = r'''
const ts=require(process.argv[1]),path=require('node:path'),root=process.argv[2];
const parsed=ts.getParsedCommandLineOfConfigFile(path.join(root,'tsconfig.json'),{}, {...ts.sys,onUnRecoverableConfigFileDiagnostic:()=>{throw new Error('config');}});
console.log(JSON.stringify({version:ts.version,target:ts.resolveModuleName('@/entry.js',path.join(root,'main.ts'),parsed.options,ts.sys).resolvedModule?.resolvedFileName}));
'''
    reference = json.loads(subprocess.check_output(['node','-e',node,str(compiler),str(root)],text=True))
    assert native['result']['error'] == 'ts_module_target_unavailable'
    assert reference['target'] == str(root / 'src/entry.ts')
    assert any(p['path']=='src/entry.ts' and p['availability'] == 'unavailable' for p in native['probes'])
    report = dict(binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(), scope='Native lookup refuses an oversized preferred TypeScript candidate instead of choosing JavaScript. The compiler target verifies priority; refusal is not positive target equivalence. Default bindings remain unintegrated.',native=native,compiler=reference,required_next_step='Persist namespace observations with project-aware binding dependencies before publishing default alias bindings.')
    print(json.dumps(report,indent=2).replace(str(root),'<fixture>'))
