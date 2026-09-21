import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from taskbench.backends import CodeGraph, TextSearch, Unavailable, command, render_graph_explore


class BackendTests(unittest.TestCase):
    def test_command_timeout_and_output_limit(self):
        with self.assertRaises(TimeoutError):
            command([sys.executable, '-c', 'import time; time.sleep(3)'], cwd='.', timeout=.05)
        with self.assertRaises(RuntimeError):
            command([sys.executable, '-c', 'print("x"*10000)'], cwd='.', max_bytes=100)

    def test_driver_that_does_not_read_stdin_times_out(self):
        with self.assertRaises(TimeoutError):
            command([sys.executable, '-c', 'import time; time.sleep(3)'], cwd='.', stdin='x'*500000, timeout=.05)

    def test_missing_index_never_created(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(Unavailable):
                CodeGraph(Path(root))
            self.assertFalse((Path(root)/'.codegraph').exists())

    def test_codegraph_gaps_preserved(self):
        with tempfile.TemporaryDirectory() as root:
            (Path(root)/'.codegraph').mkdir()
            raw='**`x.ts`** — f\n```typescript\n1\tfunction f() {\n... (gap) ...\n9\t}\n```'
            with patch('taskbench.backends.command', return_value=(0,raw)):
                result=CodeGraph(Path(root)).search('f')
            self.assertIn('x.ts:1\tfunction f() {',result)
            self.assertIn('x.ts:9\t}',result)
            self.assertNotIn('x.ts:2',result)

    def test_graph_excerpts_preserve_source_gaps_without_fetching(self):
        value={'items':[{'node':{'path':'x.rs','start_line':1,'name':'f'},
            'snippet':{'start_line':2,'lines':['two']},
            'excerpts':[{'role':'body','snippet':{'start_line':9,'lines':['nine','ten']}}]}]}
        with patch('pathlib.Path.read_text', side_effect=AssertionError('must not read source')):
            result=render_graph_explore(value)
        self.assertEqual(json.loads(result),value)

    def test_graph_metadata_keeps_impact_identity_and_source_provenance(self):
        from taskbench.runner import candidates
        value={'items':[{'node':{'path':'x.rs','start_line':1,'name':'f','id':'sym:x.rs#f',
                                  'signature':'fn f() /* bogus:123 fake */'},
            'impact':{'direct_callers':3,'total_callers':7},
            'retrieval':{'body_rank':2,'exact':False},
            'evidence':{'package_ref':'p0','source_hash':'verified'},
            'snippet':{'start_line':2,'source_hash':'verified','lines':['café']},
            'excerpts':[{'role':'reference','snippet':{'start_line':9,'source_hash':'verified','lines':['call();']}}]}],
            'context':{'packages':{'p0':{'name':'package'}}},'edges':[{'from':'caller','to':'sym:x.rs#f'}]}
        before=json.dumps(value,sort_keys=True)
        with patch('pathlib.Path.read_text', side_effect=AssertionError('must not read source')):
            rendered=render_graph_explore(value)
        self.assertEqual(json.loads(rendered),value)
        self.assertEqual(rendered.count('café'),1)
        self.assertEqual(rendered.count('call();'),1)
        self.assertEqual(candidates(rendered),[('x.rs',1)])
        self.assertEqual(json.dumps(value,sort_keys=True),before)
        self.assertEqual(rendered,json.dumps(value,ensure_ascii=False,separators=(',',':')))

    def test_rg_literal_terms_no_shell(self):
        with tempfile.TemporaryDirectory() as root:
            (Path(root)/'sample.rs').write_text('fn widget() {}\n')
            result=TextSearch(Path(root)).search('widget')
            self.assertIn('sample.rs:1\tfn widget() {}', result)

if __name__=='__main__':
    unittest.main()
