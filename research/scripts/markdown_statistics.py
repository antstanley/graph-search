"""Characterize structural Markdown corpus statistics against native 80-line windows.

This is an exhaustive scorer equivalence/corpus diagnostic, not a final-context
quality or performance experiment. Uses the first source-valid task by ID in each
authorized sibling repo. Builds must finish before capture begins.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

from native_review import ROOT, eligible_tasks

sys.path.insert(0, str(ROOT / "evaluation"))
from taskbench.provenance import repository_snapshot


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def sources():
    names = subprocess.check_output(["git", "ls-files", "--cached", "--others",
        "--exclude-standard", "-z", "crates", "Cargo.toml", "Cargo.lock",
        "research/harness/src/bin/body_partition_probe.rs", "research/harness/Cargo.toml",
        "research/harness/Cargo.lock", "research/scripts/markdown_statistics.py"], cwd=ROOT)
    return {name.decode(): digest(ROOT / name.decode()) for name in names.split(b"\0") if name}


def indexes(root):
    return {str(path.relative_to(root)): digest(path)
            for path in sorted((root / ".codegraph").rglob("*")) if path.is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    roots = {name: ROOT.parent / name for name in ("nanus", "blogwright", "whatsurvey")}
    tasks, _, excluded = eligible_tasks(roots)
    selected = [next(task for task in sorted(tasks, key=lambda task: task["id"])
                     if task["repo"] == name) for name in roots]
    binary = ROOT / "research/harness/target/release/body_partition_probe"
    before = sources()
    binary_hash = digest(binary)
    snapshots = {name: repository_snapshot(root) for name, root in roots.items()}
    index_hashes = {name: indexes(root) for name, root in roots.items()}
    write(out / "tasks.json", selected)
    write(out / "label-validation.json", {"valid_ids": [task["id"] for task in tasks], "excluded": excluded})
    for task in selected:
        data = json.loads(subprocess.check_output([str(binary), str(roots[task["repo"]]), task["prompt"]], cwd=ROOT))
        write(out / (task["repo"] + ".json"), data)
    after = sources()
    snapshots_after = {name: repository_snapshot(root) for name, root in roots.items()}
    indexes_after = {name: indexes(root) for name, root in roots.items()}
    write(out / "provenance.json", {"production_before": before, "production_after": after,
        "binary_sha256": binary_hash, "binary_after_sha256": digest(binary),
        "siblings_before": snapshots, "siblings_after": snapshots_after,
        "indexes_before": index_hashes, "indexes_after": indexes_after})
    assert before == after
    assert binary_hash == digest(binary)
    assert snapshots == snapshots_after
    assert index_hashes == indexes_after


if __name__ == "__main__":
    main()
