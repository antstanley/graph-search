#!/usr/bin/env python3
"""Compile disposable privacy/path fixtures without executing application code."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CASES = [
    ("anchored", "fn send() {} mod child { fn local() {} fn call() { crate::send(); super::send(); self::local(); crate /* comment */ :: send(); } }", True),
    ("parent_visible", "mod api { pub(super) fn send() {} } mod sibling { fn call() { crate::api::send(); } }", True),
    ("crate_visible", "mod api { pub(crate) fn send() {} } mod sibling { fn call() { crate::api::send(); } }", True),
    ("private_sibling", "mod api { fn send() {} } mod sibling { fn call() { crate::api::send(); } }", False),
    ("super_at_root", "fn send() {} fn call() { super::send(); }", False),
]


def main():
    results = []
    with tempfile.TemporaryDirectory(prefix="graph-search-rust-path-") as directory:
        root = Path(directory)
        for name, source, succeeds in CASES:
            path = root / "lib.rs"
            path.write_text(source)
            command = ["rustc", "--edition=2021", "--crate-type=lib", "--emit=metadata", str(path), "-o", str(root / "output.rmeta")]
            run = subprocess.run(command, text=True, capture_output=True, timeout=30)
            results.append(dict(name=name, source=source, expected_success=succeeds, exit_code=run.returncode, stderr=run.stderr))
            assert (run.returncode == 0) == succeeds, results[-1]
    result = dict(rustc=subprocess.check_output(["rustc", "--version"], text=True).strip(), cases=results)
    Path(sys.argv[1]).write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
