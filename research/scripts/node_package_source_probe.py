#!/usr/bin/env python3
"""Source-backed four-file whatsurvey fixture; never executes application code."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

FILES = [
    "workspaces/backend/package.json",
    "workspaces/backend/src/core/index.ts",
    "workspaces/backend/src/core/whatsapp/flow-json.ts",
    "workspaces/backend/src/functions/admin-api/routes/flows.ts",
]
TARGET = "sym:workspaces/backend/src/core/whatsapp/flow-json.ts#function:compileFlowJson"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--in-tree-store", action="store_true")
    args = parser.parse_args()
    cli = args.cli.resolve()
    binary_hash = digest(cli.read_bytes())
    sources = {path: (args.repo / path).read_bytes() for path in FILES}
    manifest = json.loads(sources[FILES[0]])
    assert manifest["imports"]["#core"] == "./src/core/index.ts"
    assert b'compileFlowJson,' in sources[FILES[1]]
    assert b'"./whatsapp/flow-json.js"' in sources[FILES[1]]
    assert b'export function compileFlowJson(' in sources[FILES[2]]
    assert b'} from "#core";' in sources[FILES[3]]
    assert sources[FILES[3]].count(b'compileFlowJson(draft.definition)') == 2
    captures = {}
    with tempfile.TemporaryDirectory(prefix="graph-search-node-source-") as directory:
        temporary = Path(directory)
        root = temporary / "source"
        store = root / "index-store" if args.in_tree_store else temporary / "store"
        for path, data in sources.items():
            destination = root / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        base = [str(cli), "--root", str(root), "--store", str(store), "--json"]

        def invoke(*command):
            run = subprocess.run(base + list(command), capture_output=True, text=True, timeout=120)
            assert run.returncode == 0, run.stderr
            return json.loads(run.stdout)

        def capture():
            payload = invoke("--no-reconcile", "search", "occurrences", "compileFlowJson", "--by", "name", "--rel", "calls", "--path", FILES[3])
            assert not payload["stale"], payload["stale_paths"]
            assert payload["results"]["indexed_files"] == 4
            items = payload["results"]["items"]
            assert len(items) == 2
            return dict(items=items, indexed_files=4, versions=payload["context"]["indexed_versions"])

        invoke("index")
        captures["mapped"] = capture()
        for item in captures["mapped"]["items"]:
            assert item["occurrence"]["target"] == TARGET
            assert item["occurrence"]["resolution"] == "explicit_import"
        del manifest["imports"]
        (root / FILES[0]).write_text(json.dumps(manifest))
        invoke("sync")
        captures["mapping_removed_in_copy"] = capture()
        for item in captures["mapping_removed_in_copy"]["items"]:
            assert item["occurrence"]["target"] is None
            assert item["occurrence"]["reason"] == "node_import_map_missing"
        (root / FILES[0]).write_bytes(sources[FILES[0]])
        invoke("sync")
        captures["mapping_restored_in_copy"] = capture()
        assert captures["mapped"] == captures["mapping_restored_in_copy"]
    assert all((args.repo / path).read_bytes() == data for path, data in sources.items())
    assert digest(cli.read_bytes()) == binary_hash
    args.out.write_text(json.dumps(dict(in_tree_store=args.in_tree_store, scope="four-file source-backed fixture, not a repository-wide benchmark; no application code executes", source_sha256={path: digest(data) for path, data in sources.items()}, cli_sha256=binary_hash, sources_unchanged=True, captures=captures), indent=2) + "\n")


if __name__ == "__main__":
    main()
