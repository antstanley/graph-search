import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from taskbench.core import digest


class CliTests(unittest.TestCase):
    def test_agent_to_blind_grade_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            base=Path(temporary);repo=base/'repo';repo.mkdir()
            subprocess.run(['git','init','-q',str(repo)],check=True)
            subprocess.run(['git','-C',str(repo),'-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','--allow-empty','-qm','fixture'],check=True)
            (repo/'x.rs').write_text('fn f() {}\n')
            tasks=[dict(id='fixture.f.debug',repo='fixture',family='f',split='dev',kind='debug',prompt='Explain this function')]
            oracle={'fixture.f.debug':dict(regions=[dict(id='r',path='x.rs',start=1,end=1,file_sha256=digest(b'fn f() {}\n'),sha256=digest(b'fn f() {}\n'))],criteria=[dict(id='c',description='Function has empty body',regions=['r'])],relationships=[])}
            tasks.append({**tasks[0],'id':'fixture.f.failure'})
            oracle['fixture.f.failure']=oracle['fixture.f.debug']
            for name,value in [('tasks',tasks),('oracles',oracle),('roots',{'fixture':str(repo)})]:
                (base/(name+'.json')).write_text(json.dumps(value))
            driver=base/'driver.py'
            driver.write_text('''import json,sys
x=json.load(sys.stdin)
if x['task']['id'].endswith('failure'): print('bad json');sys.exit(0)
if not x['history']: print(json.dumps({'action':{'name':'read','arguments':{'path':'x.rs','start':1}}}))
else: print(json.dumps({'answer':'The function is empty.','citations':[{'path':'x.rs','line':1}]}))
''')
            env={**os.environ,'PYTHONPATH':str(Path(__file__).resolve().parents[1])}
            def cli(*args,success=True):
                result=subprocess.run([sys.executable,'-m','taskbench',*map(str,args)],env=env,capture_output=True,text=True)
                self.assertEqual(result.returncode==0,success,result.stderr)
            run=base/'run';packets=base/'packets'
            cli('run','--roots',base/'roots.json','--tasks',base/'tasks.json','--oracles',base/'oracles.json','--arms','text','--output',run,'--agent-command',json.dumps([sys.executable,str(driver)]),'--model','scripted-fixture','--effort','none','--calls','1')
            cli('blind',run,'--output',packets)
            path=next(packets.glob('*.judgments.json'));judgment=json.loads(path.read_text())
            cli('apply-grades',run,packets,'--output',base/'rejected',success=False)
            judgment.update(reviewer='fixture-test',criteria={'c':True},unsupported_claims=False)
            path.write_text(json.dumps(judgment))
            cli('apply-grades',run,packets,'--output',base/'graded')
            results=json.loads((base/'graded/results.json').read_text())
            by_task={r['task_id']:r for r in results}
            self.assertTrue(by_task['fixture.f.debug']['task_success'])
            self.assertFalse(by_task['fixture.f.failure']['task_success'])
            original={r['task_id']:r for r in json.loads((run/'results.json').read_text())}
            self.assertIsNone(original['fixture.f.debug']['task_success'])
            self.assertFalse(original['fixture.f.failure']['task_success'])
            row=json.loads((base/'graded/report.json').read_text())['rows'][0]
            self.assertEqual(row['task_success_rate'],.5)
            self.assertEqual(row['graded'],1)
            self.assertEqual(row['resolved_agent_trials'],2)
            self.assertTrue(json.loads((run/'manifest.json').read_text())['source_validation'].endswith('passed'))

if __name__=='__main__':unittest.main()
