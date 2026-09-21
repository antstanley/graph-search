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
        try:
            process.wait(timeout=max(.01, deadline-time.monotonic()))
        except subprocess.TimeoutExpired as error:
            raise TimeoutError('command timed out after output closed') from error
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
    def __init__(self, root, host, retrieval=None, **_):
        self.retrieval=retrieval
        self.root=root
        self.host=host
        self.tmp=tempfile.TemporaryDirectory(prefix='task-eval-store-')
        self.stderr=tempfile.TemporaryFile()
        self.process=None
        self.closed=False
        self.restart_events=[]
        try:
            self.setup_ms=self._start(180)
        except BaseException:
            self.close()
            raise

    def _stop(self):
        process,self.process=self.process,None
        if process is not None:
            try:
                terminate(process)
            finally:
                for stream in (process.stdin,process.stdout):
                    try:
                        stream.close()
                    except OSError:
                        # Closing buffered stdin may flush into an already dead host.
                        pass

    def _start(self, timeout):
        self._stop()
        started=time.monotonic()
        self.pending=b''
        try:
            self.process=subprocess.Popen([str(self.host), str(self.root), str(Path(self.tmp.name)/'index')],
                stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.stderr,start_new_session=True)
            ready=self.receive(timeout)
            if not ready.get('ready'):
                raise RuntimeError('graph host failed to initialize')
            return (time.monotonic()-started)*1000
        except BaseException:
            self._stop()
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
        if self.closed:
            raise RuntimeError('graph backend is closed')
        started=time.monotonic()
        if self.process is None or self.process.poll() is not None:
            event={'elapsed_ms':None,'succeeded':False}
            tick=time.monotonic()
            try:
                self._start(timeout)
                event['succeeded']=True
            finally:
                event['elapsed_ms']=(time.monotonic()-tick)*1000
                self.restart_events.append(event)
        try:
            remaining=timeout-(time.monotonic()-started)
            if remaining<=0:
                raise TimeoutError('graph recovery exhausted query timeout')
            request=dict(query=query,mode=mode)
            if self.retrieval is not None:
                request['retrieval']=self.retrieval
            self.process.stdin.write((json.dumps(request)+'\n').encode())
            self.process.stdin.flush()
            value=self.receive(remaining)
        except (OSError,RuntimeError,TimeoutError,ValueError):
            # Do not retry the failed query or reuse a desynchronized JSONL stream.
            self._stop()
            raise
        if 'error' in value:
            raise RuntimeError(value['error'])
        if mode!='explore':
            return json.dumps(value,ensure_ascii=False)
        return render_graph_explore(value)

    def close(self):
        if self.closed:
            return
        self.closed=True
        try:
            self._stop()
        finally:
            try:
                self.stderr.close()
            finally:
                self.tmp.cleanup()


def render_graph_explore(value):
    """Deliver the complete compact API JSON without dropping or expanding fields."""
    return json.dumps(value,ensure_ascii=False,separators=(',',':'))


BACKENDS={'text':TextSearch,'codegraph':CodeGraph,'graph-search':GraphSearch}
