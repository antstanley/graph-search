#!/usr/bin/env python3
"""Compare native alias dispatch with TS on explicit-file fixtures.

The native probe's research callback loads only exact admitted filenames. Cases
intentionally have explicit .ts substitutions/specifiers and no directory/package
or competing extension targets. This is not a complete native module-loader test.
"""
import hashlib
import json
import pathlib
import platform
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = ROOT / 'research/harness/target/release/typescript_alias_probe'
COMPILER = ROOT.parent / 'whatsurvey/node_modules/typescript/lib/typescript.js'
OUT = pathlib.Path(sys.argv[1])
OUT.mkdir(parents=True, exist_ok=True)
SOURCES = ['whatsurvey/workspaces/frontend/tsconfig.json', 'whatsurvey/workspaces/frontend/.svelte-kit/tsconfig.json']

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

before = {name: digest(ROOT.parent / name) for name in SOURCES}
compiler_hash, binary_hash = digest(COMPILER), digest(BIN)
configs = {
    'order/base.json': {'compilerOptions': {'moduleResolution': 'bundler', 'baseUrl': '../base', 'paths': {'@/exact.ts': ['exact.ts'], '@/*Z': ['first/*.ts'], '@/*YZ': ['second/*.ts'], '@/long*': ['long/*.ts'], 'fallback': ['missing.ts', 'second.ts', 'third.ts'], 'miss.ts': ['absent.ts'], 'empty*': ['literal*.ts'], 'é*終': ['unicode/*.ts'], '.bare': ['bare.ts'], '@/*': ['generic/*.ts']}}},
    'order/tsconfig.json': {'extends': './base.json'},
    'reverse/tsconfig.json': {'compilerOptions': {'moduleResolution': 'bundler', 'baseUrl': '../base', 'paths': {'@/*YZ': ['second/*.ts'], '@/*Z': ['first/*.ts']}}},
    'override/tsconfig.json': {'extends': '../order/base.json', 'compilerOptions': {'baseUrl': '../override-base'}},
    'origin/config/base.json': {'compilerOptions': {'moduleResolution': 'bundler', 'paths': {'@/*': ['./src/*.ts']}}},
    'origin/tsconfig.json': {'extends': './config/base.json'},
    'empty-map/tsconfig.json': {'extends': '../order/base.json', 'compilerOptions': {'paths': {}}},
    'plain/tsconfig.json': {'compilerOptions': {'moduleResolution': 'bundler'}},
}
files = [
    'base/exact.ts', 'base/generic/exact.ts.ts', 'base/second/ghost.ts', 'base/first/xY.ts', 'base/second/x.ts', 'base/long/YZ.ts',
    'base/second.ts', 'base/third.ts', 'base/miss.ts', 'base/other.ts',
    'base/literal*.ts', 'base/unicode/中.ts', 'base/bare.ts',
    'override-base/exact.ts', 'origin/config/src/file.ts',
    'whatsurvey/workspaces/frontend/src/lib/file.ts',
    'whatsurvey/workspaces/frontend/.svelte-kit/types/index.d.ts',
]
cases = [
    ('order/tsconfig.json', '@/exact.ts', 'base/exact.ts', 'paths', '@/exact.ts'),
    ('order/tsconfig.json', '@/xYZ', 'base/first/xY.ts', 'paths', '@/*Z'),
    ('order/tsconfig.json', '@/ghostYZ', None, 'paths', '@/*Z'),
    ('reverse/tsconfig.json', '@/xYZ', 'base/second/x.ts', 'paths', '@/*YZ'),
    ('order/tsconfig.json', '@/longYZ', 'base/long/YZ.ts', 'paths', '@/long*'),
    ('order/tsconfig.json', 'fallback', 'base/second.ts', 'paths', 'fallback'),
    ('order/tsconfig.json', 'miss.ts', None, 'paths', 'miss.ts'),
    ('order/tsconfig.json', 'other.ts', 'base/other.ts', 'base_url', None),
    ('order/tsconfig.json', 'empty', 'base/literal*.ts', 'paths', 'empty*'),
    ('order/tsconfig.json', 'é中終', 'base/unicode/中.ts', 'paths', 'é*終'),
    ('order/tsconfig.json', '.bare', 'base/bare.ts', 'paths', '.bare'),
    ('override/tsconfig.json', '@/exact.ts', 'override-base/exact.ts', 'paths', '@/exact.ts'),
    ('origin/tsconfig.json', '@/file', 'origin/config/src/file.ts', 'paths', '@/*'),
    ('empty-map/tsconfig.json', 'miss.ts', 'base/miss.ts', 'base_url', None),
    ('plain/tsconfig.json', 'absent-package', None, 'unmatched', None),
    ('whatsurvey/workspaces/frontend/tsconfig.json', '$lib/file.ts', 'whatsurvey/workspaces/frontend/src/lib/file.ts', 'paths', '$lib/*'),
    ('whatsurvey/workspaces/frontend/tsconfig.json', '$app/types', 'whatsurvey/workspaces/frontend/.svelte-kit/types/index.d.ts', 'paths', '$app/types'),
]
queries = [dict(config=config, specifier=specifier) for config, specifier, *_ in cases]
node = r'''
const fs=require('node:fs'),path=require('node:path'),ts=require(process.argv[1]);
const {root,queries}=JSON.parse(fs.readFileSync(0,'utf8'));
const rows=queries.map(query=>{
 const parsed=ts.getParsedCommandLineOfConfigFile(path.join(root,query.config),{}, {...ts.sys,onUnRecoverableConfigFileDiagnostic:d=>{throw new Error(ts.flattenDiagnosticMessageText(d.messageText,' '))}});
 const traces=[];
 const result=ts.resolveModuleName(query.specifier,path.join(root,path.dirname(query.config),'entry.ts'), {...parsed.options,traceResolution:true}, {...ts.sys,trace:text=>traces.push(text)});
 return {query,target:result.resolvedModule?.resolvedFileName??null,traces,diagnostics:parsed.errors.map(d=>({code:d.code,message:ts.flattenDiagnosticMessageText(d.messageText,' ')}))};
});
console.log(JSON.stringify({version:ts.version,rows}));
'''
with tempfile.TemporaryDirectory(prefix='graph-ts-alias-') as tmp:
    root = pathlib.Path(tmp).resolve()
    (root / '.graph-search').mkdir()
    (root / '.graph-search/config.toml').write_text('include_hidden = true\n')
    for name, config in configs.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(config, ensure_ascii=False))
    for name in SOURCES:
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes((ROOT.parent / name).read_bytes())
    for name in files:
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('export const marker = 1;\n' if not name.endswith('.d.ts') else 'export declare const marker: number;\n')
    native = json.loads(subprocess.check_output([str(BIN), str(root)], input=json.dumps(queries), text=True))
    oracle = json.loads(subprocess.check_output(['node', '-e', node, str(COMPILER)], input=json.dumps(dict(root=str(root), queries=queries)), text=True))
    rows = []
    for case, actual, reference in zip(cases, native, oracle['rows'], strict=True):
        config, specifier, expected, route, pattern = case
        expected_abs = str(root / expected) if expected else None
        assert reference['target'] == expected_abs, (case, reference)
        assert actual['result'].get('target') == expected and actual['result'].get('route') == route, (case, actual)
        if pattern is not None:
            assert actual['result']['pattern'] == pattern
        if specifier == 'fallback':
            assert [a['candidate'] for a in actual['attempts']] == ['base/missing.ts', 'base/second.ts']
        if config == 'order/tsconfig.json' and specifier == 'miss.ts':
            assert actual['attempts'] == [dict(candidate='base/absent.ts', mapped=True, substitution='absent.ts')]
        rows.append(dict(query=actual['query'], expected=expected, native=actual, compiler=reference, matched=True))
    report = json.dumps(dict(compiler_version=oracle['version'], scope='Native alias dispatch with exact-file research callback, not complete module resolution', cases=rows), indent=2, ensure_ascii=False).replace(str(root), '<fixture>')
assert before == {name: digest(ROOT.parent / name) for name in SOURCES}
assert compiler_hash == digest(COMPILER) and binary_hash == digest(BIN)
(OUT / 'oracle.json').write_text(report + '\n')
(OUT / 'environment.json').write_text(json.dumps(dict(platform=platform.platform(),compiler_sha256=compiler_hash,binary_sha256=binary_hash,driver_sha256=digest(pathlib.Path(__file__)),source_sha256=before,sibling_sources_unchanged=True),indent=2)+'\n')
print(f"Matched {len(rows)} alias-dispatch cases with TypeScript {oracle['version']}; sibling sources unchanged.")
