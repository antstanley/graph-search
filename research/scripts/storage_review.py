"""Measure generation churn on a frozen repository copy, without editing its source.

Usage: python3 research/scripts/storage_review.py --output /tmp/storage-review
Artifact/record sizes are logical bytes, not measured physical I/O.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def run(args, **kwargs):
    return subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def create_corpus(directory, synthetic_files, commit, synthetic_kind="markdown"):
    """Return a reproducible corpus label and the relative file to modify."""
    if synthetic_files:
        for document in range(synthetic_files):
            if synthetic_kind == "rust":
                text = "".join(
                    f"pub fn route_{document:04}_{function:04}(input: usize) -> usize {{ "
                    + (f"route_{document:04}_{function-1:04}(input) + {function}"
                       if function else "input + 1") + " }\n"
                    for function in range(64))
                (Path(directory) / f"document-{document:04}.rs").write_text(text)
            else:
                text = f"# Document {document}\n\n" + "".join(
                    f"route_{document:04}_{line:04} query_{line:04} handles native search parameters, "
                    f"source evidence, graph results and indexing state.\n" for line in range(512))
                (Path(directory) / f"document-{document:04}.md").write_text(text)
        shape = "functions=64" if synthetic_kind == "rust" else "body_lines=512"
        corpus = f"synthetic-{synthetic_kind}:files={synthetic_files},{shape}"
        edit = "document-0000.rs" if synthetic_kind == "rust" else "document-0000.md"
    else:
        archive = run(["git", "archive", commit], stdout=subprocess.PIPE).stdout
        run(["tar", "-x", "-C", directory], input=archive)
        corpus = "git-archive:" + commit
        edit = "crates/core/src/query.rs"
    return corpus, edit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--synthetic-files", type=int, default=0,
                        help="Use deterministic 512-line Markdown files instead of the HEAD archive")
    parser.add_argument("--synthetic-kind", choices=("markdown", "rust"), default="markdown")
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("--repeats must be positive")
    if not 0 <= args.synthetic_files <= 4096:
        parser.error("--synthetic-files must be between 0 and 4096")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    run(["cargo", "build", "--release", "--offline", "--locked", "--manifest-path",
         "research/harness/Cargo.toml", "--bin", "sync_probe"])
    commit = run(["git", "rev-parse", "HEAD"], stdout=subprocess.PIPE).stdout.decode().strip()
    summaries = []
    with tempfile.TemporaryDirectory(prefix="graph-search-storage-") as temporary:
        corpus, edit = create_corpus(temporary, args.synthetic_files, commit, args.synthetic_kind)
        for repeat in range(1, args.repeats + 1):
            result = json.loads(run([str(ROOT / "research/harness/target/release/sync_probe"),
                temporary, edit], stdout=subprocess.PIPE).stdout)
            result["source"] = corpus
            write(out / f"trial-{repeat}.json", result)
            before = result["initial_artifacts"]["files"]
            after = result["sync_artifacts"]["files"]
            records = {}
            for name, artifact in after.items():
                if artifact.get("records") is None:
                    continue
                old = before[name]["records"]
                new = artifact["records"]
                changed = [p for p, value in new.items() if old.get(p) != value]
                records[name] = {"total_records": len(new), "changed_records": len(changed),
                    "changed_bytes": sum(new[p]["bytes"] for p in changed),
                    "total_record_bytes": sum(v["bytes"] for v in new.values()),
                    "removed_records": len(old.keys() - new.keys())}
            summaries.append({"repeat": repeat, "sync_ms": result["sync"]["elapsed_ms"],
                "initial_ms": result["initial_ms"], "clean_reindex_ms": result["clean_reindex_ms"],
                "reopen_ms": result["reopen_ms"],
                "source_files_on_disk": sum(k.startswith("source-records/") for k in after),
                "extraction_files_on_disk": sum(k.startswith("extraction-records/") for k in after),
                "manifest_artifact_bytes": after["manifest.json"]["bytes"],
                "noop_ms": result["noop"]["elapsed_ms"],
                "generation_bytes": sum(v["bytes"] for v in after.values()),
                "shared_inode_bytes": sum(v["bytes"] for k, v in after.items()
                    if v.get("identity") is not None and v["identity"] == before.get(k, {}).get("identity")),
                "noop_generation_unchanged": result["sync_artifacts"] == result["noop_artifacts"],
                "record_changes": records})
    write(out / "summary.json", summaries)
    names = run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "crates"],
                stdout=subprocess.PIPE).stdout.split(b"\0")
    paths = [ROOT / p.decode() for p in names if p] + [ROOT / "Cargo.toml", ROOT / "Cargo.lock",
        ROOT / "research/harness/src/bin/sync_probe.rs", Path(__file__).resolve()]
    write(out / "provenance.json", {"git_head": commit, "platform": platform.platform(),
        "source_sha256": {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths},
        "repeats": args.repeats, "corpus": corpus, "edited_path": edit,
        "note": "Independent temporary copies; graph equality checked before and after reopen, with persisted source/occurrence fact equality. Logical artifact sizes, not physical I/O. Record hashes use canonical JSON values."})


if __name__ == "__main__":
    main()
