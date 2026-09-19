import json
from pathlib import Path
import sys
import tempfile
import unittest

from taskbench.backends import GraphSearch, command
from taskbench.runner import trial
from taskbench.__main__ import finalize, report


class EmptyBackend:
    name='text'
    def __init__(self,root): self.root=root
    def search(self,*args,**kwargs): return ''


class FailureIsolationTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name)
        self.task=dict(id='x',repo='fixture',split='dev',kind='debug',prompt='Explain')
        self.backend=EmptyBackend(self.root)

    def test_graph_timeout_recovers_next_trial_and_closes_twice(self):
        host=self.root/'host'
        host.write_text('#!'+sys.executable+'''\nimport json,sys,time
print(json.dumps({'ready':True,'setup_ms':0}),flush=True)
for line in sys.stdin:
 request=json.loads(line)
 if request['query']=='hang': time.sleep(10)
 print(json.dumps({'items':[],'edges':[]}),flush=True)
''')
        host.chmod(0o755)
        backend=GraphSearch(self.root,host)
        try:
            with self.assertRaises(TimeoutError): backend.search('hang',timeout=.1)
            result=trial(self.task,backend)
            self.assertEqual(result['errors'],[])
            self.assertEqual(len(result['backend_restarts']),1)
            self.assertTrue(result['backend_restarts'][0]['succeeded'])
            self.assertGreater(result['backend_restarts'][0]['elapsed_ms'],0)
            backend.close();backend.close()
        finally:
            backend.close()

    def test_dead_host_and_buffered_broken_pipe_cleanup(self):
        host=self.root/'host'
        host.write_text('#!'+sys.executable+'''\nimport json,time
print(json.dumps({'ready':True,'setup_ms':0}),flush=True)
time.sleep(10)
''');host.chmod(0o755)
        backend=GraphSearch(self.root,host)
        backend.process.kill();backend.process.wait()
        backend.process.stdin.write(b'buffered data')
        backend.close();backend.close()
        self.assertFalse(Path(backend.tmp.name).exists())

    def test_cleanup_failure_does_not_skip_artifacts_or_other_backends(self):
        calls=[]
        class Broken:
            def close(self): calls.append('broken');raise BrokenPipeError('fixture failure')
        class Good:
            def close(self): calls.append('good')
        finalize(self.root,{},[],{('fixture','broken'):Broken(),('fixture','good'):Good()})
        self.assertEqual(calls,['broken','good'])
        self.assertTrue((self.root/'report.json').exists())
        self.assertEqual(len(json.loads((self.root/'manifest.json').read_text())['cleanup_errors']),1)

    def test_eof_before_exit_timeout_becomes_trial_error(self):
        argv=[sys.executable,'-c','import os,time;os.close(1);os.close(2);time.sleep(10)']
        with self.assertRaises(TimeoutError): command(argv,cwd=self.root,timeout=.2)
        result=trial(self.task,self.backend,driver=argv,wall_seconds=.2)
        self.assertEqual(result['status'],'driver_error')
        self.assertFalse(result['task_success'])
        self.assertEqual(result['driver_steps'],1)
        self.assertIsNone(result['provider_tokens'])
        # A new trial still runs and can produce an answer.
        next_result=trial(self.task,self.backend,driver=[sys.executable,'-c','print(\'{"answer":"ok"}\')'])
        self.assertEqual(next_result['status'],'answered')
        self.assertIsNone(next_result['task_success'])

    def test_failed_decision_marks_usage_incomplete(self):
        (self.root/'x').write_text('hello\n')
        for failure in ['print("bad json")','sys.exit(2)','print(json.dumps({"answer":"ok"}))','print(json.dumps({"answer":"ok","usage":{"input_tokens":-1,"output_tokens":0}}))']:
            with self.subTest(failure=failure):
                driver=self.root/'driver.py'
                driver.write_text('import sys,json\nr=json.load(sys.stdin)\nif not r["history"]:\n print(json.dumps({"action":{"name":"read","arguments":{"path":"x","start":1}},"usage":{"input_tokens":10,"output_tokens":5}}))\nelse:\n '+failure+'\n')
                result=trial(self.task,self.backend,driver=[sys.executable,str(driver)])
                self.assertEqual(result['driver_steps'],2)
                self.assertIsNone(result['provider_tokens'])
                self.assertFalse(result['provider_usage_complete'])
                self.assertEqual(result['known_provider_tokens'],dict(input_tokens=10,output_tokens=5))

    def test_terminal_failures_count_pending_and_evidence_do_not(self):
        failure=trial(self.task,self.backend,driver=[sys.executable,'-c','print("bad")'])
        success={**failure,'task_id':'y','status':'answered','errors':[],'task_success':True}
        for status in ['driver_error','call_budget','context_budget','wall_budget']:
            with self.subTest(status=status):
                # Legacy null failures must also enter the denominator.
                terminal={**failure,'status':status,'task_success':None}
                records=[terminal,success]
                row=report(records)['rows'][0]
                self.assertEqual(row['task_success_rate'],.5)
                self.assertEqual(row['graded_answer_success_rate'],1)
                self.assertEqual(row['graded'],1)
                self.assertEqual(row['terminal_agent_failures'],1)
                other=[{**v,'arm':'other','status':'answered','task_success':True} for v in records]
                pair=report(records+other)['paired'][0]
                self.assertEqual(pair['paired_resolved_agent_trials'],2)
                self.assertEqual(abs(pair['success_delta_left_minus_right']),.5)
        pending={**success,'task_id':'pending','task_success':None}
        row=report([failure,success,pending])['rows'][0]
        self.assertIsNone(row['task_success_rate'])
        self.assertEqual(row['pending_agent_answers'],1)
        evidence=trial(self.task,self.backend)
        self.assertIsNone(evidence['task_success'])
        self.assertIsNone(report([evidence])['rows'][0]['task_success_rate'])

if __name__=='__main__': unittest.main()
