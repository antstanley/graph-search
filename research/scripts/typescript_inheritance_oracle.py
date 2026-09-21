#!/usr/bin/env python3
"""Compare native inherited fields and origins with an installed TS compiler.

No production compiler dependency. Indexes disposable config copies only. This
checks inheritance, not default project selection or native module resolution.
"""
import hashlib
import json
import pathlib
import platform
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = ROOT / 'research/harness/target/release/typescript_inheritance_probe'
COMPILER = ROOT.parent / 'whatsurvey/node_modules/typescript/lib/typescript.js'
OUT = pathlib.Path(sys.argv[1])
OUT.mkdir(parents=True, exist_ok=True)
SOURCES = [
    'blogwright/tsconfig.base.json',
    'blogwright/packages/core/tsconfig.json',
    'blogwright/packages/cli/tsconfig.json',
    'whatsurvey/tsconfig.json',
    'whatsurvey/workspaces/backend/tsconfig.json',
    'whatsurvey/workspaces/frontend/tsconfig.json',
    'whatsurvey/workspaces/frontend/.svelte-kit/tsconfig.json',
]

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

before = {name: digest(ROOT.parent / name) for name in SOURCES}
compiler_hash, binary_hash = digest(COMPILER), digest(BIN)
fixtures = {
    'chain/configs/base.json': {'compilerOptions': {'baseUrl': './base', 'rootDir': './src', 'outDir': './out', 'strict': False, 'paths': {'z*': ['./z*'], 'a*': ['./a*']}}, 'include': ['src/**/*.ts'], 'exclude': ['out'], 'references': [{'path': '../not-inherited'}]},
    'chain/configs/mid.json': {'extends': './base', 'compilerOptions': {'strict': True}},
    'chain/tsconfig.json': {'extends': './configs/mid.json', 'compilerOptions': {'baseUrl': './child'}, 'files': []},
    'array/first.json': {'compilerOptions': {'baseUrl': './first', 'strict': True, 'paths': {'first*': ['./first*']}}, 'include': ['first/**/*.ts']},
    'array/second.json': {'compilerOptions': {'paths': {'second*': ['./second*']}}, 'include': ['second/**/*.ts'], 'references': [{'path': '../not-inherited'}]},
    'array/tsconfig.json': {'extends': ['./first.json', './second.json'], 'references': [{'path': '../chain'}]},
    'array/clear.json': {'extends': './tsconfig.json', 'compilerOptions': {'paths': {}}, 'include': [], 'exclude': [], 'files': [], 'references': []},
    'jsonc/base.jsonc': '{/*comment*/"compilerOptions":{"rootDir":"./source",},"files":["entry.ts",],}',
    'jsonc/tsconfig.json': {'extends': './base.jsonc'},
    'diamond/base.json': {'compilerOptions': {'rootDir': './src', 'strict': True}},
    'diamond/left.json': {'extends': './base.json', 'compilerOptions': {'strict': False}},
    'diamond/right.json': {'extends': './base.json'},
    'diamond/tsconfig.json': {'extends': ['./left.json', './right.json']},
    'dotted/base.custom.json': {'compilerOptions': {'strict': True, 'rootDir': './src'}},
    'dotted/tsconfig.json': {'extends': './base.custom'},
    'dotfile/.json.json': {'compilerOptions': {'strict': True}},
    'dotfile/tsconfig.json': {'extends': './.json'},
    'missing/tsconfig.json': {'extends': './absent.json'},
    'cycle/tsconfig.json': {'extends': './second.json'},
    'cycle/second.json': {'extends': './tsconfig.json'},
}
selected = ['chain/tsconfig.json', 'array/tsconfig.json', 'array/clear.json', 'jsonc/tsconfig.json', 'diamond/tsconfig.json', 'dotted/tsconfig.json', 'dotfile/tsconfig.json', 'missing/tsconfig.json', 'cycle/tsconfig.json'] + SOURCES
node = r'''
const fs=require('node:fs'),path=require('node:path'),ts=require(process.argv[1]);
const {root,selected}=JSON.parse(fs.readFileSync(0,'utf8'));
const cases=selected.map(name=>{
 const unrecoverable=[];
 const host={...ts.sys,onUnRecoverableConfigFileDiagnostic:d=>unrecoverable.push(d)};
 const result=ts.getParsedCommandLineOfConfigFile(path.join(root,name),{},host);
 return {name,options:result?.options,raw:result?.raw,references:result?.projectReferences??[],
 diagnostics:[...unrecoverable,...(result?.errors??[])].map(d=>({code:d.code,message:ts.flattenDiagnosticMessageText(d.messageText,' ')}))};
});
console.log(JSON.stringify({version:ts.version,cases}));
'''
with tempfile.TemporaryDirectory(prefix='graph-ts-inheritance-') as tmp:
    root = pathlib.Path(tmp).resolve()
    (root / '.graph-search').mkdir()
    (root / '.graph-search/config.toml').write_text('include_hidden = true\n')
    for name in SOURCES:
        destination = root / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes((ROOT.parent / name).read_bytes())
    for name, value in fixtures.items():
        destination = root / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(value if isinstance(value, str) else json.dumps(value))
    native = json.loads(subprocess.check_output([str(BIN), str(root), *selected], text=True))
    oracle = json.loads(subprocess.check_output(['node', '-e', node, str(COMPILER)], input=json.dumps(dict(root=str(root), selected=selected)), text=True))
    results = []
    def absolute(origin, value):
        return str((root / origin).parent.joinpath(value).resolve())
    for reference in oracle['cases']:
        name = reference['name']
        result = native[name]
        checks = []
        codes = [diagnostic['code'] for diagnostic in reference['diagnostics']]
        if name.startswith(('missing/', 'dotfile/')):
            assert result == {'error': 'ts_config_missing'}, result
            assert 5083 in codes, reference
            checks.append('missing parent fails closed')
        elif name.startswith('cycle/'):
            assert result == {'error': 'ts_config_inheritance_cycle'}, result
            assert 18000 in codes, reference
            checks.append('cycle fails closed')
        else:
            assert 'error' not in result, (name, result)
            assert not any(code in codes for code in [5083, 18000]), reference
            fields = result['configuration'].get('fields', {})
            options = fields.get('compilerOptions', {})
            for key in ['baseUrl', 'rootDir', 'outDir']:
                if key in options:
                    assert absolute(result['option_origins'][key], options[key]) == reference['options'][key], (name, key, result, reference)
                    checks.append(key + ' declaring directory')
            for key in ['strict', 'noEmit', 'skipLibCheck', 'paths', 'types', 'declaration', 'esModuleInterop', 'isolatedModules', 'noUncheckedIndexedAccess', 'verbatimModuleSyntax']:
                if key in options:
                    assert options[key] == reference['options'][key], (name, key, result, reference)
                    checks.append(key + ' effective value')
            if 'paths' in options:
                assert str((root / result['option_origins']['paths']).parent) == reference['options']['pathsBasePath'], (name, result, reference)
                assert result['configuration'].get('path_patterns', []) == [key for key in reference['options']['paths'] if '*' in key], (name, result, reference)
                checks.append('paths origin and wildcard order')
            for key in ['include', 'exclude', 'files']:
                if key in fields:
                    actual = [absolute(result['field_origins'][key], value) for value in fields[key]]
                    expected = [absolute(name, value) for value in reference['raw'][key]]
                    assert actual == expected, (name, key, result, reference)
                    checks.append(key + ' declaring directory')
            refs = fields.get('references', [])
            actual = [absolute(result['field_origins']['references'], value['path']) for value in refs]
            expected = [value['path'] for value in reference['references']]
            assert actual == expected, (name, result, reference)
            checks.append('references local only')
            for dep, sha in result['dependencies'].items():
                assert digest(root / dep) == sha
            assert name in result['dependencies']
            checks.append('all dependency hashes')
        results.append(dict(name=name, checks=checks, native=result, compiler=reference, matched=True))
    # Replace temporary prefixes in saved evidence; hashes still describe exact inputs.
    report = json.dumps(dict(compiler_version=oracle['version'], cases=results), indent=2).replace(str(root), '<fixture>')
assert before == {name: digest(ROOT.parent / name) for name in SOURCES}
assert compiler_hash == digest(COMPILER) and binary_hash == digest(BIN)
(OUT / 'oracle.json').write_text(report + '\n')
(OUT / 'environment.json').write_text(json.dumps(dict(platform=platform.platform(), compiler_sha256=compiler_hash, binary_sha256=binary_hash, source_sha256=before, driver_sha256=digest(pathlib.Path(__file__)), sibling_sources_unchanged=True), indent=2) + '\n')
print(f"Matched {len(results)} inheritance cases with TypeScript {oracle['version']}; sibling sources unchanged.")
