#!/usr/bin/env python3
"""Independent native Node ESM checks for graph-search's module fixtures.

Only the small authored fixtures below execute, in a disposable directory.
No project code, dependency installation or third-party implementation is used.
"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

FILES = {
    "api.mjs": "function privateSend(){} export function send(){return 'send'} export default function primary(){return 'primary'}",
    "other.mjs": "export function send(){return 'other'}",
    "named.mjs": "export {send as deliver, default} from './api.mjs';",
    "local.mjs": "import {send as local} from './api.mjs'; export {local as deliver};",
    "cycle.mjs": "export * from './star.mjs';",
    "star.mjs": "export * from './api.mjs'; export * from './cycle.mjs';",
    "diamond.mjs": "export * from './api.mjs'; export * from './star.mjs';",
    "ambiguous.mjs": "export * from './api.mjs'; export * from './other.mjs';",
    "override.mjs": "export * from './ambiguous.mjs'; export {send} from './api.mjs';",
}
CASES = [
    ("named", "import {deliver} from './named.mjs'; assert.equal(deliver(),'send');", True),
    ("default", "import main from './named.mjs'; assert.equal(main(),'primary');", True),
    ("namespace", "import * as api from './api.mjs'; assert.equal(api.send(),'send'); assert.equal(api.privateSend,undefined);", True),
    ("private", "import {privateSend} from './api.mjs'; privateSend();", False),
    ("local_forward", "import {deliver} from './local.mjs'; assert.equal(deliver(),'send');", True),
    ("cycle", "import {send} from './star.mjs'; assert.equal(send(),'send');", True),
    ("diamond", "import {send} from './diamond.mjs'; assert.equal(send(),'send');", True),
    ("ambiguous", "import {send} from './ambiguous.mjs'; send();", False),
    ("explicit_over_star", "import {send} from './override.mjs'; assert.equal(send(),'send');", True),
    ("star_excludes_default", "import main from './star.mjs'; main();", False),
]


def main():
    results = []
    with tempfile.TemporaryDirectory(prefix="graph-search-js-module-") as directory:
        root = Path(directory)
        for name, source in FILES.items():
            (root / name).write_text(source)
        for name, source, expected in CASES:
            path = root / "case.mjs"
            path.write_text("import assert from 'node:assert/strict';\n" + source)
            run = subprocess.run(["node", str(path)], capture_output=True, text=True, timeout=30)
            result = dict(name=name, source=source, expected_success=expected, exit_code=run.returncode, stdout=run.stdout, stderr=run.stderr)
            results.append(result)
            assert (run.returncode == 0) == expected, result
    Path(sys.argv[1]).write_text(json.dumps(dict(node=subprocess.check_output(["node", "--version"], text=True).strip(), files=FILES, cases=results), indent=2) + "\n")


if __name__ == "__main__":
    main()
