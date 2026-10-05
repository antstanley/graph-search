"""A small client for the JSONL accuracy host."""
import json
import os
import subprocess


class HostStartError(RuntimeError):
    """The host exited before it was ready; carries its stderr."""


class Host:
    def __init__(self, binary, root, store, excludes=()):
        env = dict(os.environ, ACCURACY_EXCLUDES="\n".join(excludes))
        self.proc = subprocess.Popen(
            [str(binary), str(root), str(store)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1, env=env,
        )
        line = self.proc.stdout.readline()
        if not line:
            raise HostStartError(self.proc.stderr.read().strip())
        self.ready = json.loads(line)

    def ask(self, **request):
        self.proc.stdin.write(json.dumps(request) + "\n")
        self.proc.stdin.flush()
        line = self.proc.stdout.readline()
        if not line:
            raise RuntimeError("host exited")
        return json.loads(line)

    def close(self):
        if self.proc.poll() is None:
            self.proc.stdin.close()
            self.proc.wait(timeout=60)
