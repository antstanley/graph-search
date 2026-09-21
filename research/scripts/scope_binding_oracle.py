#!/usr/bin/env python3
"""Check native bound call targets against independent TypeScript declaration sites."""
import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile

REPO=pathlib.Path(__file__).resolve().parents[2]
BIN=REPO/'research/harness/target/release/scope_binding_probe'
TS=REPO.parent/'whatsurvey/node_modules/typescript/lib/typescript.js'
OUT=pathlib.Path(sys.argv[1]); OUT.mkdir(parents=True,exist_ok=True)
# Expectations are authored before compiler/native execution; False means the
# scoped value has no supported direct target, not that the compiler lacks a symbol.
CASES=[
 ('nested','function send(){} function entry(){function send(){} send();}',[True]),
 ('parameter','function send(){} function entry(send){send();}',[False]),
 ('destructure','function send(){} function entry({field:send}){send();}',[False]),
 ('array','function send(){} function entry(){const [send]=values;send();}',[False]),
 ('alias','function send(){} function entry(){let send=other;send();}',[False]),
 ('block','function send(){} function entry(){{let send=other;send();}send();}',[False,True]),
 ('closure','function send(){} function entry(send){const f=()=>send();}',[False]),
 ('hoisted','function send(){} function entry(){send();function send(){}}',[True]),
 ('tdz','function send(){} function entry(){send();const send=()=>{};send();}',[False,True]),
 ('immutable','function send(){} function entry(){const send=()=>send();send();}',[True,True]),
 ('mutable','function send(){} function entry(){let send=()=>send();send();}',[False,False]),
 ('catch','function send(){} function entry(){try{}catch(send){send();}send();}',[False,True]),
 ('loop','function send(){} function entry(){for(const send of values){send();}send();}',[False,True]),
 ('rename','function send(){} function entry(){const {send:other}=value;send();}',[True]),
 ('static','class Api{static send(){}} function entry(){class Api{static send(){}}Api.send();}',[True]),
 ('member-shadow','class Api{static send(){}} function entry(){function Api(){}Api.send();}',[False]),
 ('member-missing','class Api{static send(){}} function entry(){class Api{}Api.send();}',[False]),
 ('member-instance','class Api{static send(){}} function entry(){class Api{send(){}}Api.send();}',[False]),
 ('unicode','// élève 🦀\r\nfunction send(){}\r\nfunction entry(){send();}',[True]),
]
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
hashes={str(p):digest(p) for p in [BIN,TS,pathlib.Path(__file__)]}
with tempfile.TemporaryDirectory(prefix='graph-scope-oracle-') as temporary:
 root=pathlib.Path(temporary).resolve(); expected={}
 for ext in ['js','ts']:
  for name,code,labels in CASES:
   path=f'{name}.{ext}';(root/path).write_text(code+'\nexport {};\n');expected[path]=labels
 files=sorted(expected)
 fixture_hashes={path:digest(root/path) for path in files}
 native=json.loads(subprocess.check_output([str(BIN),str(root)],text=True))
 script=r'''
const ts=require(process.argv[1]),fs=require('node:fs'),path=require('node:path'),root=process.argv[2];
const files=JSON.parse(fs.readFileSync(0,'utf8'));
const program=ts.createProgram(files.map(f=>path.join(root,f)),{allowJs:true,noLib:true,noResolve:true,target:ts.ScriptTarget.ESNext,module:ts.ModuleKind.ESNext});
const checker=program.getTypeChecker(),rows=[];
for(const name of files){const file=program.getSourceFile(path.join(root,name));
 const byte=p=>Buffer.byteLength(file.text.slice(0,p),'utf8');
 function visit(node){if(ts.isCallExpression(node)){
  let symbol=checker.getSymbolAtLocation(node.expression);
  if(symbol && (symbol.flags & ts.SymbolFlags.Alias))symbol=checker.getAliasedSymbol(symbol);
  const declarations=(symbol?.declarations||[]).map(d=>({path:path.relative(root,d.getSourceFile().fileName),name_start:Buffer.byteLength(d.getSourceFile().text.slice(0,(d.name||d).getStart()),'utf8'),kind:ts.SyntaxKind[d.kind]}));
  rows.push({path:name,start:byte(node.getStart()),end:byte(node.end),declarations});
 }ts.forEachChild(node,visit);}visit(file);
}console.log(JSON.stringify(rows));
'''
 reference=json.loads(subprocess.check_output(['node','-e',script,str(TS),str(root)],input=json.dumps(files),text=True))
 by_site={(row['path'],row['start'],row['end']):row for row in reference}
 rows=[];resolved=0;correct=0;blocked=0
 for path in files:
  calls=sorted((row for row in native if row['path']==path),key=lambda row:row['occurrence']['span']['start_byte'])
  assert len(calls)==len(expected[path]),(path,calls)
  sites=[(row['occurrence']['span']['start_byte'],row['occurrence']['span']['end_byte']) for row in calls]
  compiler_sites=[(row['start'],row['end']) for row in reference if row['path']==path]
  assert len(sites)==len(set(sites)) and sorted(sites)==sorted(compiler_sites),(path,sites,compiler_sites)
  for native_row,wanted in zip(calls,expected[path]):
   occurrence=native_row['occurrence'];span=occurrence['span'];ref=by_site[(path,span['start_byte'],span['end_byte'])]
   target=native_row['target'];matched=False
   if target:
    resolved+=1
    matched=any(d['path']==target['path'] and target['span']['start_byte']<=d['name_start']<target['span']['end_byte'] for d in ref['declarations'])
    correct+=int(matched)
   else:
    blocked+=1;matched=bool(occurrence['reason']) and occurrence['resolution']=='unresolved'
   rows.append(dict(path=path,expected_resolved=wanted,native=native_row,compiler=ref,matched=matched and bool(target)==wanted))
 assert fixture_hashes=={path:digest(root/path) for path in files}
 report=dict(cases=len(files),calls=len(rows),resolved=resolved,correct_targets=correct,explicitly_unresolved=blocked,rows=rows)
 (OUT/'oracle.json').write_text(json.dumps(report,indent=2)+'\n')
 (OUT/'environment.json').write_text(json.dumps(dict(hashes=hashes,fixtures=fixture_hashes,cases=CASES),indent=2)+'\n')
 assert all(row['matched'] for row in rows),json.dumps([row for row in rows if not row['matched']],indent=2)
assert hashes=={str(p):digest(p) for p in [BIN,TS,pathlib.Path(__file__)]}
print(f'{correct}/{resolved} bound targets match compiler declaration sites; {blocked} explicit refusals; {len(rows)} calls in {len(files)} fixtures.')
