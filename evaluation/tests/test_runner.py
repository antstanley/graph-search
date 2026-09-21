import json
from pathlib import Path
import sys
import tempfile
import unittest
from taskbench.runner import trial, sanitized
from taskbench.__main__ import report


class FakeBackend:
    name='fixture'
    def __init__(self,root): self.root=root
    def search(self,query,**kwargs): return 'candidate x.rs:1 f\nx.rs:1\tfn f() {}'


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name)
        (self.root/'x.rs').write_text('fn f() {}\n')
        self.backend=FakeBackend(self.root)
        self.task=dict(id='x',repo='fixture',family='f',split='dev',kind='debug',prompt='Find the behavior')

    def test_evidence_protocol_never_claims_success(self):
        result=trial(self.task,self.backend)
        self.assertEqual(result['calls'],2)
        self.assertIsNone(result['task_success'])
        self.assertEqual(result['seen'],[('x.rs',1)])
        self.assertNotIn('history',sanitized(result))

    def test_caps_apply_before_coverage(self):
        result=trial(self.task,self.backend,response_bytes=20,context_bytes=20)
        self.assertEqual(result['response_bytes'],20)
        self.assertEqual(result['seen'],[])

    def test_native_json_survives_exact_budget_and_truncation_is_not_evidence(self):
        from taskbench.backends import render_graph_explore
        from taskbench.core import digest
        value={'items':[{'node':{'path':'x.rs','start_line':1,'name':'f'},
                        'impact':{'direct_callers':2,'total_callers':3},
                        'snippet':{'start_line':1,'source_hash':digest(b'fn f() {}\n'),
                                   'lines':['fn f() {}']}}],
               'edges':[{'from':'caller','to':'f'}],'context':{'generation':'fixture'}}
        encoded=render_graph_explore(value)
        self.backend.search=lambda *args,**kwargs: encoded
        result=trial(self.task,self.backend,response_bytes=len(encoded.encode()))
        self.assertEqual(result['calls'],2)
        self.assertEqual(json.loads(result['history'][0]['response']),value)
        self.assertFalse(result['history'][0]['truncated'])
        self.assertEqual(result['seen'],[('x.rs',1)])
        self.assertEqual(result['history'][1]['action']['arguments']['path'],'x.rs')
        truncated=trial(self.task,self.backend,response_bytes=len(encoded.encode())-1)
        self.assertEqual(truncated['calls'],1)
        self.assertTrue(truncated['history'][0]['truncated'])
        self.assertEqual(truncated['seen'],[])

    def test_agent_action_answer_and_usage(self):
        driver=self.root/'driver.py'
        driver.write_text('''import json,sys
r=json.load(sys.stdin)
assert "oracle" not in r and "criteria" not in r
if not r['history']:
 print(json.dumps({'action':{'name':'read','arguments':{'path':'x.rs','start':1}},'usage':{'input_tokens':10,'output_tokens':5}}))
else:
 print(json.dumps({'answer':'The function is empty.','citations':[{'path':'x.rs','line':1}],'usage':{'input_tokens':20,'output_tokens':7}}))
''')
        result=trial(self.task,self.backend,driver=[sys.executable,str(driver)],max_calls=1)
        self.assertEqual(result['status'],'answered')
        self.assertEqual(result['calls'],1)
        self.assertEqual(result['provider_tokens'],dict(input_tokens=30,output_tokens=12))
        self.assertIsNone(result['task_success'])

    def test_malformed_driver_is_error(self):
        result=trial(self.task,self.backend,driver=[sys.executable,'-c','print("not json")'])
        self.assertEqual(result['status'],'driver_error')
        self.assertIsNone(result['provider_tokens'])

    def test_report_missing_is_not_a_miss(self):
        records=[dict(task_id='x',repo='x',split='dev',kind='debug',arm='missing',status='unavailable',task_success=None)]
        row=report(records)['rows'][0]
        self.assertEqual(row['available'],0)
        self.assertIsNone(row['task_success_rate'])
        self.assertIsNone(row['required_file_recall_mean'])

if __name__=='__main__': unittest.main()
