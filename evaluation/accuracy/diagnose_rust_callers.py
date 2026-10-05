#!/usr/bin/env python3
"""Classifies missed Rust caller sites from a results directory.

Each missed site is re-located in the oracle's call list (to recover whether it
sits inside macro arguments) and placed inside or outside a `#[cfg(test)]`
module. The test-module test is textual: a line after `#[cfg(test)]` followed
by `mod <name> {` up to that module's closing brace (brace counting ignores
strings and comments, so it is approximate).
"""
import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
RUST_ORACLE = "/tmp/rust-oracle-target/release/rust-oracle"
SITE = re.compile(r"^(?P<path>.+?):(?P<line>\d+) recv=(?P<recv>\w+)")


def test_regions(text):
    lines = text.split("\n")
    regions = []
    for index, line in enumerate(lines):
        if line.strip().startswith("#[cfg(test)]"):
            for next_index in range(index + 1, min(index + 4, len(lines))):
                if re.match(r"\s*(pub(\(\w+\))?\s+)?mod\s+\w+\s*\{", lines[next_index]):
                    depth, end = 0, len(lines)
                    for scan in range(next_index, len(lines)):
                        depth += lines[scan].count("{") - lines[scan].count("}")
                        if depth <= 0 and scan > next_index:
                            end = scan + 1
                            break
                    regions.append((next_index + 1, end))
                    break
    return regions


def main():
    results = Path(sys.argv[1])
    repos = json.loads((results / "repos.json").read_text())
    tally = Counter()
    for failures in sorted(results.glob("*.failures.json")):
        name = failures.name.removesuffix(".failures.json")
        records = json.loads(failures.read_text()).get("rust", [])
        missed = []
        for record in records:
            if record["task"] == "callers":
                for site in record.get("missed", []):
                    match = SITE.match(site)
                    if match:
                        missed.append((match["path"], int(match["line"]), match["recv"]))
        if not missed:
            continue
        root = Path(repos[name])
        files = sorted({path for path, _, _ in missed})
        oracle = json.loads(subprocess.run([RUST_ORACLE], input=json.dumps({"root": str(root), "files": files}),
                                           capture_output=True, text=True, check=True).stdout)
        macro_lines = {(c["path"], c["line"]) for c in oracle["calls"] if c.get("in_macro")}
        regions = {path: test_regions((root / path).read_text()) for path in files}
        for path, line, recv in missed:
            in_test = "/tests/" in f"/{path}" or any(start <= line <= end for start, end in regions[path])
            where = "macro_arg" if (path, line) in macro_lines else recv
            tally[(where, "test" if in_test else "non-test")] += 1
    total = sum(tally.values())
    print(f"missed Rust caller sites: {total}")
    for (where, scope), count in tally.most_common():
        print(f"  {where:10s} {scope:9s} {count:5d}  {100 * count / total:5.1f}%")


if __name__ == "__main__":
    main()
