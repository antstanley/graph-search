"""Profile native storage phases in a disposable instrumented checkout.

Timers are diagnostic and can overlap; they are not an uninstrumented latency
comparison. Production source and executable are preserved.
"""
import argparse
from collections import defaultdict
import difflib
import json
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tempfile

from storage_compare import digest, metrics
from storage_review import ROOT, create_corpus, run, write


def replace_once(text, before, after):
    if text.count(before) != 1:
        raise ValueError(f"Expected one instrumentation site: {before[:100]}")
    return text.replace(before, after, 1)


def instrument(checkout, out):
    edits = {}
    def edit(relative, transform):
        path = checkout / relative
        before = path.read_text()
        after = transform(before)
        path.write_text(after)
        edits[relative] = {"sha256": digest(path), "patch": "".join(difflib.unified_diff(
            before.splitlines(keepends=True), after.splitlines(keepends=True),
            fromfile=relative, tofile=relative))}

    def records(text):
        start = text.index("    fn load_inner<T: DeserializeOwned>(")
        end = text.index("\nfn valid_hash", start)
        part = text[start:end]
        part = part.replace("        let mut files = BTreeMap::new();", "        let mut files = BTreeMap::new();\n        let mut times = [0u128; 3];")
        part = part.replace("            let bytes = read_pack", "            let timer = std::time::Instant::now();\n            let bytes = read_pack")
        part = part.replace("            if verify_records {", "            times[0] += timer.elapsed().as_micros();\n            let timer = std::time::Instant::now();\n            if verify_records {")
        part = part.replace("            for (path, reference) in group {", "            times[1] += timer.elapsed().as_micros();\n            let timer = std::time::Instant::now();\n            for (path, reference) in group {")
        part = part.replace("        }\n        Ok(files)", "            times[2] += timer.elapsed().as_micros();\n        }\n        for (name, us) in [\"load_read_pack_hash\", \"load_ranges_and_optional_record_hash\", \"load_decode\"].into_iter().zip(times) { profile_us(name, layout.index, us); }\n        Ok(files)")
        text = text[:start] + part + text[end:]
        start = text.index("pub(crate) fn save_records<T: Serialize>(")
        end = text.index("\n#[cfg(test)]", start)
        part = text[start:end]
        part = part.replace("    let mut reusable:", "    let timer = std::time::Instant::now();\n    let mut reusable:")
        part = part.replace("    let mut small =", '    profile("reuse_decision", layout.index, timer);\n    let timer = std::time::Instant::now();\n    let mut small =')
        part = part.replace("    for (path, record) in files {", '    profile("retain_packs", layout.index, timer);\n    let timer = std::time::Instant::now();\n    for (path, record) in files {')
        part = part.replace("    crate::generation::sync_dir(&directory)?;", '    profile("new_records_and_flush", layout.index, timer);\n    let timer = std::time::Instant::now();\n    crate::generation::sync_dir(&directory)?;')
        part = part.replace("    Ok(index)", '    profile("record_index_commit", layout.index, timer);\n    Ok(index)')
        text = text[:start] + part + text[end:]
        return text + '''
pub(crate) fn profile(name: &str, kind: &str, started: std::time::Instant) {
    profile_us(name, kind, started.elapsed().as_micros());
}
fn profile_us(name: &str, kind: &str, us: u128) {
    eprintln!("@storage-phase {}", serde_json::json!({"phase":name,"kind":kind,"us":us}));
}
'''
    edit("crates/engine/src/source_records.rs", records)

    def generation(text):
        before = '''    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(tmp, path)'''
        after = '''    let parent = path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or("");
    let kind = if ["source-records", "extraction-records"].contains(&parent) { parent } else { path.file_name().and_then(|s| s.to_str()).unwrap_or("other") };
    let timer = std::time::Instant::now();
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    crate::source_records::profile("replace_write", kind, timer);
    let timer = std::time::Instant::now();
    file.sync_all()?;
    crate::source_records::profile("replace_file_sync", kind, timer);
    let timer = std::time::Instant::now();
    std::fs::rename(tmp, path)?;
    crate::source_records::profile("replace_rename", kind, timer);
    Ok(())'''
        return replace_once(text, before, after)
    edit("crates/engine/src/generation.rs", generation)

    def sidecar(text):
        text = replace_once(text, 'pub(crate) fn prepare_manifest(store_dir: &Path, manifest: &Manifest) -> std::io::Result<()> {', 'pub(crate) fn prepare_manifest(store_dir: &Path, manifest: &Manifest) -> std::io::Result<()> {\n    let timer = std::time::Instant::now();')
        start = text.index("pub(crate) fn prepare_manifest(")
        end = text.index("\n/// Loads every", start)
        part = text[start:end].replace("    let mut file =", '    crate::source_records::profile("manifest_serialize", "manifest.json", timer);\n    let timer = std::time::Instant::now();\n    let mut file =').replace("    std::fs::rename(&tmp, &target)", '    std::fs::rename(&tmp, &target)?;\n    crate::source_records::profile("manifest_file_commit", "manifest.json", timer);\n    Ok(())')
        return text[:start] + part + text[end:]
    edit("crates/engine/src/sidecar.rs", sidecar)

    def stages(text):
        for marker, name in [("    let initial = p.reindex", "initial"), ("    let sync = p.sync", "sync"),
                             ("    let rebuild = p.reindex", "clean"), ("    let noop = p.sync", "noop"),
                             ("    let reopen_started =", "reopen"), ("    let reopened_noop = p.sync", "reopened_noop")]:
            text = replace_once(text, marker, f'    eprintln!("@storage-stage {name}");\n' + marker)
        for marker in ["    let initial_artifacts =", "    let sync_artifacts =", "    let equivalent =", "    let noop_artifacts =", "    let reopen_equivalent =", "    println!("]:
            text = replace_once(text, marker, '    eprintln!("@storage-stage validation");\n' + marker)
        return text
    edit("research/harness/src/bin/sync_probe.rs", stages)
    write(out / "instrumentation.json", edits)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--synthetic-files", type=int, default=128)
    args = parser.parse_args()
    if args.repeats < 1 or not 1 <= args.synthetic_files <= 4096:
        parser.error("positive repeats and 1..4096 synthetic files required")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    build = ["cargo", "build", "--release", "--offline", "--locked", "--manifest-path"]
    run(build + ["research/harness/Cargo.toml", "--bin", "sync_probe"])
    target = ROOT / "research/harness/target"
    executable = target / "release/sync_probe"
    commit = run(["git", "rev-parse", "HEAD"], stdout=subprocess.PIPE).stdout.decode().strip()
    names = run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "Cargo.toml",
        "Cargo.lock", "crates", ".cargo", "research/harness/Cargo.toml", "research/harness/Cargo.lock",
        "research/harness/src"], stdout=subprocess.PIPE).stdout.split(b"\0")
    sources = [Path(name.decode()) for name in names if name]
    hashes = {str(p): digest(ROOT / p) for p in sources}
    with tempfile.TemporaryDirectory(prefix="graph-search-storage-phases-") as temporary:
        work = Path(temporary)
        production = work / "production-probe"
        profile = work / "profile-probe"
        shutil.copy2(executable, production)
        checkout = work / "instrumented"
        for relative in sources:
            destination = checkout / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, destination)
        instrument(checkout, out)
        try:
            run(build + [str(checkout / "research/harness/Cargo.toml"), "--target-dir", str(target), "--bin", "sync_probe"])
            shutil.copy2(executable, profile)
        finally:
            restored = executable.with_suffix(".restore")
            shutil.copy2(production, restored)
            restored.replace(executable)
        write(out / "provenance.json", {"git_head":commit, "platform":platform.platform(),
            "source_sha256":hashes, "driver_sha256":digest(Path(__file__)),
            "binary_sha256":{"production":digest(production), "instrumented":digest(profile)},
            "repeats":args.repeats, "synthetic_files":args.synthetic_files,
            "note":"Diagnostic timers include logging overhead and nested phases. Do not sum replace_* subphases with enclosing save phases."})
        summaries = []
        for corpus_name, count in (("frozen", 0), ("rust", args.synthetic_files)):
            corpus = work / corpus_name
            corpus.mkdir()
            label, edit = create_corpus(corpus, count, commit, "rust")
            for repeat in range(1, args.repeats + 1):
                result = run([str(profile), str(corpus), edit], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                trial = json.loads(result.stdout)
                trial["source"] = label
                assert all(trial[k] for k in ("equivalent_to_clean_reindex", "reopen_equivalent_to_clean_reindex", "fact_records_equivalent_to_clean_reindex"))
                assert trial["noop_artifacts"] == trial["sync_artifacts"]
                write(out / f"{corpus_name}-{repeat}.json", trial)
                stage = "setup"
                events = []
                totals = defaultdict(int)
                for line in result.stderr.decode().splitlines():
                    if line.startswith("@storage-stage "):
                        stage = line.removeprefix("@storage-stage ")
                    elif line.startswith("@storage-phase "):
                        event = json.loads(line.removeprefix("@storage-phase "))
                        event["stage"] = stage
                        events.append(event)
                        totals[f"{stage}/{event['kind']}/{event['phase']}"] += event["us"]
                write(out / f"{corpus_name}-{repeat}-phases.json", events)
                summaries.append({"corpus":label,"repeat":repeat,"metrics":metrics(trial),"phase_us":dict(totals)})
                write(out / "summary.json", summaries)
                print(f"{corpus_name} repeat {repeat} complete", flush=True)
        if any(digest(ROOT / p) != hashes[str(p)] for p in sources):
            raise ValueError("Production sources changed during profiling")
        medians = {}
        for corpus in sorted({r["corpus"] for r in summaries}):
            rows = [r["phase_us"] for r in summaries if r["corpus"] == corpus]
            medians[corpus] = {key:statistics.median(row.get(key,0) for row in rows)
                for key in sorted(set().union(*(row.keys() for row in rows)))}
        write(out / "median-phases.json", medians)


if __name__ == "__main__":
    main()
