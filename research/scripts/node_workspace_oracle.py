#!/usr/bin/env python3
"""Independent installed pnpm/Node probes; no install, network, or application code."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def run(command, cwd):
    env = dict(os.environ, COREPACK_ENABLE_NETWORK="0", COREPACK_ENABLE_PROJECT_SPEC="0", COREPACK_ENABLE_AUTO_PIN="0")
    result = subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True, timeout=90)
    assert result.returncode == 0, (command, result.stdout, result.stderr)
    return result.stdout.strip()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    versions = {tool: run([tool, "--version"], Path.cwd()) for tool in ["node", "pnpm"]}
    captures = []
    with tempfile.TemporaryDirectory(prefix="graph-search-workspace-oracle-") as directory:
        root = Path(directory)
        packages = {"": "root", "packages/api": "api", "packages/client": "client", "packages/excluded": "excluded", "packages/deep/child": "child", "packages/client/private/api": "nested", "packages/.hidden": "hidden", "outside": "outside"}
        for path, name in packages.items():
            package = root / path
            package.mkdir(parents=True, exist_ok=True)
            (package / "package.json").write_text(json.dumps(dict(name=name, version="1.0.0", type="module")))
        client_manifest = root / "packages/client/package.json"
        client_data = json.loads(client_manifest.read_text())
        client_data["workspaces"] = ["private/*"]
        client_manifest.write_text(json.dumps(client_data))
        cases = [
            ("direct_and_exclusion", "packages:\n  - packages/*\n  - '!packages/excluded'\n", {"root", "api", "client"}),
            ("recursive", "packages:\n  - packages/**\n  - '!**/excluded'\n", {"root", "api", "client", "child", "nested"}),
            ("literal_hidden", "packages:\n  - packages/.hidden\n", {"root", "hidden"}),
            ("empty", "packages: []\n", {"root"}),
        ]
        for label, text, expected in cases:
            (root / "pnpm-workspace.yaml").write_text(text)
            observed = json.loads(run(["pnpm", "--dir", str(root), "list", "--recursive", "--depth", "-1", "--json"], root))
            names = {entry["name"] for entry in observed}
            assert names == expected, (label, names, expected)
            captures.append(dict(case=label, observed=sorted(names), expected=sorted(expected)))
        api = root / "packages/api"
        client = root / "packages/client"
        (client / "node_modules").mkdir()
        (client / "node_modules/api").symlink_to(api, target_is_directory=True)
        (api / "index.js").write_text("export function marker(){return 'runtime'}\n")
        (api / "types.js").write_text("export function marker(){return 'types'}\n")
        for label, exports, expected in [
            ("uniform", {"types": "./index.js", "default": "./index.js"}, ["runtime", "runtime", "runtime"]),
            ("divergent", {"types": "./types.js", "default": "./index.js"}, ["runtime", "types", "runtime"]),
            ("nested_uniform", {"node": {"import": "./index.js", "default": "./index.js"}, "default": "./index.js"}, ["runtime", "runtime", "runtime"]),
        ]:
            (api / "package.json").write_text(json.dumps(dict(name="api", type="module", exports=exports)))
            observed = []
            for flags in [[], ["--conditions=types"], ["--conditions=custom"]]:
                observed.append(run(["node", *flags, "--input-type=module", "--eval", "import {marker} from 'api'; console.log(marker());"], client))
            assert observed == expected, (label, observed)
            captures.append(dict(case=label, conditions=["default", "types", "custom"], observed=observed, expected=expected, native_binding_expected=label != "divergent"))
    args.out.write_text(json.dumps(dict(versions=versions, scope="Independent membership and condition-invariance oracles over authored temporary fixtures; no installation or application execution", captures=captures), indent=2) + "\n")


if __name__ == "__main__":
    main()
