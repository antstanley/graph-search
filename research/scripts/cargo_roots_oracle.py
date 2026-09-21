#!/usr/bin/env python3
"""Disposable Cargo/rustc semantic oracle; neither command runs in user repos.

No registry access, dependencies, compilation of build scripts, or source/index
mutation. The production resolver does not invoke either tool.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

BASE_FILES = {
    "lib": ["src/lib.rs"],
    "bin": ["src/main.rs", "src/bin/worker.rs"],
    "example": ["examples/demo.rs"],
    "test": ["tests/check.rs"],
    "bench": ["benches/speed.rs"],
}


def invoke(args):
    return subprocess.run(args, text=True, capture_output=True, check=False)


def case(name, manifest, files, expected):
    with tempfile.TemporaryDirectory(prefix="graph-search-cargo-oracle-") as temp:
        root = Path(temp)
        (root / "Cargo.toml").write_text(manifest)
        for path in files:
            target = root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("fn main() {}\n")
        command = ["cargo", "metadata", "--offline", "--no-deps", "--format-version", "1", "--manifest-path", str(root / "Cargo.toml")]
        result = invoke(command)
        actual = []
        if result.returncode == 0:
            actual = sorted(str(Path(target["src_path"]).relative_to(root))
                            for target in json.loads(result.stdout)["packages"][0]["targets"])
        passed = result.returncode != 0 if expected is None else result.returncode == 0 and actual == sorted(expected)
        return {"case": name, "manifest": manifest, "files": files,
                "expected_paths": expected, "actual_paths": actual,
                "exit_code": result.returncode, "passed": passed,
                "stderr": result.stderr.replace(temp, "<fixture>")}


def module_paths():
    files = {
        "custom/entry.rs": '#[path="flat.rs"] pub mod path_loaded; pub mod inner_file; pub mod outer; pub mod inline { pub mod child; }\n#[path="thread_files"] pub mod threaded { #[path="tls.rs"] pub mod data; }\n',
        "custom/flat.rs": 'pub mod special;\n',
        "custom/special.rs": "",
        "custom/flat/special.rs": 'compile_error!("path-loaded child uses physical parent");\n',
        "custom/inner_file.rs": '#![path="ignored_name.rs"]\npub mod ghost;\n',
        "custom/ghost.rs": "",
        "custom/inner_file/ghost.rs": 'compile_error!("inner file attribute changes lookup");\n',
        "custom/outer.rs": 'pub mod child; #[path="near.rs"] pub mod near; pub mod nested { #[path="other.rs"] pub mod file; }\n',
        "custom/outer/child.rs": "",
        "custom/near.rs": "",
        "custom/outer/nested/other.rs": "",
        "custom/inline/child.rs": "",
        "custom/thread_files/tls.rs": "",
        "custom/child.rs": 'compile_error!("wrong sibling");\n',
        "custom/entry/outer.rs": 'compile_error!("root treated as ordinary module");\n',
    }
    with tempfile.TemporaryDirectory(prefix="graph-search-rust-module-oracle-") as temp:
        root = Path(temp)
        for path, source in files.items():
            target = root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(source)
        result = invoke(["rustc", "--edition=2021", "--crate-type=lib", "--emit=metadata", str(root / "custom/entry.rs"), "-o", str(root / "module.rmeta")])
        return {"files": files, "exit_code": result.returncode,
                "passed": result.returncode == 0,
                "stderr": result.stderr.replace(temp, "<fixture>")}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    records = []
    defaults = [path for paths in BASE_FILES.values() for path in paths]
    for edition in ["2015", "2021"]:
        package = f"[package]\nname='p'\nversion='0.1.0'\nedition='{edition}'\n"
        for kind in BASE_FILES:
            table = "[lib]" if kind == "lib" else f"[[{kind}]]"
            manifest = package + table + "\nname='explicit'\npath='entry.rs'\n"
            expected = [path for family, paths in BASE_FILES.items() for path in paths
                        if family != kind or (edition != "2015" and kind != "lib")]
            records.append(case(f"{edition}-explicit-{kind}", manifest, defaults + ["entry.rs"], expected + ["entry.rs"]))
            if kind != "lib":
                expected = [path for family, paths in BASE_FILES.items() for path in paths
                            if family != kind or edition != "2015"]
                records.append(case(f"{edition}-empty-{kind}", f"{kind}=[]\n" + package, defaults, expected))
    package = "[package]\nname='p'\nversion='0.1.0'\nedition='2021'\n"
    for setting, selected in [("", "build.rs"), ("build=true\n", "build.rs"), ("build=false\n", None), ("build='tools/setup.rs'\n", "tools/setup.rs")]:
        records.append(case(f"build-{setting or 'omitted'}", package + setting,
                            ["src/lib.rs", "build.rs", "tools/setup.rs"],
                            ["src/lib.rs"] + ([selected] if selected else [])))
    records.append(case("hidden-layout-is-not-auto-discovered", package,
                        ["src/lib.rs", "src/bin/.hidden.rs", "src/bin/.folder/main.rs", "src/bin/task.rs", "src/bin/task/helpers.rs", "examples/.hidden.rs", "tests/.hidden/main.rs", "benches/.hidden.rs"],
                        ["src/lib.rs", "src/bin/task.rs"]))
    records.append(case("duplicate-binary-layout", package, defaults + ["src/bin/worker/main.rs"], None))
    records.append(case("missing-binary-name", package + "[[bin]]\npath='entry.rs'\n", defaults + ["entry.rs"], None))
    modules = module_paths()
    result = {"cargo": invoke(["cargo", "--version"]).stdout.strip(),
              "rustc": invoke(["rustc", "--version"]).stdout.strip(),
              "cases": records, "module_paths": modules,
              "passed": all(record["passed"] for record in records) and modules["passed"]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"cases": len(records), "module_compile": modules["passed"], "passed": result["passed"]}))
    if not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
