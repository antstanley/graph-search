"""Reproduce the native search review without changing production or external indexes.

Usage: python3 research/scripts/native_review.py --output /tmp/new-review --external
Existing dependencies must be cached. Raw external transcripts stay in the
printed temporary run directory; published results contain no external source.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "evaluation"))
from taskbench.core import validate  # noqa: E402
from taskbench.provenance import repository_snapshot  # noqa: E402


def write(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def run(argv, **kwargs):
    return subprocess.run(argv, cwd=ROOT, check=True, **kwargs)


def capture(argv):
    return json.loads(run(argv, stdout=subprocess.PIPE).stdout)


def eligible_tasks(roots):
    tasks = json.loads((ROOT / "evaluation/tasks.json").read_text())
    oracles = json.loads((ROOT / "evaluation/oracles.json").read_text())
    eligible, excluded = [], []
    for task in tasks:
        try:
            validate([task], {task["id"]: oracles[task["id"]]}, roots)
        except (ValueError, OSError) as error:
            excluded.append({"id": task["id"], "repo": task["repo"], "reason": str(error)})
        else:
            eligible.append(task)
    return eligible, {t["id"]: oracles[t["id"]] for t in eligible}, excluded


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--external", action="store_true")
    parser.add_argument("--roots", type=Path, help="Optional JSON repo-name to absolute path map")
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    target = ROOT / "research/harness/target"
    run(["cargo", "build", "--release", "--offline", "--locked", "--manifest-path",
         "research/harness/Cargo.toml", "--bin", "native_probe"])
    binary = target / "release/native_probe"
    write(out / "native-probe.json", capture([str(binary)]))
    commit = run(["git", "rev-parse", "HEAD"], stdout=subprocess.PIPE).stdout.decode().strip()
    with tempfile.TemporaryDirectory(prefix="graph-search-frozen-") as temporary:
        archive = run(["git", "archive", commit], stdout=subprocess.PIPE).stdout
        run(["tar", "-x", "-C", temporary], input=archive)
        result = capture([str(binary), temporary])
        result["root"] = "git-archive:" + commit
        result["corpus_commit"] = commit
        write(out / "repository-probe.json", result)
    tracked = run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "crates"], stdout=subprocess.PIPE).stdout
    paths = [ROOT / p.decode() for p in tracked.split(b"\0") if p]
    paths += [ROOT / "Cargo.toml", ROOT / "Cargo.lock",
              ROOT / "research/harness/src/bin/native_probe.rs", Path(__file__).resolve()]
    write(out / "provenance.json", {"git_head": commit, "platform": platform.platform(),
        "rustc": run(["rustc", "--version"], stdout=subprocess.PIPE).stdout.decode().strip(),
        "source_sha256": {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths},
        "note": "One warmup, median of five for microbenchmarks; external trials three repeats. No application code or external index is modified."})
    if not args.external:
        return
    roots = ({k: Path(v).resolve() for k, v in json.loads(args.roots.read_text()).items()}
             if args.roots else {r: ROOT.parent / r for r in ("nanus", "blogwright", "whatsurvey")})
    tasks, oracles, excluded = eligible_tasks(roots)
    write(out / "label-validation.json", {"valid_ids": [t["id"] for t in tasks], "excluded": excluded})
    if not tasks:
        raise ValueError("No unchanged source labels; see label-validation.json")
    # Keep external source transcripts out of the result artifact directory.
    work = Path(tempfile.mkdtemp(prefix="graph-search-review-external-"))
    print("Raw run directory:", work, flush=True)
    write(work / "roots.json", {k: str(v) for k, v in roots.items()})
    write(work / "tasks.json", tasks)
    write(work / "oracles.json", oracles)
    before = {k: repository_snapshot(v) for k, v in roots.items()}
    for repo, root in roots.items():
        query_path = work / f"{repo}-tasks.json"
        write(query_path, [t for t in tasks if t["repo"] == repo])
        write(out / f"{repo}-candidates.json", capture([str(binary), "--candidates", str(root), str(query_path)]))
    after = {k: repository_snapshot(v) for k, v in roots.items()}
    write(out / "candidate-source-stability.json", {"before": before, "after": after, "equal": before == after})
    if before != after:
        raise ValueError("External corpus changed during candidate experiment")
    run(["cargo", "build", "--release", "--offline", "--locked", "--manifest-path",
         "evaluation/harness/Cargo.toml", "--target-dir", str(target)])
    env = dict(os.environ, PYTHONPATH=str(ROOT / "evaluation"))
    run([sys.executable, "-m", "taskbench", "run", "--roots", str(work / "roots.json"),
         "--tasks", str(work / "tasks.json"), "--oracles", str(work / "oracles.json"),
         "--split", "all", "--allow-heldout", "--arms", "graph-search", "codegraph", "text",
         "--repeats", "3", "--host", str(target / "release/task-eval-host"),
         "--output", str(work / "run")], env=env)
    for name in ("results.json", "report.json", "manifest.json"):
        write(out / ("external-" + name), json.loads((work / "run" / name).read_text()))


if __name__ == "__main__":
    main()
