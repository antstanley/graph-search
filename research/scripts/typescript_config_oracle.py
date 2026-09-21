#!/usr/bin/env python3
"""Compare native raw config projections with an installed TypeScript parser.

The compiler is an independent research oracle, never a production dependency.
Only disposable source/store copies are indexed; sibling configurations are read.
"""
import hashlib,json,pathlib,platform,subprocess,sys,tempfile
ROOT=pathlib.Path(__file__).resolve().parents[2]
BIN=ROOT/'research/harness/target/release/typescript_config_probe'
COMPILER=ROOT.parent/'whatsurvey/node_modules/typescript/lib/typescript.js'
OUT=pathlib.Path(sys.argv[1]);OUT.mkdir(parents=True,exist_ok=True)
FIELDS=['extends','compilerOptions','files','include','exclude','references']
SOURCES=['blogwright/tsconfig.base.json','blogwright/packages/core/tsconfig.json','blogwright/packages/cli/tsconfig.json','whatsurvey/tsconfig.json','whatsurvey/workspaces/backend/tsconfig.json','whatsurvey/workspaces/frontend/tsconfig.json','whatsurvey/workspaces/frontend/.svelte-kit/tsconfig.json']
def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
before={path:digest(ROOT.parent/path) for path in SOURCES}
compiler_hash=digest(COMPILER); binary_hash=digest(BIN)
cases=[dict(name=f'actual-{i}.json',text=(ROOT.parent/path).read_bytes().decode("utf-8"),original=path) for i,path in enumerate(SOURCES)]
cases.extend([
    dict(name='comments.jsonc',text='\ufeff{/* café */"extends":["./first.json","./last.json",],//comment\r\n"compilerOptions":{"paths":{"$lib/*":["../src/*",]},"baseUrl":"https://x/*y*/",},"files":[],}'),
    dict(name='empty.json',text='{}'),
    dict(name='null.json',text='{"compilerOptions":{"paths":null},"exclude":null}'),
    dict(name='duplicates.json',text='{"compilerOptions":{"baseUrl":"first","baseUrl":"last"}}'),
    dict(name='quoted.json',text=json.dumps({'compilerOptions':{'baseUrl':'quoted"// /* string \\ café','paths':{'*':['./one/*','./two/*']}},'references':[{'path':'../other','prepend':True}]})),
    dict(name='tie-first.json',text='{"compilerOptions":{"paths":{"a*Z":["/first.ts"],"a*YZ":["/second.ts"]}}}',tie=True,expected='/first.ts'),
    dict(name='tie-second.json',text='{"compilerOptions":{"paths":{"a*YZ":["/second.ts"],"a*Z":["/first.ts"]}}}',tie=True,expected='/second.ts'),
    dict(name='duplicate-containers.json',text='{"compilerOptions":{"paths":{"unused*":[]}},"compilerOptions":{"paths":{"old*":[]},"paths":{"a*Z":["first"],"a*YZ":["second"],"a*Z":["last"],"exact":[]}}}'),
    dict(name='unterminated.json',text='{"compilerOptions":{} /*',invalid=True),
    dict(name='holes.json',text='{"files":[,]}',invalid=True),
    dict(name='value.json',text='{"files":,}',invalid=True),
    dict(name='syntax.json',text='{"files":["a" "b"]}',invalid=True),
])
node=r'''
const fs=require('node:fs'); const ts=require(process.argv[1]);
const cases=JSON.parse(fs.readFileSync(0,'utf8'));
console.log(JSON.stringify({version:ts.version,cases:cases.map(c=>{
const p=ts.parseConfigFileTextToJson(c.name,c.text);
const patterns=p.error?null:Object.keys(p.config.compilerOptions?.paths??{}).filter(k=>k.includes('*'));
const resolved=!c.tie||p.error?null:ts.resolveModuleName('aYZ','/entry.ts',
{moduleResolution:ts.ModuleResolutionKind.Bundler,baseUrl:'/',paths:p.config.compilerOptions.paths},
{fileExists:f=>['/first.ts','/second.ts'].includes(f),readFile:()=>'',directoryExists:()=>true}).resolvedModule?.resolvedFileName;
return {name:c.name,patterns,resolved,error:p.error?{code:p.error.code,message:ts.flattenDiagnosticMessageText(p.error.messageText,' ')}:null,
fields:p.error?null:Object.fromEntries(['extends','compilerOptions','files','include','exclude','references'].filter(k=>Object.hasOwn(p.config,k)).map(k=>[k,p.config[k]]))};})}));
'''
oracle=json.loads(subprocess.check_output(['node','-e',node,str(COMPILER)],input=json.dumps(cases),text=True))
with tempfile.TemporaryDirectory(prefix='graph-tsconfig-oracle-') as tmp:
    source=pathlib.Path(tmp)
    for case in cases: (source/case['name']).write_text(case['text'])
    native=json.loads(subprocess.check_output([str(BIN),str(source)],text=True))
    results=[]
    for case,reference in zip(cases,oracle['cases'],strict=True):
        row=native[case['name']]
        assert row['source_hash']==hashlib.sha256(case['text'].encode()).hexdigest()
        assert row['version']==14
        fact=row['config']
        if case.get('invalid'):
            assert reference['error'] is not None,reference
            assert fact.get('unavailable_reason') and not fact.get('fields'),fact
        else:
            assert reference['error'] is None,reference
            assert not fact.get('unavailable_reason'),fact
            assert fact.get('fields',{})==reference['fields'],(case['name'],fact,reference)
            assert fact.get('path_patterns',[])==reference['patterns'],(case['name'],fact,reference)
            if case.get('tie'): assert reference['resolved']==case['expected'],reference
        results.append(dict(name=case['name'],original=case.get('original'),native=row,oracle=reference,matched=True))
assert before=={path:digest(ROOT.parent/path) for path in SOURCES}
assert compiler_hash==digest(COMPILER) and binary_hash==digest(BIN)
(OUT/'oracle.json').write_text(json.dumps(dict(compiler_version=oracle['version'],cases=results,sibling_sources_unchanged=True),indent=2)+'\n')
(OUT/'environment.json').write_text(json.dumps(dict(platform=platform.platform(),compiler=str(COMPILER),compiler_sha256=compiler_hash,binary_sha256=binary_hash,source_sha256=before,driver_sha256=digest(pathlib.Path(__file__))),indent=2)+'\n')
print(f"Matched {len(cases)} cases with TypeScript {oracle['version']}; sibling sources, compiler and native binary unchanged.")
