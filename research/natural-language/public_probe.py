"""Compare pinned baseline and current core query engines on frozen labels.

Build the baseline search-research binary at bdcbece and copy it into private/;
build current research/harness using CARGO_TARGET_DIR below. No source snippets
or full graph dumps are committed. Source files/indexes are never modified.
"""
import argparse
import shutil
import tempfile
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import sys
import time
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'evaluation'))
from taskbench.core import canonical, validate
from taskbench.provenance import repository_snapshot
from experiment import BASE, ROOT


def requests(repo,tasks,docs):
    items=[]
    for label,suffix in [('discovery','queries'),('expanded','expanded-queries'),('natural','natural-queries'),('heldout-split','heldout-queries')]:
        group=json.loads((ROOT/'research/results'/f'{repo}-{suffix}.json').read_text())
        if label=='discovery':group=group[:10]
        for q in group:
            items.append({**q,'group':label if label!='expanded' else q['category']})
    for task in tasks:
        if task['repo']==repo:
            regions=ORACLES[task['id']]['regions']
            items.append({'mode':'explore','query':task['prompt'],'group':'task-'+task['split'],'task_id':task['id'],'expected_path':regions[0]['path']})
    for i,doc in enumerate(docs):
        if doc['repo']==repo:
            items.append({'mode':'explore','query':doc['prompt'],'group':'documentation-'+doc['split'],'expected_path':doc['expected_path'],'documentation_id':i})
    return items


TASKS=json.loads((ROOT/'evaluation/tasks.json').read_text())
ORACLES=json.loads((ROOT/'evaluation/oracles.json').read_text())
DOCS=json.loads((BASE/'documentation-queries.json').read_text())
ROOTS={r:Path.home()/'code'/r for r in ['nanus','blogwright','whatsurvey']}


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--baseline', type=Path, default=BASE/'private/baseline-search-research')
    parser.add_argument('--current', type=Path, default=Path('/private/tmp/graph-search-natural-language-target/debug/search-research'))
    args=parser.parse_args()
    validate(TASKS,ORACLES,ROOTS)
    # Copy executables so rebuilding either arm cannot silently change this run.
    temporary=tempfile.TemporaryDirectory(prefix='nl-probe-binaries-')
    binaries={name:Path(temporary.name)/name for name in ['baseline','current']}
    for name, source in [('baseline',args.baseline),('current',args.current)]:
        shutil.copy2(source,binaries[name])
    output={'base_commit':'bdcbece','binaries':{name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in binaries.items()},'repos':{}}
    body_stats={}
    for repo,root in ROOTS.items():
        before=repository_snapshot(root)
        queries=requests(repo,TASKS,DOCS)
        qp=BASE/'private'/f'{repo}-frozen-requests.json';qp.write_text(json.dumps(queries))
        output['repos'][repo]={'before':before,'arms':{}}
        for arm,binary in binaries.items():
            destination=BASE/'private'/f'{repo}-{arm}-frozen.json'
            with destination.open('w') as stream:
                subprocess.run([str(binary),str(root),str(qp)],stdout=stream,check=True,timeout=1200)
            data=json.loads(destination.read_text());records=[]
            if arm=='current':
                body_nodes=[n for n in data['nodes'] if n['kind'] in ('function','method')]
                bags=[json.loads(n['attributes']['graph_search.body_terms.v1']) for n in body_nodes if 'graph_search.body_terms.v1' in n['attributes']]
                body_stats[repo]={'functions_methods':len(body_nodes),'with_body_terms':len(bags),
                    'truncated':sum(bag['truncated'] for bag in bags),
                    'unique_term_entries':sum(len(bag['terms']) for bag in bags),
                    'node_attribute_bytes':sum(len(n['attributes'].get('graph_search.body_terms.v1','').encode()) for n in body_nodes),
                    'missing_attribute':[{'name':n['name'],'path':n['path'],'span':n['span']} for n in body_nodes if 'graph_search.body_terms.v1' not in n['attributes']]}
            parser_versions={n['parser_version'] for n in data['nodes'] if n['kind']=='file'}
            assert parser_versions==({2} if arm=='baseline' else {3}),parser_versions
            for item in data['queries']:
                q=item['request'];result=item['result']
                hits=[{'id':h['node']['id'],'path':h['node']['path'],'name':h['node'].get('name'),'start':h['node'].get('start_line'),'end':h['node'].get('end_line')} for h in result.get('items',[])[:8]]
                rank=next((i+1 for i,h in enumerate(hits) if h['path']==q['expected_path'] and ('expected_name' not in q or h['name']==q['expected_name'])),None)
                records.append({'request':q,'rank':rank,'hits':hits,'elapsed_us':item['elapsed_us'],'error':result.get('error'),'truncations':result.get('truncations',[])})
            groups={g:[r for r in records if r['request']['group']==g] for g in sorted({r['request']['group'] for r in records})}
            summary={g:{'hits':sum(r['rank'] is not None for r in rs),'count':len(rs),'mrr':statistics.mean(1/r['rank'] if r['rank'] else 0 for r in rs),'median_us':statistics.median(r['elapsed_us'] for r in rs)} for g,rs in groups.items()}
            output['repos'][repo]['arms'][arm]={'summary':summary,'records':records,'index_report':data['report'],
                'node_count':len(data['nodes']),'body_term_attribute_bytes':sum(len(n.get('attributes',{}).get('graph_search.body_terms.v1','').encode()) for n in data['nodes'])}
            print(repo,arm,summary,flush=True)
            (BASE/'results/frozen-public-core.json').write_text(json.dumps(output,indent=2)+'\n')
        after=repository_snapshot(root);assert before==after,repo
        output['repos'][repo]['after']=after
    validate(TASKS,ORACLES,ROOTS)
    output['source_validation']='before and after passed'
    temporary.cleanup()
    (BASE/'results/body-index-stats.json').write_text(json.dumps(body_stats,indent=2)+'\n')
    (BASE/'results/frozen-public-core.json').write_text(json.dumps(output,indent=2)+'\n')

if __name__=='__main__':main()
