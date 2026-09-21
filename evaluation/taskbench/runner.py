"""Solver actions are planned without oracle access; scoring occurs after trial end."""
from __future__ import annotations
import json
import re
import tempfile
import time
from pathlib import Path
from .backends import command
from .core import bounded, canonical, delivered_lines, digest, explore_items, read_source

TOOLS = [
    {'name':'search','arguments':{'query':'string'}},
    {'name':'read','arguments':{'path':'repository-relative string','start':'positive integer','count':'1..200, default 80'}},
]


def candidates(text):
    result=[]; seen=set()
    items=explore_items(text)
    if items is not None:
        for item in items:
            if not isinstance(item,dict) or not isinstance(item.get('node'),dict):
                continue
            node=item['node'];path=node.get('path');start=node.get('start_line',1)
            if isinstance(path,str) and path and type(start) is int and start>0 and path not in seen:
                result.append((path,start));seen.add(path)
        return result
    for line in text.splitlines():
        match=(re.match(r'^candidate (.+?):(\d+)(?:\t| )',line)
               or re.match(r'^(.+?):(\d+)\t',line))
        if match and match[1] not in seen:
            result.append((match[1],int(match[2])))
            seen.add(match[1])
    return result


def trial(task, backend, *, driver=None, max_calls=4, response_bytes=16384,
          context_bytes=49152, wall_seconds=180, driver_metadata=None):
    """A fresh driver process receives full public history at each decision.

    A driver returns {action:{name,arguments}} or {answer,citations}; usage is an
    optional object containing nonnegative integer input_tokens/output_tokens.
    Drivers are trusted programs, not sandboxed adversarial solvers. Never give
    a driver oracle paths or labels. This runner has no oracle parameter.
    """
    if min(max_calls,response_bytes,context_bytes,wall_seconds)<=0:
        raise ValueError('budgets must be positive')
    started=time.monotonic(); history=[]; seen=set(); answer=None; citations=[]
    context_used=0; model_input_bytes=0; model_ms=0; errors=[]; status='call_budget'
    usage=[]; reads=[]; driver_steps=0
    restart_offset=len(getattr(backend,'restart_events',[]))
    with tempfile.TemporaryDirectory(prefix='task-eval-driver-') as directory:
        for step in range(max_calls+1):
            remaining=wall_seconds-(time.monotonic()-started)
            if remaining<=0:
                status='wall_budget';break
            if driver:
                request={'protocol':1,'task':task,'tools':TOOLS,'history':history,
                         'remaining_calls':max_calls-len(history),'remaining_response_bytes':context_bytes-context_used}
                encoded=canonical(request)
                model_input_bytes+=len(encoded.encode())
                tick=time.monotonic()
                driver_steps+=1
                usage.append(None)
                try:
                    code,raw=command(driver,cwd=directory,stdin=encoded,timeout=min(60,remaining),max_bytes=131072)
                    if code:
                        raise RuntimeError(f'driver exited {code}: {raw[:200]}')
                    decision=json.loads(raw)
                    if not isinstance(decision,dict):
                        raise ValueError('driver response must be an object')
                    supplied=decision.get('usage')
                    if supplied is not None:
                        if set(supplied)!={'input_tokens','output_tokens'} or any(type(v) is not int or v<0 for v in supplied.values()):
                            raise ValueError('invalid provider token usage')
                    usage[-1]=supplied
                    if 'answer' in decision:
                        if not isinstance(decision['answer'],str) or not decision['answer'].strip():
                            raise ValueError('answer must be nonempty text')
                        citations=decision.get('citations',[])
                        if not isinstance(citations,list) or any(not isinstance(c,dict) or set(c)!={'path','line'} or not isinstance(c['path'],str) or type(c['line']) is not int or c['line']<1 for c in citations):
                            raise ValueError('invalid citations')
                        answer=decision['answer'];status='answered';break
                    action=decision['action']
                except (ValueError,KeyError,TypeError,OSError,RuntimeError,TimeoutError) as error:
                    errors.append(str(error));status='driver_error';break
                finally:
                    model_ms+=(time.monotonic()-tick)*1000
            else:
                if step==0:
                    action={'name':'search','arguments':{'query':task['prompt']}}
                elif reads:
                    path,start=reads.pop(0)
                    action={'name':'read','arguments':{'path':path,'start':max(1,start-10),'count':100}}
                else:
                    status='protocol_complete';break
            if len(history)>=max_calls:
                status='call_budget';break
            if context_used>=context_bytes:
                status='context_budget';break
            tick=time.monotonic();error=None
            try:
                if not isinstance(action,dict) or set(action)!={'name','arguments'}:
                    raise ValueError('invalid action envelope')
                args=action['arguments']
                if action['name']=='search':
                    if set(args)!={'query'} or not isinstance(args['query'],str) or not args['query'].strip() or len(args['query'])>8192:
                        raise ValueError('search requires bounded nonempty query')
                    raw=backend.search(args['query'],timeout=min(30,max(.01,wall_seconds-(time.monotonic()-started))))
                elif action['name']=='read':
                    if not {'path','start'}<=set(args) or not set(args)<={'path','start','count'}:
                        raise ValueError('invalid read arguments')
                    raw=read_source(backend.root,**args)
                else:
                    raise ValueError('unknown tool')
            except (ValueError,TypeError,KeyError,OSError,RuntimeError,TimeoutError) as exc:
                error=str(exc);raw='tool error: '+error;errors.append(error)
            elapsed=(time.monotonic()-tick)*1000
            response,truncated=bounded(raw,min(response_bytes,context_bytes-context_used))
            size=len(response.encode());context_used+=size
            seen.update(delivered_lines(response,backend.root))
            history.append({'action':action,'response':response,'response_bytes':size,
                            'truncated':truncated,'elapsed_ms':elapsed,'error':error})
            if not driver and step==0:
                reads=candidates(response)[:max_calls-1]
        else:
            status='call_budget'
    all_usage=bool(usage) and all(item is not None for item in usage)
    return {'task_id':task['id'],'repo':task['repo'],'split':task['split'],'kind':task['kind'],
            'arm':backend.name,'protocol':'agent' if driver else 'evidence-v1',
            'driver_metadata':driver_metadata if driver else None,'status':status,
            'answer':answer,'citations':citations,'seen':sorted(seen),'history':history,'errors':errors,
            'calls':len(history),'wall_ms':(time.monotonic()-started)*1000,'model_ms':model_ms,
            'tool_ms':sum(h['elapsed_ms'] for h in history),'response_bytes':context_used,
            'model_input_bytes':model_input_bytes,'estimated_response_tokens_chars_div_4':sum(len(h['response']) for h in history)/4,
            'provider_tokens':{k:sum(u[k] for u in usage) for k in ('input_tokens','output_tokens')} if all_usage else None,
            'provider_usage_complete':all_usage,
            'known_provider_tokens':{k:sum(u[k] for u in usage if u is not None) for k in ('input_tokens','output_tokens')} if usage else None,
            'backend_restarts':getattr(backend,'restart_events',[])[restart_offset:],
            'driver_steps':driver_steps,'task_success':False if driver and answer is None else None,
            'budgets':dict(max_calls=max_calls,response_bytes=response_bytes,context_bytes=context_bytes,wall_seconds=wall_seconds)}


def sanitized(result):
    """Publish metrics and source locations without external source or model answers."""
    value={k:v for k,v in result.items() if k not in {'history','answer','citations','seen'}}
    value['transcript_sha256']=digest(canonical(result).encode())
    value['has_answer']=bool(result.get('answer'))
    return value
