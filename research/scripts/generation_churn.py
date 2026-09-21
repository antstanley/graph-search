#!/usr/bin/env python3
"""Disposable generation-retention experiment; no production instrumentation."""
import hashlib,json,pathlib,subprocess,tempfile,sys,platform
ROOT=pathlib.Path(__file__).resolve().parents[2]
BIN=ROOT/'research/harness/target/release/generation_churn_probe'
OUT=pathlib.Path(sys.argv[1]); OUT.mkdir(parents=True,exist_ok=True)
def hashes():
    paths=list((ROOT/'crates').rglob('*.rs'))+[pathlib.Path(__file__),ROOT/'research/harness/src/bin/generation_churn_probe.rs',BIN]
    return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
class Child:
    def __init__(self,role,source,store):
        self.p=subprocess.Popen([str(BIN),role,str(source),str(store)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        self.ready=self.read()
    def read(self):
        line=self.p.stdout.readline()
        if not line: raise RuntimeError(f'child exited: {self.p.wait()}')
        return json.loads(line)
    def send(self,c):
        self.p.stdin.write(c+'\n'); self.p.stdin.flush()
    def ask(self,c): self.send(c); return self.read()
    def close(self,crash=False):
        self.send('crash' if crash else 'exit'); assert self.p.wait()==(87 if crash else 0)
def disk(store):
    seen=set(); logical=unique=allocated=0
    for p in store.rglob('*'):
        if not p.is_file(): continue
        s=p.stat(); logical+=s.st_size
        if (s.st_dev,s.st_ino) not in seen:
            seen.add((s.st_dev,s.st_ino)); unique+=s.st_size; allocated+=s.st_blocks*512
    return dict(logical_bytes=logical,unique_inode_bytes=unique,allocated_inode_bytes=allocated,generations=sorted(p.name for p in (store/'generations').iterdir()))
def run(files,funcs,arm,repeat):
    children=[]
    with tempfile.TemporaryDirectory(prefix='graph-churn-') as tmp:
        root=pathlib.Path(tmp); source=root/'source'; source.mkdir(); store=root/'store'
        def body(i): return ''.join(f'pub fn f{i}_{j}() -> usize {{ '+(f'f{i}_{j-1}()' if j else str(i))+' }\n' for j in range(funcs))
        for i in range(files): (source/f'f{i}.rs').write_text(body(i))
        try:
            writer=Child('writer',source,store); children.append(writer)
            current=writer.ready; previous=None; readers=[]; rows=[]; checks=0; rebuilds=0
            openings={'none':[], 'one':[0], 'distinct':[0,4,8], 'shared':[0,0,0]}[arm]
            def open_readers(step):
                for _ in range(openings.count(step)):
                    r=Child('reader',source,store); children.append(r)
                    assert r.ready['generation']==current['generation']
                    readers.append((r,current['generation'],current['fingerprint'],step))
            def record(step,result):
                d=disk(store); expected={current['generation']}|({previous} if previous else set())|{r[1] for r in readers}
                assert set(d['generations'])==expected,(d['generations'],expected)
                rows.append(dict(step=step,readers=len(readers),**d,**result))
            open_readers(0); record(0,dict(build_ns=current['build_ns']))
            for step in range(1,25):
                mutation=(step-1)%6
                if mutation==0: (source/'f0.rs').write_text(body(0)+f'// revision {step}\n')
                elif mutation==1: (source/'f1.rs').write_text(body(1)+f'pub fn extra_{step}() {{ f1_0(); }}\n')
                elif mutation==2: (source/'f1.rs').write_text(body(1))
                elif mutation==3:
                    a=source/'f2.rs'; b=source/'renamed_f2.rs'; (a if a.exists() else b).rename(b if a.exists() else a)
                elif mutation==4: (source/'extra.rs').write_text('pub fn added() {}\n')
                else: (source/'extra.rs').unlink()
                previous=current['generation']; current=writer.ask('sync'); assert current['generation']!=previous
                open_readers(step)
                if step%3==0:
                    for r,g,f,opened in readers:
                        if step-opened>=2:
                            assert r.ask('check')==dict(generation=g,fingerprint=f); checks+=1
                if step<=6 or step==24: assert writer.ask('rebuild-check')['rebuild_equal']; rebuilds+=1
                record(step,dict(sync_ns=current['sync_ns']))
            for idx in range(len(readers)):
                r,g,f,_=readers.pop(0); assert r.ask('check')==dict(generation=g,fingerprint=f); checks+=1
                r.close(crash=idx%2==0)
                (source/'f0.rs').write_text(body(0)+f'// released {idx}\n')
                previous=current['generation']; current=writer.ask('sync'); assert current['generation']!=previous
                record(25+idx,dict(sync_ns=current['sync_ns'],release='crash' if idx%2==0 else 'normal'))
            assert writer.ask('sync')['generation']==current['generation']
            writer.close()
            return dict(files=files,functions_per_file=funcs,arm=arm,repeat=repeat,reader_checks=checks,rebuild_checks=rebuilds,rows=rows)
        finally:
            for child in children:
                if child.p.poll() is None: child.p.kill(); child.p.wait()
before=hashes(); results=[]
(OUT/'environment.json').write_text(json.dumps(dict(platform=platform.platform(),python=sys.version,sources=before),indent=2)+'\n')
for files,funcs in [(64,8),(256,16)]:
    for repeat in range(3):
        arms=['none','one','distinct','shared']; arms=arms[repeat:]+arms[:repeat]
        for arm in arms:
            result=run(files,funcs,arm,repeat); results.append(result)
            (OUT/'runs.json').write_text(json.dumps(results,indent=2)+'\n')
            print(json.dumps({k:v for k,v in result.items() if k!='rows'}),flush=True)
assert before==hashes(),'source changed during measurement'
(OUT/'validation.json').write_text(json.dumps(dict(source_unchanged=True,runs=len(results),reader_checks=sum(r['reader_checks'] for r in results),rebuild_checks=sum(r['rebuild_checks'] for r in results)),indent=2)+'\n')
