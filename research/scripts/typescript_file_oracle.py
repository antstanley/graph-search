#!/usr/bin/env python3
"""Compare native inheritance + aliases + file loading with TypeScript 6.

Uses disposable indexed source facts and a case-sensitive compiler inventory.
Directory-package rules deliberately produce an unsupported result, never an
invented index target. No default project-selection claim is made.
"""
import hashlib
import json
import pathlib
import platform
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = ROOT / 'research/harness/target/release/typescript_file_probe'
COMPILER = ROOT.parent / 'whatsurvey/node_modules/typescript/lib/typescript.js'
OUT = pathlib.Path(sys.argv[1])
OUT.mkdir(parents=True, exist_ok=True)

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

compiler_hash, binary_hash = digest(COMPILER), digest(BIN)
cases = []

def case(name, candidate, files, *, modes=('bundler',), mappings=('wildcard',), options=None, unsupported=None):
    for mode in modes:
        for mapping in mappings:
            cases.append(dict(name=f'{name}-{mode}-{mapping}', candidate=candidate, files=files, mode=mode,
                              mapping=mapping, options=options or {}, unsupported=unsupported))

MODES = ('bundler', 'node_cjs', 'node_esm')
MAPS = ('wildcard', 'literal', 'base_url')
case('runtime-js', 'target.js', ['target.ts', 'target.tsx', 'target.d.ts', 'target.js', 'target.jsx'], modes=MODES, mappings=MAPS)
case('runtime-jsx', 'target.jsx', ['target.ts', 'target.tsx', 'target.d.ts', 'target.js', 'target.jsx'], modes=MODES, mappings=MAPS)
for ext, files in [('mjs', ['target.mts', 'target.d.mts', 'target.mjs', 'target.ts']),
                   ('cjs', ['target.cts', 'target.d.cts', 'target.cjs', 'target.ts']),
                   ('d.ts', ['target.ts', 'target.d.ts'])]:
    case('family-' + ext, 'target.' + ext, files, modes=MODES, mappings=MAPS)
for name, files in [
    ('all', ['target.ts', 'target.tsx', 'target.d.ts', 'target.js', 'target.jsx']),
    ('tsx', ['target.tsx', 'target.d.ts', 'target.js']),
    ('declaration', ['target.d.ts', 'target.js']),
    ('js', ['target.js', 'target.jsx']),
    ('jsx', ['target.jsx']),
    ('json-not-implicit', ['target.json']),
    ('module-families-not-implicit', ['target.mts', 'target.cts']),
    ('directory', ['target/index.ts', 'target/index.js']),
    ('js-file-before-ts-index', ['target.js', 'target/index.ts']),
]:
    case('extensionless-' + name, 'target', files, modes=MODES)
case('trailing-directory', 'target/', ['target.ts', 'target/index.ts'], modes=MODES, mappings=('literal',))
case('directory-with-dot', 'target.js/', ['target.ts', 'target.js/index.ts'], modes=MODES, mappings=('literal',))
case('duplicate-extension', 'target.js', ['target.js.ts'], modes=MODES, mappings=MAPS)
case('custom-wrapper', 'style.css', ['style.css', 'style.d.css.ts', 'style.css.ts'], modes=MODES, mappings=MAPS)
case('custom-appended-ts', 'style.css', ['style.css', 'style.css.ts'], modes=MODES)
case('unsupported-extension-only', 'style.css', ['style.css'], modes=MODES)
case('case-sensitive-suffix', 'target.JS', ['target.JS', 'target.d.JS.ts'], modes=MODES)
case('hidden-json-name', '.json', ['.json', '.d.json.ts'], modes=MODES, mappings=MAPS)
case('json-default', 'data.json', ['data.json'], modes=MODES, mappings=MAPS)
case('json-wrapper', 'data.json', ['data.d.json.ts', 'data.json'], modes=MODES, mappings=MAPS)
case('json-disabled', 'data.json', ['data.json'], modes=MODES, mappings=MAPS, options={'resolveJsonModule': False})
case('json-disabled-wrapper', 'data.json', ['data.d.json.ts', 'data.json'], modes=MODES, options={'resolveJsonModule': False})
case('node16-json-default', 'data.json', ['data.json'], modes=('node_cjs', 'node_esm'), options={'moduleResolution':'Node16','module':'Node16'})
case('node16-json-explicit', 'data.json', ['data.json'], modes=('node_cjs', 'node_esm'), options={'moduleResolution':'Node16','module':'Node16','resolveJsonModule':True})
case('suffix-inside-extension', 'target', ['target.ts', 'target.native.tsx'], options={'moduleSuffixes':['.native','']})
case('suffix-without-default', 'target', ['target.ts', 'target.native.tsx'], options={'moduleSuffixes':['.native']})
case('suffix-order', 'target', ['target.native.ts', 'target.web.ts'], options={'moduleSuffixes':['.web','.native','']})
case('empty-suffix-array', 'target', ['target.ts'], options={'moduleSuffixes':[]})
case('suffix-declaration', 'target.d.ts', ['target.native.d.ts', 'target.d.native.ts'], mappings=MAPS, options={'moduleSuffixes':['.native']})
case('suffix-explicit-js', 'target.js', ['target.native.js','target.native.ts'], modes=MODES, mappings=MAPS, options={'moduleSuffixes':['.native']})
case('suffix-json-wrapper', 'data.json', ['data.d.json.native.ts','data.native.d.json.ts'], options={'moduleSuffixes':['.native']})
case('suffix-directory', 'target/', ['target/index.native.ts','target/index.ts'], mappings=('literal',), options={'moduleSuffixes':['.native','']})
case('unicode', 'café.js', ['café.ts', 'café.js'], mappings=MAPS)
case('root-directory', '../', ['../index.ts'], modes=MODES, mappings=('literal',))
case('package-guard', 'pkg', {'pkg/package.json':'{"main":"./custom.js"}', 'pkg/custom.ts':'export const x=1;', 'pkg/index.ts':'export const y=1;'}, modes=('bundler','node_cjs'), unsupported='ts_module_package_directory_unmodeled')
case('esm-package-directory-not-eligible', 'pkg', {'pkg/package.json':'{"main":"./custom.js"}', 'pkg/custom.ts':'export const x=1;', 'pkg/index.ts':'export const y=1;'}, modes=('node_esm',))

node = r'''
const fs=require('node:fs'),path=require('node:path'),ts=require(process.argv[1]);
const {root,queries,inventory}=JSON.parse(fs.readFileSync(0,'utf8'));
const files=new Set(inventory.map(name=>path.join(root,name)));
const directories=new Set([root]);
for(let directory=root;;directory=path.dirname(directory)){directories.add(directory);if(directory===path.dirname(directory))break;}
for(const file of files){for(let dir=path.dirname(file);dir.startsWith(root);dir=path.dirname(dir)){directories.add(dir);if(dir===root)break;}}
const host={...ts.sys,useCaseSensitiveFileNames:true,getCurrentDirectory:()=>root,
 fileExists:file=>files.has(file),readFile:file=>files.has(file)?fs.readFileSync(file,'utf8'):undefined,
 directoryExists:directory=>directories.has(path.resolve(directory)),
 onUnRecoverableConfigFileDiagnostic:d=>{throw new Error(ts.flattenDiagnosticMessageText(d.messageText,' '));}};
const rows=queries.map(query=>{
 const config=path.join(root,query.config),parsed=ts.getParsedCommandLineOfConfigFile(config,{},host);
 const mode=query.mode==='node_esm'?ts.ModuleKind.ESNext:query.mode==='node_cjs'?ts.ModuleKind.CommonJS:undefined;
 const result=ts.resolveModuleName(query.specifier,path.join(path.dirname(config),'entry.ts'),parsed.options,host,undefined,undefined,mode);
 return {query,target:result.resolvedModule?.resolvedFileName??null,failed:result.failedLookupLocations,
 diagnostics:parsed.errors.map(d=>({code:d.code,message:ts.flattenDiagnosticMessageText(d.messageText,' ')}))};
});
console.log(JSON.stringify({version:ts.version,rows}));
'''
with tempfile.TemporaryDirectory(prefix='graph-ts-files-') as tmp:
    root = pathlib.Path(tmp).resolve()
    (root / '.graph-search').mkdir()
    (root / '.graph-search/config.toml').write_text('include_hidden = true\nreplace_defaults = true\nexcludes = []\n')
    queries, inventory = [], set()
    for index, row in enumerate(cases):
        directory = pathlib.Path(f'case-{index:03}')
        options = {'moduleResolution':'bundler' if row['mode']=='bundler' else 'NodeNext', 'module':'ESNext' if row['mode']=='bundler' else 'NodeNext'}
        options.update(row['options'])
        candidate = row['candidate']
        if row['mapping'] == 'literal':
            options['paths'] = {'request':['./'+candidate]}
            specifier = 'request'
        elif row['mapping'] == 'wildcard':
            options['paths'] = {'*':['./*']}
            specifier = candidate
        else:
            options['baseUrl'] = '.'
            specifier = candidate
        config = directory / 'tsconfig.json'
        (root / directory).mkdir()
        (root / config).write_text(json.dumps({'compilerOptions':options}))
        inventory.add(config.as_posix())
        items = row['files'].items() if isinstance(row['files'],dict) else ((name,'{}' if name.endswith('.json') else 'export const marker = 1;\n') for name in row['files'])
        for name, contents in items:
            path = (root / directory / name).resolve()
            assert path.is_relative_to(root)
            path.parent.mkdir(parents=True,exist_ok=True)
            path.write_text(contents)
            inventory.add(path.relative_to(root).as_posix())
        queries.append(dict(config=config.as_posix(),specifier=specifier,mode=row['mode']))
    inputs = {name:digest(root/name) for name in sorted(inventory)}
    native = json.loads(subprocess.check_output([str(BIN),str(root)],input=json.dumps(queries),text=True))
    oracle = json.loads(subprocess.check_output(['node','-e',node,str(COMPILER)],input=json.dumps(dict(root=str(root),queries=queries,inventory=sorted(inventory))),text=True))
    rows, mismatches = [], []
    for case_data, actual, reference in zip(cases,native,oracle['rows'],strict=True):
        expected = pathlib.Path(reference['target']).relative_to(root).as_posix() if reference['target'] else None
        if case_data['unsupported']:
            matched = actual['result'] == {'error':case_data['unsupported']}
            assert expected is not None, reference
        else:
            matched = 'error' not in actual['result'] and actual['result']['target'] == expected
        row = dict(case=case_data,native=actual,compiler=reference,matched=matched)
        rows.append(row)
        if not matched: mismatches.append(row)
    assert inputs == {name:digest(root/name) for name in inventory}
    report = json.dumps(dict(compiler_version=oracle['version'],case_sensitive_inventory=True,scope='Selected modern file loader composed with native inheritance and aliases; no automatic project selection',supported_cases=sum(row['unsupported'] is None for row in cases),explicitly_unsupported_cases=sum(row['unsupported'] is not None for row in cases),cases=rows),indent=2,ensure_ascii=False).replace(str(root),'<fixture>')
assert compiler_hash == digest(COMPILER) and binary_hash == digest(BIN)
(OUT/'oracle.json').write_text(report+'\n')
(OUT/'environment.json').write_text(json.dumps(dict(platform=platform.platform(),compiler_sha256=compiler_hash,binary_sha256=binary_hash,driver_sha256=digest(pathlib.Path(__file__)),fixture_sha256=inputs,fixture_unchanged=True,sibling_sources_unchanged=True),indent=2)+'\n')
if mismatches:
    print(json.dumps([dict(name=row['case']['name'],native=row['native']['result'],compiler=row['compiler']['target']) for row in mismatches],indent=2,ensure_ascii=False).replace(str(root),'<fixture>'))
    raise SystemExit(f'{len(mismatches)} / {len(cases)} cases mismatched')
print(f'Matched {len(cases)} cases with TypeScript {oracle["version"]}: {len(cases)-2} supported and 2 explicit package-directory refusals.')
