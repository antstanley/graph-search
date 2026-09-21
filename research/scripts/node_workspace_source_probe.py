#!/usr/bin/env python3
"""Read-only six-file whatsurvey capture, mutated only inside a disposable fixture."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

FILES = ["package.json", "pnpm-workspace.yaml", "packages/types/package.json", "packages/types/src/contact-name.ts", "workspaces/backend/package.json", "workspaces/backend/src/core/db/contacts.ts"]
TARGET = "sym:packages/types/src/contact-name.ts#function:contactNameParts"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    cli = args.cli.resolve()
    binary_hash = digest(cli.read_bytes())
    source = {path: (args.repo / path).read_bytes() for path in FILES}
    manifest = json.loads(source[FILES[4]])
    assert manifest["dependencies"]["@whatsurvey/types"] == "workspace:*"
    target_manifest = json.loads(source[FILES[2]])
    assert target_manifest["exports"]["./contact-name"] == {"types": "./src/contact-name.ts", "default": "./src/contact-name.ts"}
    assert b'import { contactNameParts } from "@whatsurvey/types/contact-name";' in source[FILES[5]]
    assert b'  - packages/*\n' in source[FILES[1]]
    captures = {}
    with tempfile.TemporaryDirectory(prefix="graph-search-workspace-source-") as directory:
        root = Path(directory)
        for path, data in source.items():
            destination = root / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        base = [str(cli), "--root", str(root), "--store", str(root / "index-store"), "--json"]
        def invoke(*command):
            result = subprocess.run(base + list(command), capture_output=True, text=True, timeout=120)
            assert result.returncode == 0, result.stderr
            return json.loads(result.stdout)
        def capture(label, reason=None):
            payload = invoke("--no-reconcile", "search", "occurrences", "contactNameParts", "--by", "name", "--rel", "calls", "--path", FILES[5])
            assert not payload["stale"], payload["stale_paths"]
            assert payload["results"]["indexed_files"] == 6
            items = payload["results"]["items"]
            assert len(items) == 5, items
            for item in items:
                occurrence = item["occurrence"]
                if reason is None:
                    assert occurrence["target"] == TARGET, occurrence
                    assert occurrence["resolution"] == "explicit_import", occurrence
                else:
                    assert occurrence["target"] is None, occurrence
                    assert occurrence["reason"] == reason, occurrence
            captures[label] = dict(indexed_files=6, items=items, versions=payload["context"]["indexed_versions"])
        invoke("index")
        capture("declared_workspace")
        (root / FILES[1]).write_bytes(source[FILES[1]].replace(b'  - packages/*\n', b''))
        invoke("sync")
        capture("membership_removed_in_copy", "node_workspace_target_missing")
        (root / FILES[1]).write_bytes(source[FILES[1]])
        invoke("sync")
        capture("membership_restored")
        del manifest["dependencies"]["@whatsurvey/types"]
        (root / FILES[4]).write_text(json.dumps(manifest))
        invoke("sync")
        capture("dependency_removed_in_copy", "node_dependency_not_declared")
        (root / FILES[4]).write_bytes(source[FILES[4]])
        invoke("sync")
        capture("dependency_restored")
        assert captures["declared_workspace"] == captures["membership_restored"] == captures["dependency_restored"]
    assert all((args.repo / path).read_bytes() == data for path, data in source.items())
    assert digest(cli.read_bytes()) == binary_hash
    args.out.write_text(json.dumps(dict(scope="six-file source-backed correctness fixture, not a whole-repository quality or timing claim; no application code executes", source_sha256={path:digest(data) for path,data in source.items()}, cli_sha256=binary_hash, sources_unchanged=True, captures=captures), indent=2) + "\n")


if __name__ == "__main__":
    main()
