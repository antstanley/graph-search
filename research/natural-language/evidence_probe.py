"""Development-only evidence protocol on prototype rankings, not public-engine claims."""
import json
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'evaluation'))
from taskbench.core import coverage, read_source, validate
from taskbench.runner import trial, sanitized
from experiment import BASE, ROOT, read_corpus, rank


class Prototype:
    name='prototype'
    def __init__(self,repo,candidates,indexes,configuration):
        self.root=Path.home()/'code'/repo
        self.candidates=candidates;self.indexes=indexes;self.configuration=configuration
    def search(self,query,**kwargs):
        lines=[]
        for item in rank(self.candidates,self.indexes,query,self.configuration):
            lines.append(f"candidate {item['path']}:{item['start']} {item['name']}")
            lines.append(read_source(self.root,item['path'],max(1,item['start']-1),4))
        return '\n'.join(lines)


def main():
    tasks=json.loads((ROOT/'evaluation/tasks.json').read_text());oracles=json.loads((ROOT/'evaluation/oracles.json').read_text())
    roots={r:Path.home()/'code'/r for r in ['nanus','blogwright','whatsurvey']}
    validate(tasks,oracles,roots)
    arms={'metadata':{'weights':{'metadata':1}},'bodies':json.loads((BASE/'results/field-selection.json').read_text())['configuration'],
          'selected':json.loads((BASE/'results/final-selection.json').read_text())['configuration']}
    results=[]
    for repo in roots:
        candidates,indexes,_=read_corpus(repo)
        for name,configuration in arms.items():
            backend=Prototype(repo,candidates,indexes,configuration)
            for task in [t for t in tasks if t['repo']==repo and t['split']=='dev']:
                result=trial(task,backend);result['arm']=name
                result['evidence']=coverage(oracles[task['id']],{tuple(x) for x in result['seen']})
                results.append(sanitized(result))
    validate(tasks,oracles,roots)
    (BASE/'results/development-evidence-proxy.json').write_text(json.dumps(results,indent=2)+'\n')
    for arm in arms:
        subset=[r for r in results if r['arm']==arm]
        print(arm,'files',sum(r['evidence']['required_file_recall']==1 for r in subset),'ready',sum(r['evidence']['evidence_ready'] for r in subset),'errors',sum(bool(r['errors']) for r in subset))

if __name__=='__main__':main()
