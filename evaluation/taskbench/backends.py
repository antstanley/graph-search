"""Read-only search adapters. Stores created by graph-search live in temporary dirs."""
from __future__ import annotations
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import tempfile
import time


class Unavailable(RuntimeError):
    pass


def terminate(process):
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def command(argv, *, cwd, timeout=30, stdin=None, max_bytes=4_000_000):
    """Bound process time and disk output; never interpolate a shell command."""
    process = subprocess.Popen(argv, cwd=cwd, stdin=subprocess.PIPE if stdin is not None else subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        pending = memoryview(stdin.encode()) if stdin is not None else None
        if pending is not None:
            os.set_blocking(process.stdin.fileno(), False)
            selector.register(process.stdin, selectors.EVENT_WRITE)
        chunks, size, deadline = [], 0, time.monotonic()+timeout
        while selector.get_map():
            remaining = deadline-time.monotonic()
            if remaining <= 0:
                raise TimeoutError('command timed out')
            for key, event in selector.select(min(remaining, .1)):
                if event & selectors.EVENT_WRITE:
                    try:
                        written = os.write(key.fileobj.fileno(), pending[:65536]) if pending else 0
                        pending = pending[written:]
                    except BrokenPipeError:
                        pending = pending[:0]
                    if not pending:
                        selector.unregister(key.fileobj)
                        key.fileobj.close()
                    continue
                chunk = os.read(key.fileobj.fileno(), 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                size += len(chunk)
                if size > max_bytes:
                    raise RuntimeError('command output exceeds capture limit')
                chunks.append(chunk)
        process.wait(timeout=max(.01, deadline-time.monotonic()))
        return process.returncode, b''.join(chunks).decode('utf-8', errors='replace')
    finally:
        terminate(process)
        if process.stdout:
            process.stdout.close()
        if process.stdin and not process.stdin.closed:
            process.stdin.close()
        if 'selector' in locals():
            selector.close()


class TextSearch:
    name = 'text'
    setup_ms = 0
    def __init__(self, root: Path, **_):
        self.root = root

    def search(self, query: str, mode='explore', timeout=30) -> str:
        if mode != 'explore':
            raise ValueError('text arm supports search and read only')
        # Fixed lexical baseline: ordinary case-insensitive OR search, path order.
        stop = set('a an the of to in on and or for with is are be as its it this that why how what which explain plan source cite relevant execution path observed behavior assumptions distinguish propose regression tests do not edit repository'.split())
        terms = list(dict.fromkeys(t.lower() for t in re.findall(r'[\w]+', query) if len(t)>2 and t.lower() not in stop))[:32]
        if not terms:
            return ''
        argv = ['rg', '--json', '--sort', 'path', '-i', '-m', '5', '-e', '|'.join(re.escape(t) for t in terms), '.']
        code, output = command(argv, cwd=self.root, timeout=timeout)
        if code not in (0,1):
            raise RuntimeError(output[:1000])
        result=[]
        for line in output.splitlines():
            item=json.loads(line)
            if item['type'] != 'match':
                continue
            data=item['data']
            path=data['path'].get('text', '').removeprefix('./')
            if path and 'text' in data['lines']:
                for offset, content in enumerate(data['lines']['text'].splitlines()):
                    result.append(f"{path}:{data['line_number']+offset}\t{content}")
        return '\n'.join(result)

    def close(self):
        pass


class CodeGraph(TextSearch):
    name = 'codegraph'
    def __init__(self, root, **kwargs):
        super().__init__(root, **kwargs)
        if not (root/'.codegraph').is_dir():
            raise Unavailable('no existing CodeGraph index; suite never indexes external repos')

    def search(self, query, mode='explore', timeout=30):
        if mode != 'explore':
            raise ValueError('CodeGraph arm supports search and read only')
        code, raw = command(['codegraph', 'explore', query, '-p', str(self.root), '--max-files', '8'], cwd=self.root, timeout=timeout)
        if code:
            raise RuntimeError(raw[:1000])
        result=[]; path=None; fenced=False
        for line in raw.splitlines():
            heading=re.match(r'^\*\*`([^`]+)`\*\*',line)
            if heading:
                path=heading.group(1)
            if line.startswith('```'):
                fenced=not fenced
                continue
            numbered=re.match(r'^\s*(\d+)\t(.*)$',line)
            if path and fenced and numbered:
                result.append(f'{path}:{numbered[1]}\t{numbered[2]}')
            else:
                result.append(line)
        return '\n'.join(result)


class GraphSearch:
    name = 'graph-search'
    def __init__(self, root, host, **_):
        self.root=root
        self.tmp=tempfile.TemporaryDirectory(prefix='task-eval-store-')
        self.stderr=tempfile.TemporaryFile()
        self.process=subprocess.Popen([str(host), str(root), str(Path(self.tmp.name)/'index')],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.stderr,start_new_session=True)
        self.pending=b''
        try:
            ready=self.receive(180)
            if not ready.get('ready'):
                raise RuntimeError('graph host failed to initialize')
            self.setup_ms=ready['setup_ms']
        except BaseException:
            self.close()
            raise

    def receive(self, timeout):
        selector=selectors.DefaultSelector()
        selector.register(self.process.stdout,selectors.EVENT_READ)
        deadline=time.monotonic()+timeout
        try:
            while b'\n' not in self.pending:
                remaining=deadline-time.monotonic()
                if remaining<=0:
                    terminate(self.process)
                    raise TimeoutError('graph host timed out')
                if not selector.select(remaining):
                    continue
                chunk=os.read(self.process.stdout.fileno(),65536)
                if not chunk:
                    raise RuntimeError('graph host exited before response')
                self.pending+=chunk
                if len(self.pending)>8_000_000:
                    terminate(self.process)
                    raise RuntimeError('graph response exceeds capture limit')
            line,self.pending=self.pending.split(b'\n',1)
            return json.loads(line)
        finally:
            selector.close()

    def search(self,query,mode='explore',timeout=30):
        self.process.stdin.write((json.dumps(dict(query=query,mode=mode))+'\n').encode())
        self.process.stdin.flush()
        value=self.receive(timeout)
        if 'error' in value:
            raise RuntimeError(value['error'])
        if mode!='explore':
            return json.dumps(value,ensure_ascii=False)
        lines=[]
        for item in value.get('items',[]):
            node=item['node']; snippet=item.get('snippet')
            lines.append(f"candidate {node['path']}:{node.get('start_line',1)} {node.get('name','')}")
            if snippet:
                for offset,line in enumerate(snippet['lines']):
                    lines.append(f"{node['path']}:{snippet['start_line']+offset}\t{line}")
        lines.append(json.dumps({k:v for k,v in value.items() if k!='items'},ensure_ascii=False))
        return '\n'.join(lines)

    def close(self):
        if hasattr(self,'process'):
            terminate(self.process)
            self.process.stdin.close(); self.process.stdout.close()
        self.stderr.close()
        self.tmp.cleanup()


BACKENDS={'text':TextSearch,'codegraph':CodeGraph,'graph-search':GraphSearch}
