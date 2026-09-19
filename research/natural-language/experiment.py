"""Offline field/expansion/diversity ablations; gold is used only for scoring."""
from __future__ import annotations
import argparse
from collections import Counter, defaultdict
import hashlib
import json
import math
from pathlib import Path
import re
import statistics
import time

BASE=Path(__file__).resolve().parent
ROOT=BASE.parents[1]
STOP=set('how does the a an is are in to of and where what for with'.split())
DOC_EXT={'.md','.mdx','.rst','.txt','.adoc'}


def tokens(text):
    # Same identifier boundaries as core::lexical::tokens.
    words=[];word=''
    for i,c in enumerate(text):
        if not c.isalnum():
            if word: words.append(word);word=''
            continue
        previous=text[i-1] if i else None
        following=text[i+1] if i+1<len(text) else None
        if c.isupper() and previous and (previous.islower() or previous.isnumeric() or (previous.isupper() and following and following.islower())):
            if word: words.append(word);word=''
        word+=c.lower()
    if word:words.append(word)
    return words


def terms(query):
    return sorted(set(t for t in tokens(query) if len(query.split())<=1 or t not in STOP))


def exact(query):
    return re.sub(r'^[\W]+|[\W]+$','',query.strip()).lower()


def stem(term):
    """Frozen inflection-only expansion; no learned/task-specific synonyms."""
    if len(term)>5 and term.endswith('ies'):return term[:-3]+'y'
    if len(term)>5 and term.endswith('ing'):
        base=term[:-3]
        if len(base)>2 and base[-1]==base[-2] and base[-1] in 'bdgmnpt':base=base[:-1]
        return base.rstrip('e')
    if len(term)>4 and term.endswith('ed'):return term[:-2].rstrip('e')
    if len(term)>4 and term.endswith('es'):return term[:-2].rstrip('e')
    if len(term)>3 and term.endswith('s') and not term.endswith('ss'):return term[:-1].rstrip('e')
    return term.rstrip('e') if len(term)>3 else term


class BM25:
    def __init__(self, documents, offset=0):
        self.offset=offset
        self.postings=defaultdict(list);self.length=[]
        for i,fields in enumerate(documents):
            counts=Counter();length=0
            for text,weight in fields:
                for token in tokens(text):
                    if token not in STOP:counts[token]+=weight;length+=1
            self.length.append(length)
            for term,count in counts.items():self.postings[term].append((i,count))
        self.average=max(1,sum(self.length)/max(1,len(self.length)))
        self.stems=defaultdict(list)
        for term in self.postings:self.stems[stem(term)].append(term)

    def scores(self, query, expand=False):
        scores=defaultdict(float);count=len(self.length)
        query_terms={term:1.0 for term in query}
        if expand:
            for term in query:
                for alternative in self.stems.get(stem(term),[]):
                    query_terms.setdefault(alternative,.5)
        for term,weight in query_terms.items():
            hits=self.postings.get(term,[]);df=len(hits)
            if not df:continue
            idf=max(.000001,math.log((count-df+.5)/(df+.5)))
            for i,tf in hits:
                scores[i+self.offset]+=weight*idf*tf*2.2/(tf+1.2*(.25+.75*self.length[i]/self.average))
        return scores


def read_corpus(repo):
    root=Path.home()/'code'/repo
    dump=json.loads((BASE/'private'/f'{repo}-graph.json').read_text())
    nodes=[n for n in dump['nodes'] if n['kind']!='file']
    files={n['path'] for n in dump['nodes'] if n['kind']=='file'}
    texts={};fingerprints={};truncated=[]
    for path in sorted(files):
        source=root/path
        if not source.resolve().is_relative_to(root.resolve()) or not source.is_file():continue
        with source.open('rb') as stream:data=stream.read(1024*1024+1)
        if len(data)>1024*1024:truncated.append(path);data=data[:1024*1024]
        try:text=data.decode('utf-8')
        except UnicodeDecodeError:continue
        texts[path]=text.splitlines()
        fingerprints[path]=hashlib.sha256(data).hexdigest()
    candidates=[];metadata=[];comments=[];bodies=[];docs=[]
    for node in nodes:
        span=node.get('span') or {};start=span.get('start_line',1);end=span.get('end_line',start)
        lines=texts.get(node['path'],[])
        leading=[]
        for line in reversed(lines[max(0,start-33):max(0,start-1)]):
            stripped=line.strip()
            if stripped.startswith('#['):continue
            if stripped.startswith(('//','/*','*','*/')):leading.append(line)
            else:break
        comment='\n'.join(reversed(leading))[:2048]
        body='\n'.join(lines[max(0,start-1):min(end,start+63)])[:4096] if node['kind'] in {'function','method'} else ''
        candidates.append({'id':node['id'],'path':node['path'],'name':node.get('name'),'qualified_name':node.get('qualified_name'),'start':start,'end':end,'kind':node['kind']})
        metadata.append([(node.get('name') or '',8),(node['path'],2),(node.get('signature') or '',1)])
        comments.append([(comment,1)]);bodies.append([(body,1)]);docs.append([])
    symbol_count=len(candidates)
    for path,lines in texts.items():
        if Path(path).suffix.lower() not in DOC_EXT:continue
        limited='\n'.join(lines).encode('utf-8')[:65536].decode('utf-8',errors='ignore').splitlines()
        for offset in range(0,len(limited),64):
            passage='\n'.join(limited[offset:offset+64])[:4096]
            candidates.append({'id':f'doc:{path}:{offset+1}','path':path,'name':None,'qualified_name':None,'start':offset+1,'end':min(len(limited),offset+64),'kind':'file'})
            metadata.append([]);comments.append([]);bodies.append([]);docs.append([(passage,1)])
    # Metadata statistics must retain the original symbol-only document universe.
    indexes={'metadata':BM25(metadata[:symbol_count]),'comments':BM25(comments[:symbol_count]),'bodies':BM25(bodies[:symbol_count]),'docs':BM25(docs[symbol_count:],offset=symbol_count)}
    return candidates,indexes,{'symbol_count':symbol_count,'documentation_passages':len(candidates)-symbol_count,'source_prefix_sha256':fingerprints,'source_files_truncated':truncated}


def rank(candidates,indexes,query,configuration):
    qterms=terms(query);weights=configuration['weights'];expand=configuration.get('expansion',False)
    scores=defaultdict(float)
    for field,weight in weights.items():
        for i,value in indexes[field].scores(qterms,expand).items():scores[i]+=weight*value
    literal=exact(query)
    exacts={i for i,c in enumerate(candidates) if any(name and name.lower()==literal for name in (c['name'],c['qualified_name']))}
    for i in exacts:scores.setdefault(i,0)
    pending=set(scores);result=[];file_counts=Counter();diversity=configuration.get('diversity',1.0)
    # Deduplicate documentation passages by file only after selecting their best hit.
    while pending and len(result)<8:
        def key(i):
            c=candidates[i]
            return (-(i in exacts),0 if i in exacts else -scores[i]*(diversity**file_counts[c['path']]),c['path'],c['start'],c['id'])
        best=min(pending,key=key);pending.remove(best);c=candidates[best]
        result.append(c);file_counts[c['path']]+=1
        if c['kind']=='file':pending={i for i in pending if not(candidates[i]['kind']=='file' and candidates[i]['path']==c['path'])}
    return result


def evaluate(candidates,indexes,tasks,oracles,configuration):
    records=[]
    for task in tasks:
        start=time.perf_counter();hits=rank(candidates,indexes,task['prompt'],configuration)
        oracle=oracles[task['id']];paths={r['path'] for r in oracle['regions']}
        ranks=[next((i+1 for i,c in enumerate(hits) if c['path']==path),None) for path in paths]
        records.append({'id':task['id'],'repo':task['repo'],'split':task['split'],'kind':task['kind'],
            'file_recall':sum(r is not None for r in ranks)/len(ranks),'reciprocal_rank':sum(1/r if r else 0 for r in ranks)/len(ranks),
            'region_overlap':sum(any(c['path']==r['path'] and c['start']<=r['end'] and c['end']>=r['start'] for c in hits) for r in oracle['regions'])/len(oracle['regions']),
            'elapsed_ms':(time.perf_counter()-start)*1000,'hits':hits})
    return records


def summary(records):
    return {key:statistics.mean(r[key] for r in records) for key in ('file_recall','reciprocal_rank','region_overlap','elapsed_ms')}


def configurations():
    arms={'metadata':{'weights':{'metadata':1}}}
    for fields in [('comments',),('bodies',),('docs',),('comments','bodies','docs')]:
        for weight in [.25,.5,1.0]:
            arms['+'.join(fields)+f'@{weight}']={'weights':{'metadata':1,**{f:weight for f in fields}}}
    return arms


def main():
    parser=argparse.ArgumentParser();parser.add_argument('stage',choices=['fields','expansion-diversity','combined','confirm']);args=parser.parse_args()
    tasks=json.loads((ROOT/'evaluation/tasks.json').read_text());oracles=json.loads((ROOT/'evaluation/oracles.json').read_text())
    if args.stage=='fields':arms=configurations()
    elif args.stage=='expansion-diversity':
        selection=json.loads((BASE/'results/field-selection.json').read_text());base=selection['configuration']
        arms={'selected_content':base,'expansion':{**base,'expansion':True},'diversity@0.5':{**base,'diversity':.5},'diversity@0.25':{**base,'diversity':.25}}
    elif args.stage=='combined':
        base=json.loads((BASE/'results/field-selection.json').read_text())['configuration']
        selected=json.loads((BASE/'results/final-selection.json').read_text())['configuration']
        arms={'selected_individual':selected,'expanded-diversity@0.5':{**base,'expansion':True,'diversity':.5},'expanded-diversity@0.25':{**base,'expansion':True,'diversity':.25}}
    else:
        selection=json.loads((BASE/'results/final-selection.json').read_text())
        arms={'metadata':configurations()['metadata'],'selected':selection['configuration']}
    results={name:[] for name in arms};provenance={};documentation={name:[] for name in arms}
    doc_queries=json.loads((BASE/'documentation-queries.json').read_text())
    for repo in ['nanus','blogwright','whatsurvey']:
        candidates,indexes,provenance[repo]=read_corpus(repo)
        selected=[t for t in tasks if t['repo']==repo and t['split']==('heldout' if args.stage=='confirm' else 'dev')]
        for name,configuration in arms.items():
            records=evaluate(candidates,indexes,selected,oracles,configuration);results[name]+=records
            for q in doc_queries:
                if q['repo']!=repo or q['split']!=('heldout' if args.stage=='confirm' else 'dev'):continue
                source=(Path.home()/'code'/repo/q['expected_path']).read_bytes()
                assert hashlib.sha256(source).hexdigest()==q['source_sha256']
                hits=rank(candidates,indexes,q['prompt'],configuration)
                hit_rank=next((i+1 for i,c in enumerate(hits) if c['path']==q['expected_path']),None)
                documentation[name].append({**q,'rank':hit_rank,'hits':hits})
            print(repo,name,summary(records),flush=True)
    report={'stage':args.stage,'configurations':arms,'summary':{name:summary(records) for name,records in results.items()},'records':results,'documentation':documentation,'provenance':provenance}
    (BASE/'results'/f'{args.stage}.json').write_text(json.dumps(report,indent=2)+'\n')
    if args.stage!='confirm':
        best=max(arms,key=lambda name:(report['summary'][name]['file_recall'],report['summary'][name]['reciprocal_rank'],-sum(arms[name]['weights'].values())))
        target='field-selection.json' if args.stage=='fields' else 'final-selection.json'
        (BASE/'results'/target).write_text(json.dumps({'name':best,'configuration':arms[best],'summary':report['summary'][best],'selection':'development file recall, then reciprocal rank, then smaller total field weight'},indent=2)+'\n')
        print('Selected:',best,flush=True)

if __name__=='__main__':main()
