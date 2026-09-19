"""Check eight small, source-confirmed application relationships; not an exhaustive graph oracle."""
import json,pathlib,hashlib
base=pathlib.Path(__file__).resolve().parents[1]
cases={
 'nanus':[('crates/nanus-bundle/src/agent_loop.rs','AgentRunner::run_turn','AgentRunner::run_step'),('crates/nanus-bundle/src/agent_loop.rs','AgentRunner::run_step','AgentRunner::run_tools'),('crates/nanus-bundle/src/agent_loop.rs','AgentRunner::gate','approval_reason')],
 'blogwright':[('packages/pds/src/sync.ts','syncPds','listPublishablePosts'),('packages/pds/src/sync.ts','requirePdsConfig','resolvePdsSecretName'),('packages/pds/src/secret.ts','loadPdsSecret','parsePdsSecret')],
 'whatsurvey':[('workspaces/backend/src/core/db/survey-versions.ts','saveSurveyDraft','requireSurvey'),('workspaces/backend/src/core/db/survey-versions.ts','publishSurveyDraft','requireSurvey')]
}
results=[]
for repo,rows in cases.items():
 runs={phase:json.loads(pathlib.Path(f'/private/tmp/{repo}-{phase}.json').read_text()) for phase in ['baseline','fixed']}
 for path,source,target in rows:
  record={'repo':repo,'path':path,'source':source,'target':target}
  for phase,data in runs.items():
   nodes={n['id']:n for n in data['nodes']};src=[n for n in nodes.values() if n['path']==path and n['qualified_name']==source]
   assert len(src)==1,(repo,path,source)
   edges=[e for e in data['edges'] if e['from']==src[0]['id'] and e['kind']=='calls' and e['resolved'] and e['to_name']==target]
   record[phase]={'found':bool(edges),'edges':[{'source':e['from'],'target':e['to'],'line':e['line']} for e in edges]}
  text=(pathlib.Path.home()/'code'/repo/path).read_text();lines=text.splitlines();span=src[0]['span'];needle=target.split('::')[-1]
  occurrences=[i+1 for i in range(span['start_line']-1,min(span['end_line'],len(lines))) if needle+'(' in lines[i]]
  assert occurrences,(repo,source,target)
  record['source_occurrence_lines']=occurrences;record['source_sha256']=hashlib.sha256(text.encode()).hexdigest()
  results.append(record)
(base/'results/application-edges.json').write_text(json.dumps(results,indent=2)+'\n')
print('baseline',sum(r['baseline']['found'] for r in results),'fixed',sum(r['fixed']['found'] for r in results),'of',len(results))
