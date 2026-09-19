"""Reproduce natural-language experiments; external sources stay read-only."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
BASE = Path(__file__).resolve().parent
BASE_COMMIT = "bdcbecef41d9863bca23830ed30615f1c219064d"


def run(args, **kwargs):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), cwd=ROOT, check=True, **kwargs)


def prepare(target):
    (BASE / "private").mkdir(exist_ok=True)
    (BASE / "results").mkdir(exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(target))
    with tempfile.TemporaryDirectory(prefix="nl-baseline-") as directory:
        archive = Path(directory) / "baseline.tar"
        with archive.open("wb") as stream:
            run(["git", "archive", BASE_COMMIT], stdout=stream)
        source = Path(directory) / "source"
        source.mkdir()
        with tarfile.open(archive) as tar:
            tar.extractall(source, filter="data")
        baseline_env = dict(os.environ, CARGO_TARGET_DIR=str(target / "baseline"))
        for manifest, binary in [("research/harness", "search-research"),
                                 ("evaluation/harness", "task-eval-host")]:
            run(["cargo", "build", "--locked", "--offline", "--manifest-path",
                 source / manifest / "Cargo.toml", "--bin", binary], env=baseline_env)
            shutil.copy2(target / "baseline/debug" / binary,
                         BASE / "private" / ("baseline-" + binary))
    empty = BASE / "private/empty-requests.json"
    empty.write_text("[]\n")
    for repo in ["nanus", "blogwright", "whatsurvey"]:
        with (BASE / "private" / f"{repo}-graph.json").open("w") as stream:
            run([BASE / "private/baseline-search-research", Path.home() / "code" / repo,
                 empty], stdout=stream)
    for manifest, binary in [("research/harness", "search-research"),
                             ("evaluation/harness", "task-eval-host")]:
        run(["cargo", "build", "--locked", "--offline", "--manifest-path",
             ROOT / manifest / "Cargo.toml", "--bin", binary], env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--development", action="store_true")
    parser.add_argument("--confirm", action="store_true", help="Explicitly open frozen confirmation tasks")
    parser.add_argument("--public", action="store_true", help="All frozen core queries and 60 public Index tasks; requires --confirm")
    parser.add_argument("--target", type=Path, default=Path("/private/tmp/graph-search-natural-language-target"))
    args = parser.parse_args()
    if args.public and not args.confirm:
        parser.error("--public requires --confirm because it includes frozen confirmation labels")
    if args.prepare:
        prepare(args.target)
    if args.development:
        for stage in ["fields", "expansion-diversity", "combined"]:
            run([sys.executable, BASE / "experiment.py", stage])
        run([sys.executable, BASE / "evidence_probe.py"])
    if args.confirm:
        run([sys.executable, BASE / "experiment.py", "confirm"])
    if args.public:
        run([sys.executable, BASE / "public_probe.py", "--current", args.target / "debug/search-research"])
        roots = BASE / "private/roots.json"
        roots.write_text(json.dumps({repo: str(Path.home() / "code" / repo)
                                    for repo in ["nanus", "blogwright", "whatsurvey"]}))
        for arm, host in [("baseline", BASE / "private/baseline-task-eval-host"),
                          ("current", args.target / "debug/task-eval-host")]:
            destination = ROOT / "evaluation/runs" / ("natural-language-" + arm + "-reproduction")
            run([sys.executable, "-m", "taskbench", "run", "--roots", roots,
                 "--arms", "graph-search", "--split", "all", "--allow-heldout",
                 "--host", host, "--output", destination],
                env=dict(os.environ, PYTHONPATH=str(ROOT / "evaluation")))
            for name in ["manifest", "results", "report"]:
                shutil.copy2(destination / (name + ".json"),
                             BASE / "results" / f"public-index-{arm}-{name}.json")

        run([sys.executable, BASE / "profile.py", "--current", args.target / "debug/task-eval-host"])
        run([sys.executable, BASE / "summarize.py", "--raw-run-suffix", "reproduction"])


if __name__ == "__main__":
    main()
