#!/usr/bin/env python3
"""Independent compiler checks for native lexical import binding fixtures."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

PRELUDE = "mod api { pub fn send() {} pub fn other() {} } "
CASES = [
    ("hoisting", "fn call() { relay(); use crate::api::send as relay; }", True),
    ("namespace_and_outer_value", "fn send() {} fn call() { use crate::api::{self as send}; send(); send::send(); }", True),
    ("namespace_and_local_value", "fn call() { use crate::api::{self as service}; let service = 1; service::send(); }", True),
    ("value_shadow", "fn call() { use crate::api::send as relay; relay(); let relay = || {}; relay(); }", True),
    ("duplicate_import", "fn call() { use crate::api::send as relay; use crate::api::other as relay; relay(); }", False),
    ("module_boundary", "use crate::api::{self as service, send as relay}; mod child { fn call() { relay(); service::send(); } }", False),
    ("type_only_call", "use crate::api::{self as phantom}; fn call() { phantom(); }", False),
]


def main():
    cases = []
    with tempfile.TemporaryDirectory(prefix="graph-search-rust-import-") as directory:
        root = Path(directory)
        for name, body, expected in CASES:
            source = PRELUDE + body
            path = root / "lib.rs"
            path.write_text(source)
            run = subprocess.run(["rustc", "--edition=2021", "--crate-type=lib", "--emit=metadata", str(path), "-o", str(root / "out.rmeta")], capture_output=True, text=True, timeout=30)
            cases.append(dict(name=name, source=source, expected_success=expected, exit_code=run.returncode, stderr=run.stderr))
            assert (run.returncode == 0) == expected, cases[-1]
    Path(sys.argv[1]).write_text(json.dumps(dict(rustc=subprocess.check_output(["rustc", "--version"], text=True).strip(), cases=cases), indent=2) + "\n")


if __name__ == "__main__":
    main()
