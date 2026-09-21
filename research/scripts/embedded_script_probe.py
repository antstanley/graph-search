"""Check native embedded coordinates against an installed Svelte parser oracle.

The compiler supplies region boundaries; this does NOT test native framework
discovery. Original sources/indexes are read-only. No dependency install or
application code execution. Unicode AST offsets are converted from UTF-16 to UTF-8.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ORACLE = r'''
const fs = require('node:fs');
const compiler = require(process.argv[1]);
const source = fs.readFileSync(process.argv[2], 'utf8');
const ast = compiler.parse(source, { modern: true, filename: 'component.svelte' });
const byte = offset => Buffer.byteLength(source.slice(0, offset));
const span = node => ({start:byte(node.start),end:byte(node.end)});
const script = ast.instance;
if (!script || ast.module) throw Error('fixture must have exactly one instance script');
const functions = script.content.body.filter(n => n.type === 'FunctionDeclaration')
  .map(n => ({name:n.id.name,...span(n)}));
const calls = [];
function walk(value) {
  if (!value || typeof value !== 'object') return;
  if (value.type === 'CallExpression' && value.callee.type === 'Identifier')
    calls.push({name:value.callee.name,...span(value)});
  for (const [key,child] of Object.entries(value)) {
    if (key !== 'loc') {
      if (Array.isArray(child)) child.forEach(walk); else walk(child);
    }
  }
}
walk(script.content);
console.log(JSON.stringify({version:compiler.VERSION,region:span(script.content),functions,calls}));
'''


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    original = ROOT.parent / 'whatsurvey/workspaces/frontend/src/lib/components/ContactProfileFields.svelte'
    source = original.read_bytes()
    before = digest(original)
    compiler = subprocess.check_output(['node', '-e', 'console.log(require.resolve("svelte/compiler"))'],
        cwd=ROOT.parent / 'whatsurvey/workspaces/frontend', text=True).strip()
    compiler_hash = digest(Path(compiler))
    binary_hash = digest(args.binary)
    cases = []
    with tempfile.TemporaryDirectory(prefix='graph-search-embedded-probe-') as temporary:
        for name, prefix in [('original', b''), ('unicode_crlf_prefix', '<!-- 🙂 café -->\r\n'.encode())]:
            path = Path(temporary) / f'{name}.svelte'
            data = prefix + source
            path.write_bytes(data)
            oracle = json.loads(subprocess.check_output(['node', '-e', ORACLE, compiler, str(path)], text=True))
            region = oracle['region']
            native = json.loads(subprocess.check_output([str(args.binary.resolve()), str(path),
                str(region['start']), str(region['end']), 'ts', 'instance'], text=True))
            facts = native['facts']
            for function in oracle['functions']:
                matches = [s for s in facts['symbols'] if s['kind'] == 'function' and s['name'] == function['name']]
                assert len(matches) == 1, function
                span = matches[0]['span']
                assert (span['start_byte'], span['end_byte']) == (function['start'], function['end'])
                assert span['start_line'] == data[:function['start']].count(b'\n') + 1
            for call in oracle['calls']:
                matches = [r for r in facts['references'] if r['kind'] == 'calls'
                    and r.get('raw_name') == call['name'] and r.get('span', {}).get('start_byte') == call['start']]
                assert len(matches) == 1, call
                assert matches[0]['span']['end_byte'] == call['end']
            add_tag = [r for r in native['calls'] if r['raw_name'] == 'addTag']
            assert len(add_tag) == 1
            assert add_tag[0]['target'] == 'sym:component.svelte#embedded:instance>function:addTag'
            assert add_tag[0]['from_key'] == 'embedded:instance>function:onTagKeydown'
            assert add_tag[0]['class'] == 'explicit_lexical'
            cases.append({'name':name,'source_sha256':hashlib.sha256(data).hexdigest(),
                          'oracle':oracle,'matched_functions':len(oracle['functions']),
                          'matched_identifier_calls':len(oracle['calls']),
                          'resolved_add_tag':add_tag[0],
                          'symbol_keys':[s['key'] for s in facts['symbols']]})
    assert cases[0]['symbol_keys'] == cases[1]['symbol_keys']
    assert before == digest(original)
    assert compiler_hash == digest(Path(compiler))
    assert binary_hash == digest(args.binary)
    result = {'source':str(original),'original_sha256':before,'original_unchanged':True,
              'compiler_path':compiler,'compiler_sha256':compiler_hash,'binary_sha256':binary_hash,
              'native_framework_discovery_tested':False,'cases':cases}
    args.output.write_text(json.dumps(result,indent=2,sort_keys=True)+'\n')
    print('Both copies match Svelte function/call coordinates and the addTag target; original unchanged.')


if __name__ == '__main__':
    main()
