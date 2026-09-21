#!/usr/bin/env python3
"""Node checks for the supported package.json self-reference/import-map rules."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

BASE = {"name": "@demo/api", "type": "module", "main": "./wrong.js", "exports": {".": "./api.js", "./sub": "./sub.js", "./hidden": None}, "imports": {"#internal": "./api.js"}}
CASES = [
    ("self_named", {}, "import {send} from '@demo/api'; assert.equal(send(),'send');", True, False),
    ("self_default", {}, "import main from '@demo/api'; assert.equal(main(),'primary');", True, False),
    ("self_subpath", {}, "import {sub} from '@demo/api/sub'; assert.equal(sub(),'sub');", True, False),
    ("private_map", {}, "import {send} from '#internal'; assert.equal(send(),'send');", True, False),
    ("blocked_export", {}, "import {send} from '@demo/api/hidden'; send();", False, False),
    ("unlisted_export", {}, "import {send} from '@demo/api/api.js'; send();", False, False),
    ("no_self_without_exports", {"exports": "__absent__"}, "import {send} from '@demo/api'; send();", False, False),
    ("null_does_not_use_main", {"exports": None}, "import {send} from '@demo/api'; send();", False, False),
    ("outside_target", {"exports": "../api.js"}, "import {send} from '@demo/api'; send();", False, False),
    ("node_modules_target", {"exports": "./NODE_MODULES/api.js"}, "import {send} from '@demo/api'; send();", False, False),
    ("nested_scope_blocks_private_inheritance", {}, "import {send} from '#internal'; send();", False, True),
    ("control_target_normalizes_but_native_rejects", {"exports": "./a\npi.js"}, "import {send} from '@demo/api'; assert.equal(send(),'send');", True, False),
    ("conditional_is_valid_but_native_unmodeled", {"exports": {"import": "./api.js"}}, "import {send} from '@demo/api'; assert.equal(send(),'send');", True, False),
]


ERROR_CODES = {
    "blocked_export": "ERR_PACKAGE_PATH_NOT_EXPORTED",
    "unlisted_export": "ERR_PACKAGE_PATH_NOT_EXPORTED",
    "no_self_without_exports": "ERR_MODULE_NOT_FOUND",
    "null_does_not_use_main": "ERR_MODULE_NOT_FOUND",
    "outside_target": "ERR_INVALID_PACKAGE_TARGET",
    "node_modules_target": "ERR_INVALID_PACKAGE_TARGET",
    "nested_scope_blocks_private_inheritance": "ERR_PACKAGE_IMPORT_NOT_DEFINED",
}


def main():
    rows = []
    for name, overrides, source, expected, nested in CASES:
        with tempfile.TemporaryDirectory(prefix="graph-search-node-package-") as directory:
            root = Path(directory)
            manifest = BASE | overrides
            if manifest.get("exports") == "__absent__":
                del manifest["exports"]
            (root / "package.json").write_text(json.dumps(manifest))
            files = {"a\npi.js": "export function send(){return 'literal-decoy'}", "api.js": "export function send(){return 'send'} export default function primary(){return 'primary'}", "sub.js": "export function sub(){return 'sub'}", "wrong.js": "export function send(){return 'wrong'}"}
            for path, text in files.items():
                (root / path).write_text(text)
            parent = root
            if nested:
                parent = root / "nested"
                parent.mkdir()
                (parent / "package.json").write_text("{}")
            entry = parent / "case.mjs"
            entry.write_text("import assert from 'node:assert/strict';\n" + source)
            run = subprocess.run(["node", str(entry)], capture_output=True, text=True, timeout=30)
            row = dict(name=name, manifest=manifest, files=files, source=source, nested=nested, expected_success=expected, exit_code=run.returncode, stdout=run.stdout, stderr=run.stderr)
            row["expected_error_code"] = ERROR_CODES.get(name)
            rows.append(row)
            assert (run.returncode == 0) == expected, row
            if name in ERROR_CODES:
                assert ERROR_CODES[name] in run.stderr, row
    Path(sys.argv[1]).write_text(json.dumps(dict(node=subprocess.check_output(["node", "--version"], text=True).strip(), cases=rows), indent=2) + "\n")


if __name__ == "__main__":
    main()
