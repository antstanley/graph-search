#!/usr/bin/env python3
"""Sample only owned probe-process RSS during generation retention.

This is a lifecycle experiment, not a heap census, peak-RSS measurement or
concurrent query benchmark. ps is invoked with a specific owned process ID.
"""
import hashlib
import json
import pathlib
import platform
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = ROOT / 'research/harness/target/release/generation_churn_probe'


def hashes():
    paths = list((ROOT / 'crates').rglob('*.rs')) + [
        pathlib.Path(__file__), ROOT / 'research/harness/src/bin/generation_churn_probe.rs', BIN]
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}


class Child:
    def __init__(self, role, source, store):
        self.p = subprocess.Popen([str(BIN), role, str(source), str(store)],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.ready = self.read()

    def read(self):
        line = self.p.stdout.readline()
        if not line:
            raise RuntimeError(f'probe exited: {self.p.wait()}')
        return json.loads(line)

    def send(self, command):
        self.p.stdin.write(command + '\n')
        self.p.stdin.flush()

    def ask(self, command):
        self.send(command)
        return self.read()

    def rss(self):
        assert self.p.poll() is None
        # Both macOS and Linux ps report RSS in KiB. Reject unsupported hosts.
        assert sys.platform in ('darwin', 'linux')
        return int(subprocess.check_output(
            ['/bin/ps', '-o', 'rss=', '-p', str(self.p.pid)], text=True).strip()) * 1024

    def close(self):
        self.send('exit')
        assert self.p.wait() == 0


def run(files, funcs, arm, repeat):
    children = []
    with tempfile.TemporaryDirectory(prefix='graph-generation-memory-') as tmp:
        root = pathlib.Path(tmp)
        source = root / 'source'
        source.mkdir()
        store = root / 'store'
        def body(i):
            return ''.join(f'pub fn f{i}_{j}() -> usize {{ ' +
                           (f'f{i}_{j-1}()' if j else str(i)) + ' }\n'
                           for j in range(funcs))
        for i in range(files):
            (source / f'f{i}.rs').write_text(body(i))
        try:
            writer = Child('writer', source, store)
            children.append(writer)
            current = writer.ready
            readers = []
            samples = []
            checks = 0
            openings = {'none': [], 'one': [0], 'shared': [0, 0, 0], 'distinct': [0, 2, 4]}[arm]
            def sample(stage, step):
                samples.append(dict(stage=stage, step=step, writer_rss_bytes=writer.rss(),
                                    readers=[dict(generation=g, rss_bytes=r.rss()) for r, g, _ in readers]))
            def open_readers(step):
                for _ in range(openings.count(step)):
                    r = Child('reader', source, store)
                    children.append(r)
                    assert r.ready['generation'] == current['generation']
                    readers.append((r, current['generation'], current['fingerprint']))
                if step in openings:
                    sample('after_open_before_manifest', step)
            sample('writer_ready', 0)
            open_readers(0)
            for step in range(1, 9):
                (source / 'f0.rs').write_text(body(0) + f'// revision {step}\n')
                previous = current['generation']
                current = writer.ask('sync')
                assert current['generation'] != previous
                open_readers(step)
            sample('after_churn_before_manifest', 8)
            for r, g, fingerprint in readers:
                assert r.ask('check') == dict(generation=g, fingerprint=fingerprint)
                checks += 1
            sample('after_manifest_checks', 8)
            for step in range(9, 9 + len(readers)):
                r, _, _ = readers.pop(0)
                r.close()
                (source / 'f0.rs').write_text(body(0) + f'// release {step}\n')
                current = writer.ask('sync')
                sample('after_release_and_publish', step)
            final_generations = len(list((store / 'generations').iterdir()))
            assert final_generations == 2
            sample('all_readers_released', 12)
            writer.close()
            return dict(files=files, functions_per_file=funcs, arm=arm, repeat=repeat,
                        reader_checks=checks, final_generations=final_generations, samples=samples)
        finally:
            for child in children:
                if child.p.poll() is None:
                    child.p.kill()
                    child.p.wait()


def main():
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    before = hashes()
    (out / 'environment.json').write_text(json.dumps(dict(
        platform=platform.platform(), python=sys.version, sources=before), indent=2) + '\n')
    runs = []
    for files, funcs in [(64, 8), (256, 16)]:
        for repeat in range(3):
            arms = ['none', 'one', 'distinct', 'shared']
            for arm in arms[repeat:] + arms[:repeat]:
                result = run(files, funcs, arm, repeat)
                runs.append(result)
                (out / 'runs.json').write_text(json.dumps(runs, indent=2) + '\n')
                print(json.dumps({k: v for k, v in result.items() if k != 'samples'}), flush=True)
    assert before == hashes(), 'source changed during measurement'
    (out / 'validation.json').write_text(json.dumps(dict(
        source_unchanged=True, runs=len(runs), reader_checks=sum(r['reader_checks'] for r in runs)), indent=2) + '\n')


if __name__ == '__main__':
    main()
