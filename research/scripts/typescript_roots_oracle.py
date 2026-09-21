#!/usr/bin/env python3
"""Native root enumeration against the existing TypeScript compiler, temp fixtures only."""
import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parents[2]
BIN = REPO / 'research/harness/target/release/typescript_roots_probe'
TS = REPO.parent / 'whatsurvey/node_modules/typescript/lib/typescript.js'
OUT = pathlib.Path(sys.argv[1])
OUT.mkdir(parents=True, exist_ok=True)
cases = []

def case(name, files, fields=None, options=None, extra=None, config='tsconfig.json'):
    cases.append(dict(name=name, files=files, fields=fields or {}, options=options or {}, extra=extra or {}, config=config))

files = ['src/a.ts', 'src/b.tsx', 'src/c.js', 'src/d.jsx', 'src/e.mts', 'src/f.cts',
         'src/g.mjs', 'src/h.cjs', 'src/i.d.ts', 'src/j.d.mts', 'src/k.d.cts', 'src/data.json',
         'src/.hidden.ts', 'src/.cache/a.ts', 'src/node_modules/a.ts', 'src/a.min.js',
         'dist/generated.ts', 'types/generated.d.ts', 'top.ts']
case('defaults', files)
case('allow-js', files, options={'allowJs': True})
case('check-js', files, options={'checkJs': True})
case('allow-js-overrides-check', files, options={'allowJs': False, 'checkJs': True})
case('jsconfig-defaults', files, config='jsconfig.json')
case('jsconfig-check-disabled', files, options={'checkJs': False}, config='jsconfig.json')
case('empty-files', files, {'files': []})
case('empty-include', files, {'include': []})
case('explicit-missing-and-excluded', files, {'files': ['missing.ts', 'src/a.ts', 'src/c.js'], 'exclude': ['**/*']})
case('files-plus-include', files, {'files': ['src/c.js'], 'include': ['src/**/*.ts']})
case('implicit-directory', files, {'include': ['src']})
case('trailing-directory', files, {'include': ['src/']})
case('flat', files, {'include': ['src/*']}, {'allowJs': True})
case('hidden-explicit', files, {'include': ['src/.hidden.ts', 'src/.cache/*']})
case('package-explicit', files, {'include': ['src/node_modules/*']})
case('package-wildcard', files, {'include': ['src/*/a.ts']})
case('minified-explicit', files, {'include': ['src/*.min.js']}, {'allowJs': True})
case('exclude-prefix', files, {'exclude': ['src']})
case('exclude-recursive', files, {'exclude': ['**/a.ts', '**/generated.*']})
case('output-defaults', files, options={'outDir': './dist', 'declarationDir': './types'})
case('output-override', files, {'exclude': []}, {'outDir': './dist', 'declarationDir': './types'})
case('json-default-glob', files, options={'resolveJsonModule': True})
case('json-explicit-glob', files, {'include': ['src/**/*.json']}, {'resolveJsonModule': True})
case('json-disabled', files, {'include': ['src/**/*.json']}, {'resolveJsonModule': False})
case('json-module-default', files, {'include': ['src/**/*.json']}, {'module': 'nodenext', 'moduleResolution': 'nodenext'})
for module in [None, 'commonjs', 'esnext', 'preserve', 'none', 'amd', 'umd', 'system', 'node16', 'node18', 'node20', 'nodenext']:
    case('json-inferred-' + ('omitted' if module is None else module), files, {'include': ['src/**/*.json']}, {'module': module, 'moduleResolution': None})
for module in ['preserve', 'esnext', 'node20', 'nodenext']:
    case('json-resolution-override-' + module, files, {'include': ['src/**/*.json']}, {'module': module, 'moduleResolution': 'node10'})
case('unicode-questions', ['src/🦀.ts', 'src/é.ts', 'src/ab.ts'], {'include': ['src/?.ts']})
case('unicode-two-questions', ['src/🦀.ts', 'src/é.ts', 'src/ab.ts'], {'include': ['src/??.ts']})
case('dotfile-empty-star', ['.ts', '.hidden.ts'], {'include': ['*.ts']})
for family in [['x.ts', 'x.tsx', 'x.d.ts', 'x.js', 'x.jsx'], ['x.d.ts', 'x.js'], ['x.cts', 'x.d.cts', 'x.cjs'], ['x.mts', 'x.d.mts', 'x.mjs']]:
    case('family-' + str(len(cases)), family, options={'allowJs': True})
    case('literal-family-' + str(len(cases)), family, {'files': [family[-1]], 'include': ['**/*']}, {'allowJs': True})
case('inherited-membership', ['src/a.ts', 'app/src/b.ts', 'dist/out.ts'],
     {'extends': '../base.json'}, extra={'base.json': {'include': ['src'], 'compilerOptions': {'outDir': 'dist'}}}, config='app/tsconfig.json')
case('inherited-files-override', ['src/a.ts', 'app/src/b.ts'],
     {'extends': '../base.json', 'files': ['src/b.ts']}, extra={'base.json': {'files': ['src/a.ts']}}, config='app/tsconfig.json')

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

before = {str(p): digest(p) for p in [BIN, TS, pathlib.Path(__file__)]}
with tempfile.TemporaryDirectory(prefix='graph-ts-roots-oracle-') as tmp:
    root = pathlib.Path(tmp).resolve()
    (root / '.graph-search').mkdir()
    (root / '.graph-search/config.toml').write_text('include_hidden = true\nreplace_defaults = true\nexcludes = []\n')
    configs = []
    for c in cases:
        home = root / c['name']
        config = home / c['config']
        config.parent.mkdir(parents=True, exist_ok=True)
        raw = dict(c['fields'])
        raw['compilerOptions'] = dict(module='esnext', moduleResolution='bundler', **{})
        raw['compilerOptions'].update(c['options'])
        # None in the case table denotes an omitted option, not JSON null.
        raw['compilerOptions'] = {k:v for k,v in raw['compilerOptions'].items() if v is not None}
        config.write_text(json.dumps(raw))
        for name in c['files']:
            path = home / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('{}' if name.endswith('.json') else 'export const marker = 1;\n')
        for name, content in c['extra'].items():
            path = home / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(content))
        configs.append(config.relative_to(root).as_posix())
    fixture_hashes = {p.relative_to(root).as_posix(): digest(p) for p in root.rglob('*') if p.is_file()}
    native = json.loads(subprocess.check_output([str(BIN), str(root)], input=json.dumps(configs), text=True))
    node = r'''
const ts=require(process.argv[1]), path=require('node:path'), fs=require('node:fs'), root=process.argv[2];
const configs=JSON.parse(fs.readFileSync(0,'utf8'));
console.log(JSON.stringify(configs.map(config=>{
 const parsed=ts.getParsedCommandLineOfConfigFile(path.join(root,config),{}, {...ts.sys,useCaseSensitiveFileNames:true,onUnRecoverableConfigFileDiagnostic:d=>{throw new Error(ts.flattenDiagnosticMessageText(d.messageText,' '));}});
 return {config,roots:parsed.fileNames.map(file=>path.relative(root,file).split(path.sep).join('/')).sort(),diagnostics:parsed.errors.map(d=>d.code)};
})));
'''
    reference = json.loads(subprocess.check_output(['node', '-e', node, str(TS), str(root)], input=json.dumps(configs), text=True))
    rows = [dict(case=c['name'], native=n, compiler=r, matched=n.get('roots') == r['roots']) for c,n,r in zip(cases,native,reference)]
    assert fixture_hashes == {p.relative_to(root).as_posix(): digest(p) for p in root.rglob('*') if p.is_file()}
    (OUT / 'oracle.json').write_text(json.dumps(rows, indent=2)+'\n')
    (OUT / 'environment.json').write_text(json.dumps(dict(hashes=before, fixtures=fixture_hashes, cases=cases), indent=2)+'\n')
assert before == {str(p): digest(p) for p in [BIN, TS, pathlib.Path(__file__)]}
failed = [r for r in rows if not r['matched']]
assert not failed, json.dumps(failed, indent=2)
print(f'Matched {len(rows)} root-file sets with TypeScript.')
