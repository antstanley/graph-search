"""Interleave native storage variants on identical corpora.

The control is built in a disposable source checkout. Production files are never
patched. Both binaries use the existing offline dependency cache and release profile.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

from storage_review import ROOT, create_corpus, run, write


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(binary, corpus, edit, label):
    result = json.loads(run([str(binary), str(corpus), edit], stdout=subprocess.PIPE).stdout)
    result["source"] = label
    for key in ("equivalent_to_clean_reindex", "reopen_equivalent_to_clean_reindex",
                "fact_records_equivalent_to_clean_reindex"):
        if result[key] is not True:
            raise ValueError(f"Failed correctness check: {key}")
    if result["sync_artifacts"] != result["noop_artifacts"]:
        raise ValueError("No-op changed a generation")
    return result


def metrics(result):
    before = result["initial_artifacts"]["files"]
    after = result["sync_artifacts"]["files"]
    return {"initial_ms": result["initial_ms"], "sync_ms": result["sync"]["elapsed_ms"],
            "clean_reindex_ms": result["clean_reindex_ms"], "noop_ms": result["noop"]["elapsed_ms"],
            "reopen_ms": result["reopen_ms"],
            "generation_bytes": sum(v["bytes"] for v in after.values()),
            "shared_inode_bytes": sum(v["bytes"] for name, v in after.items()
                if v.get("identity") is not None and v["identity"] == before.get(name, {}).get("identity")),
            "source_files_on_disk": sum(name.startswith("source-records/") for name in after),
            "extraction_files_on_disk": sum(name.startswith("extraction-records/") for name in after),
            "manifest_artifact_bytes": after["manifest.json"]["bytes"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--pairs", type=int, default=7)
    parser.add_argument("--synthetic-files", type=int, default=512)
    parser.add_argument("--control", choices=("whole-map", "intermediate-indexes", "whole-manifest", "repeated-directory-syncs", "metadata-rebuild"),
                        default="whole-map", help="Isolated operation restored in the control checkout")
    parser.add_argument("--synthetic-kind", choices=("markdown", "rust"), default="markdown")
    args = parser.parse_args()
    candidate_name, control_name = {
        "whole-map": ("packed", "whole"),
        "intermediate-indexes": ("final", "intermediate"),
        "whole-manifest": ("split", "embedded"),
        "repeated-directory-syncs": ("batched", "repeated"),
        "metadata-rebuild": ("deltas", "full"),
    }[args.control]
    if args.pairs < 1 or not 1 <= args.synthetic_files <= 4096:
        parser.error("pairs must be positive; synthetic-files must be 1..4096")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    target = ROOT / "research/harness/target"
    executable = target / "release/sync_probe"
    build = ["cargo", "build", "--release", "--offline", "--locked", "--manifest-path"]
    run(build + ["research/harness/Cargo.toml", "--bin", "sync_probe"])
    commit = run(["git", "rev-parse", "HEAD"], stdout=subprocess.PIPE).stdout.decode().strip()
    names = run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z",
        "Cargo.toml", "Cargo.lock", "crates", ".cargo", "research/harness/Cargo.toml",
        "research/harness/Cargo.lock", "research/harness/src"], stdout=subprocess.PIPE).stdout.split(b"\0")
    sources = [Path(name.decode()) for name in names if name]
    hashes = {str(p): digest(ROOT / p) for p in sources}
    with tempfile.TemporaryDirectory(prefix="graph-search-storage-pairs-") as temporary:
        work = Path(temporary)
        packed = work / "candidate-probe"
        whole = work / "control-probe"
        shutil.copy2(executable, packed)
        checkout = work / "control"
        for relative in sources:
            destination = checkout / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, destination)
        control_store = checkout / "crates/engine/src/store.rs"
        text = control_store.read_text()
        if args.control == "whole-map":
            start = text.index("        // apply_prepared only removes or replaces source facts")
            end = text.index('        self.inject("after_source_persist")?', start)
            replacement = ("        sidecar::save_sources(dir, &self.sources).map_err(store_io)?;\n"
                           "        self.source_records = None;\n")
        elif args.control == "intermediate-indexes":
            marker = "        prepared.apply_prepared(&self.complete_projection()?)?;\n"
            if text.count(marker) != 1:
                raise ValueError("Expected exactly one intermediate projection")
            start = text.index(marker)
            end = start + len(marker)
            replacement = marker + "        prepared.refresh_indexes(None)?;\n"
        elif args.control == "metadata-rebuild":
            marker = "        prepared.refresh_indexes(Some(&self.metadata))?;\n"
            if text.count(marker) != 1:
                raise ValueError("Expected exactly one final metadata delta update")
            start = text.index(marker)
            end = start + len(marker)
            replacement = "        prepared.refresh_indexes(None)?;\n"
        elif args.control == "whole-manifest":
            start = text.index("        if let Some(manifest) = manifest {", text.index("fn persist_prepared("))
            end = text.index('        self.inject("after_manifest_persist")?', start)
            replacement = ("        if let Some(manifest) = manifest {\n"
                "            sidecar::prepare_manifest(dir, manifest).map_err(store_io)?;\n"
                "            self.extraction_records = None;\n"
                "        }\n")
            generation = checkout / "crates/engine/src/generation.rs"
            original_generation = generation.read_text()
            marker = "const FORMAT: u32 = 6;"
            if original_generation.count(marker) != 1:
                raise ValueError("Expected generation format 6 for embedded-manifest control")
            generation.write_text(original_generation.replace(marker, "const FORMAT: u32 = 5;"))
            write(out / "control-generation-edit.json", {"file": "crates/engine/src/generation.rs",
                "before": marker, "after": "const FORMAT: u32 = 5;", "control_file_sha256": digest(generation)})
        else:
            start = text.index("    fn persist_prepared(")
            end = text.index("    // Private preparation only:", start)
            replacement = text[start:end]
            for name in ("manifest", "dangling"):
                marker = f"sidecar::prepare_{name}("
                if replacement.count(marker) != 1:
                    raise ValueError(f"Expected one prepared {name} call")
                replacement = replacement.replace(marker, f"sidecar::save_{name}(")
        control_store.write_text(text[:start] + replacement + text[end:])
        write(out / "control-edit.json", {"file": "crates/engine/src/store.rs",
            "before": text[start:end], "after": replacement,
            "control_file_sha256": digest(control_store)})
        try:
            run(build + [str(checkout / "research/harness/Cargo.toml"), "--target-dir", str(target),
                         "--bin", "sync_probe"], stdout=subprocess.PIPE)
            shutil.copy2(executable, whole)
        finally:
            # The shared target directory also has a top-level executable; keep
            # the production binary there, irrespective of control-build outcome.
            restored = executable.with_suffix(".restore")
            shutil.copy2(packed, restored)
            restored.replace(executable)
        write(out / "provenance.json", {"git_head": commit, "platform": platform.platform(),
            "source_sha256": hashes, "driver_sha256": digest(Path(__file__).resolve()),
            "corpus_driver_sha256": digest(ROOT / "research/scripts/storage_review.py"),
            "binary_sha256": {candidate_name: digest(packed), control_name: digest(whole)},
            "control": args.control,
            "pairs": args.pairs, "synthetic_files": args.synthetic_files, "synthetic_kind": args.synthetic_kind,
            "note": "One unmeasured warmup per binary/corpus; alternating order by pair. Each process copies the same corpus into a fresh disposable directory. Logical bytes, not physical I/O."})
        summaries = []
        for corpus_name, count in (("frozen", 0), ("synthetic", args.synthetic_files)):
            corpus = work / corpus_name
            corpus.mkdir()
            label, edit = create_corpus(corpus, count, commit, args.synthetic_kind)
            for binary in (packed, whole):
                capture(binary, corpus, edit, label)
            for pair in range(1, args.pairs + 1):
                order = ((candidate_name, control_name) if pair % 2
                         else (control_name, candidate_name))
                row = {"corpus": label, "pair": pair, "order": order}
                for variant in order:
                    result = capture(packed if variant == candidate_name else whole, corpus, edit, label)
                    write(out / f"{corpus_name}-{pair}-{variant}.json", result)
                    row[variant] = metrics(result)
                row["sync_delta_ms"] = row[candidate_name]["sync_ms"] - row[control_name]["sync_ms"]
                row["initial_delta_ms"] = row[candidate_name]["initial_ms"] - row[control_name]["initial_ms"]
                summaries.append(row)
                write(out / "summary.json", summaries)
                print(f"{corpus_name} pair {pair}: sync delta {row['sync_delta_ms']} ms", flush=True)
        if any(digest(ROOT / p) != hashes[str(p)] for p in sources):
            raise ValueError("Production sources changed during the comparison")


if __name__ == "__main__":
    main()
