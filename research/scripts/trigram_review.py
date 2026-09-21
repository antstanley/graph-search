"""Capture native trigram admission economics without changing sibling repositories."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import sys

from markdown_context import inventory
from markdown_statistics import digest, indexes, write
from native_review import ROOT
sys.path.insert(0,str(ROOT/'evaluation'))
from taskbench.provenance import repository_snapshot


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    output=args.output.resolve();output.mkdir(parents=True,exist_ok=False)
    roots={name:ROOT.parent/name for name in ('nanus','blogwright','whatsurvey')}
    probe=ROOT/'research/harness/src/bin/trigram_probe.rs'
    before=inventory();probe_before=digest(probe)
    shutil.copy2(probe,output/'probe.rs')
    sources={name:repository_snapshot(root) for name,root in roots.items()}
    index_before={name:indexes(root) for name,root in roots.items()}
    control=Path(tempfile.mkdtemp(prefix='graph-search-trigram-control-'))
    names=[name for name in before if name.startswith(('crates/','.cargo/')) or name in ('Cargo.toml','Cargo.lock')]
    names.extend(['research/harness/Cargo.toml','research/harness/Cargo.lock','research/harness/src/bin/trigram_probe.rs'])
    for name in names:
        destination=control/name;destination.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(ROOT/name,destination)
    search=control/'crates/core/src/text_search.rs'
    original=search.read_text()
    needle='        if !work.source_file()? {'
    assert original.count(needle)==1
    injection='        if !research_admits(&entry.rel) { continue; }\n'
    helper="""
std::thread_local! {
    static RESEARCH_ADMISSION: std::cell::RefCell<Option<std::collections::BTreeSet<String>>> = const { std::cell::RefCell::new(None) };
}
/// Disposable probe-only admission control, absent from production sources.
pub fn research_admission(paths: Option<&[String]>) {
    RESEARCH_ADMISSION.with(|current| *current.borrow_mut() = paths.map(|paths| paths.iter().cloned().collect()));
}
fn research_admits(path: &str) -> bool {
    RESEARCH_ADMISSION.with(|current| current.borrow().as_ref().is_none_or(|paths| paths.contains(path)))
}
"""
    search.write_text(original.replace(needle,injection+needle)+helper)
    copied_probe=control/'research/harness/src/bin/trigram_probe.rs'
    text=copied_probe.read_text()
    old='fn configure_admission(_paths: Option<&[String]>) -> bool {\n    false\n}'
    replacement='fn configure_admission(paths: Option<&[String]>) -> bool {\n    graph_search_core::text_search::research_admission(paths);\n    true\n}'
    assert text.count(old)==1
    copied_probe.write_text(text.replace(old,replacement))
    write(output/'instrumentation.json',{'temporary_source':str(control),'search_insert_before':needle,
        'search_insertion':injection,'search_append':helper,'probe_before':old,'probe_after':replacement,
        'instrumented_search_sha256':digest(search),'instrumented_probe_sha256':digest(copied_probe)})
    subprocess.run(['cargo','build','--release','--offline','--locked','--manifest-path',
                    str(control/'research/harness/Cargo.toml'),'--bin','trigram_probe',
                    '--target-dir',str(ROOT/'research/harness/target')],cwd=control,check=True)
    binary=ROOT/'research/harness/target/release/trigram_probe';binary_before=digest(binary)
    results={}
    for name,root in roots.items():
        results[name]=json.loads(subprocess.check_output([str(binary),str(root)],cwd=ROOT,timeout=300))
        write(output/(name+'.json'),results[name])
        print(name,'complete',flush=True)
    stable={'production_and_drivers':before==inventory(),'probe':probe_before==digest(probe),
            'binary':binary_before==digest(binary),
            'siblings':sources=={name:repository_snapshot(root) for name,root in roots.items()},
            'indexes':index_before=={name:indexes(root) for name,root in roots.items()}}
    write(output/'provenance.json',{'source_sha256':before,'probe_sha256':probe_before,
        'binary_sha256':binary_before,'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),
        'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        'siblings_before':sources,'indexes_before':index_before,'stability':stable,
        'scope':'Research prototype only; resident scan timings and separate production direct-scan/strict-read-hash timings. Disposable admission hook preserves the production scanner and budgets. Trusted stable-snapshot timing, not a shipped filtered API, persistence, RSS or cold-device-I/O claim.'})
    if not all(stable.values()):raise ValueError('capture fingerprint drift')


if __name__=='__main__':main()
