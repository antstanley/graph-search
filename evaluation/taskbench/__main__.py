"""Run with PYTHONPATH=evaluation python3 -m taskbench --help."""
import argparse
from collections import defaultdict
import json
import itertools
from pathlib import Path
import random
import statistics
import subprocess
import time

from .backends import BACKENDS, Unavailable
from .core import canonical, coverage, digest, grade, grading_packet, validate
from .runner import sanitized, trial
from .provenance import implementation, index_freshness, repository_snapshot

BASE=Path(__file__).resolve().parents[1]


def load(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    path=Path(path);path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps(value,indent=2,ensure_ascii=False)+'\n')


def agent_outcome(item):
    """Terminal agent failures count even in older runs that stored null."""
    if item.get('protocol')!='agent' or item['status']=='unavailable':
        return None
    if item['status'] in {'call_budget','context_budget','wall_budget','driver_error'}:
        return False
    return item.get('task_success')


def finalize(output, manifest, results, backends):
    manifest['cleanup_errors']=[]
    for key,backend in backends.items():
        try:
            backend.close()
        except Exception as error:
            manifest['cleanup_errors'].append({'backend':'/'.join(key),'error':str(error)})
    try:
        write(output/'manifest.json',manifest)
    finally:
        write(output/'report.json',report(results))


def report(records):
    groups=defaultdict(list)
    for item in records:
        groups[(item['repo'],item['split'],item['kind'],item['arm'])].append(item)
    rows=[]
    for key,items in sorted(groups.items()):
        available=[v for v in items if v['status']!='unavailable']
        scored=[v for v in available if 'evidence' in v]
        agents=[v for v in available if v.get('protocol')=='agent']
        resolved=[v for v in agents if agent_outcome(v) is not None]
        graded=[v for v in agents if v['status']=='answered' and v.get('task_success') is not None]
        rows.append(dict(zip(('repo','split','kind','arm'),key)) | {
            'scheduled':len(items),'available':len(available),'evidence_scored':len(scored),
            'error_trials':sum(bool(v.get('errors')) for v in available),
            'required_file_recall_mean':statistics.mean(v['evidence']['required_file_recall'] for v in scored) if scored else None,
            'evidence_ready_rate':statistics.mean(v['evidence']['evidence_ready'] for v in scored) if scored else None,
            'graded':len(graded),
            'graded_answer_success_rate':statistics.mean(v['task_success'] for v in graded) if graded else None,
            'agent_trials':len(agents),'resolved_agent_trials':len(resolved),
            'terminal_agent_failures':sum(v['status']!='answered' for v in resolved),
            'pending_agent_answers':len(agents)-len(resolved),
            'task_success_rate':statistics.mean(agent_outcome(v) for v in agents) if agents and len(resolved)==len(agents) else None,
            **{field+'_median':statistics.median(v[field] for v in available) if available else None for field in ('calls','wall_ms','response_bytes')},
        })
    paired=[]
    cohorts=defaultdict(dict)
    for item in records:
        cohorts[(item['repo'],item['split'],item['kind'])].setdefault(item['arm'],{})[(item['task_id'],item.get('repeat',0))]=item
    for cohort,arms in sorted(cohorts.items()):
        for left,right in itertools.combinations(sorted(arms),2):
            pairs=[(arms[left][k],arms[right][k]) for k in arms[left].keys() & arms[right].keys()
                   if arms[left][k]['status']!='unavailable' and arms[right][k]['status']!='unavailable']
            evidence=[(a,b) for a,b in pairs if 'evidence' in a and 'evidence' in b]
            agents=[(a,b) for a,b in pairs if a.get('protocol')=='agent' and b.get('protocol')=='agent']
            resolved=[(a,b) for a,b in agents if agent_outcome(a) is not None and agent_outcome(b) is not None]
            graded=[(a,b) for a,b in agents if a['status']==b['status']=='answered' and a.get('task_success') is not None and b.get('task_success') is not None]
            paired.append(dict(zip(('repo','split','kind'),cohort)) | {'left':left,'right':right,
                'paired_available':len(pairs),'paired_evidence':len(evidence),'paired_graded':len(graded),
                'paired_agent_trials':len(agents),'paired_resolved_agent_trials':len(resolved),
                'file_recall_delta_left_minus_right':statistics.mean(a['evidence']['required_file_recall']-b['evidence']['required_file_recall'] for a,b in evidence) if evidence else None,
                'success_delta_left_minus_right':statistics.mean(int(agent_outcome(a))-int(agent_outcome(b)) for a,b in agents) if agents and len(resolved)==len(agents) else None})
    return {'rows':rows,'paired':paired,'interpretation':'Evidence coverage is a retrieval proxy, not task success. Missing CodeGraph indexes are unavailable, not misses. Terminal unanswered agent trials are failures. End-to-end success and paired deltas remain null until all available agent answers are graded; conditional graded-answer rates are separate.'}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    for name in ('validate','run'):
        p=sub.add_parser(name)
        p.add_argument('--roots',type=Path,required=True,help='JSON mapping repository names to local paths')
        p.add_argument('--tasks',type=Path,default=BASE/'tasks.json')
        p.add_argument('--oracles',type=Path,default=BASE/'oracles.json')
        if name=='run':
            p.add_argument('--split',choices=['dev','heldout','all'],default='dev')
            p.add_argument('--allow-heldout',action='store_true')
            p.add_argument('--arms',nargs='+',choices=list(BACKENDS),default=list(BACKENDS))
            p.add_argument('--output',type=Path,required=True)
            p.add_argument('--host',type=Path,default=BASE/'harness/target/debug/task-eval-host')
            p.add_argument('--agent-command',help='JSON argv array; no shell interpolation')
            p.add_argument('--model',help='Required for agent trials; record exact provider/model version')
            p.add_argument('--effort',help='Required for agent trials; record reasoning effort/configuration')
            p.add_argument('--calls',type=int,default=4)
            p.add_argument('--response-bytes',type=int,default=16384)
            p.add_argument('--context-bytes',type=int,default=49152)
            p.add_argument('--wall-seconds',type=float,default=180)
            p.add_argument('--seed',type=int,default=1729)
            p.add_argument('--repeats',type=int,default=1)
            p.add_argument('--task-id',action='append',help='Select exact task IDs for smoke runs')
    p=sub.add_parser('report');p.add_argument('results',type=Path);p.add_argument('--output',type=Path,required=True)
    p=sub.add_parser('blind');p.add_argument('run',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--seed',type=int,default=19)
    p=sub.add_parser('grade');p.add_argument('packet',type=Path);p.add_argument('judgments',type=Path);p.add_argument('--output',type=Path,required=True)
    p=sub.add_parser('apply-grades');p.add_argument('run',type=Path);p.add_argument('packets',type=Path);p.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if args.command=='apply-grades':
        if args.output.exists():
            raise ValueError('grading output must be a fresh directory')
        mapping=load(args.run/'grading-map.json');records=load(args.run/'results.json')
        by_id={r['trial_id']:r for r in records};applied=set()
        for judgment_path in sorted(args.packets.glob('*.judgments.json')):
            judgment=load(judgment_path)
            packet_path=judgment_path.with_name(judgment_path.name.replace('.judgments.json','.json'))
            packet=load(packet_path)
            key=packet['packet_sha256']
            if key not in mapping or key in applied:
                raise ValueError('unknown or duplicate grading packet')
            linked=mapping[key]
            transcript=load(args.run/'trials'/(linked['trial_id']+'.json'))
            if digest(canonical(transcript).encode())!=packet['trial_sha256']:
                raise ValueError('trial changed since packet creation')
            result=grade(packet,judgment)
            by_id[linked['trial_id']]['task_success']=result['task_success']
            by_id[linked['trial_id']]['grade']=result
            applied.add(key)
        if not applied:
            raise ValueError('no completed judgments found')
        write(args.output/'results.json',records);write(args.output/'report.json',report(records));return
    if args.command=='report':
        write(args.output,report(load(args.results)));return
    if args.command=='grade':
        write(args.output,grade(load(args.packet),load(args.judgments)));return
    if args.command=='blind':
        if args.output.exists():
            raise ValueError('review packet output must be a fresh directory')
        tasks={t['id']:t for t in load(args.run/'tasks.json')};oracles=load(args.run/'oracles.json')
        trials=[load(p) for p in sorted((args.run/'trials').glob('*.json'))]
        trials=[t for t in trials if t.get('answer')]
        random.Random(args.seed).shuffle(trials)
        mapping={}
        for number,t in enumerate(trials):
            identifier=f'packet-{number:04d}'
            packet=grading_packet(tasks[t['task_id']],oracles[t['task_id']],t)
            write(args.output/(identifier+'.json'),packet)
            write(args.output/(identifier+'.judgments.json'),{'packet_sha256':packet['packet_sha256'],'reviewer':'','criteria':{c['id']:None for c in packet['oracle']['criteria']},'unsupported_claims':None})
            mapping[packet['packet_sha256']]={'trial_id':t['trial_id'],'arm':t['arm'],'task_id':t['task_id']}
        # Mapping stays beside raw run, never in reviewer directory.
        write(args.run/'grading-map.json',mapping)
        print(f'{len(trials)} answer packets exported; do not give graders the run directory');return
    roots={key:Path(value).expanduser().resolve() for key,value in load(args.roots).items()}
    tasks=load(args.tasks);oracles=load(args.oracles)
    validate(tasks,oracles,roots)
    if args.command=='validate':
        print(f'{len(tasks)} tasks: source hashes, public schema and family splits valid');return
    if args.split!='dev' and not args.allow_heldout:
        parser.error('held-out evaluation requires --allow-heldout; freeze tuning before opening this split')
    if args.repeats<1:
        parser.error('--repeats must be positive')
    driver=json.loads(args.agent_command) if args.agent_command else None
    if driver is not None and (not isinstance(driver,list) or not driver or any(not isinstance(x,str) for x in driver) or not args.model or not args.effort):
        parser.error('agent trials require a nonempty JSON argv array, --model and --effort')
    chosen=[t for t in tasks if (args.split=='all' or t['split']==args.split) and (not args.task_id or t['id'] in args.task_id)]
    if not chosen:
        parser.error('no tasks selected')
    if args.output.exists():
        parser.error('output directory already exists; use a fresh run directory')
    args.output.mkdir(parents=True)
    write(args.output/'tasks.json',tasks);write(args.output/'oracles.json',oracles)
    manifest={'schema':1,'created_unix':time.time(),'seed':args.seed,'split':args.split,'repeats':args.repeats,
              'tasks_sha256':digest(canonical(tasks).encode()),'oracles_sha256':digest(canonical(oracles).encode()),
              'implementation':implementation(BASE,args.host),'driver':driver,'model':args.model,'effort':args.effort,'roots':{},'setups':{},'source_validation':'before passed; after pending'}
    for repo,root in roots.items():
        manifest['roots'][repo]={'path':str(root),'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
                                 'dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=root,text=True)),
                                 'source_snapshot_before':repository_snapshot(root), 'codegraph_index_present':(root/'.codegraph').is_dir(), 'codegraph_freshness_before':index_freshness(root)}
    schedule=[(t,arm,repeat) for t in chosen for arm in args.arms for repeat in range(args.repeats)]
    random.Random(args.seed).shuffle(schedule)
    backends={};unavailable={};results=[]
    try:
        for index,(task,arm,repeat) in enumerate(schedule):
            key=(task['repo'],arm)
            if key not in backends and key not in unavailable:
                try:
                    backends[key]=BACKENDS[arm](roots[task['repo']],host=args.host.resolve())
                    manifest['setups']['/'.join(key)]={'setup_ms':backends[key].setup_ms}
                except (Unavailable,FileNotFoundError) as error:
                    unavailable[key]=str(error)
            trial_id=f'{index:04d}-{task["id"]}-{arm}-{repeat}'
            if key in unavailable:
                result={'trial_id':trial_id,'task_id':task['id'],'repo':task['repo'],'split':task['split'],'kind':task['kind'],'arm':arm,'repeat':repeat,'status':'unavailable','reason':unavailable[key],'task_success':None}
            else:
                result=trial(task,backends[key],driver=driver,max_calls=args.calls,response_bytes=args.response_bytes,
                             context_bytes=args.context_bytes,wall_seconds=args.wall_seconds,
                             driver_metadata={'model':args.model,'effort':args.effort})
                result.update(trial_id=trial_id,repeat=repeat)
                result['evidence']=coverage(oracles[task['id']],{tuple(x) for x in result['seen']})
                write(args.output/'trials'/(trial_id+'.json'),result)
                result=sanitized(result)
            results.append(result)
            write(args.output/'results.json',results)
            print(f'{index+1}/{len(schedule)} {task["id"]} {arm}: {result["status"]}',flush=True)
        validate(tasks,oracles,roots)
        manifest['source_validation']='before and after passed'
        for repo,root in roots.items():
            manifest['roots'][repo]['codegraph_freshness_after']=index_freshness(root)
            after=repository_snapshot(root)
            manifest['roots'][repo]['source_snapshot_after']=after
            if after!=manifest['roots'][repo]['source_snapshot_before']:
                manifest['source_validation']='FAILED: repository changed during trials'
                raise ValueError(f'repository changed during trials: {repo}')
    finally:
        finalize(args.output,manifest,results,backends)


if __name__=='__main__':
    main()
