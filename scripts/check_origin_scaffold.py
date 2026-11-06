#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Generate ORIGIN from the packaged template and run its linked acceptance tests."""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def run(arguments, cwd, environment):
    print("$ " + " ".join(map(str, arguments)), flush=True)
    subprocess.run(arguments, cwd=cwd, env=environment, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    args = parser.parse_args()
    cli = args.cli.resolve()
    environment = os.environ | {"CARGO_TARGET_DIR": str(
        (ROOT / os.environ.get("CARGO_TARGET_DIR", "target")).resolve())}
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"], cwd=ROOT))
    packages = {p["name"]: Path(p["manifest_path"]).parent for p in metadata["packages"]}
    workspace_dependencies = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["dependencies"]
    with tempfile.TemporaryDirectory(prefix="proa-origin-") as temporary:
        work = Path(temporary).resolve()
        run([cli, "new", "site", "origin-atlas", "--template", "origin", "--no-git"], work, environment)
        project = work / "origin-atlas"
        # This lane tests the workspace compiler, while registry acceptance
        # separately exercises the generated minimum-supported toolchain.
        (project / "rust-toolchain.toml").write_bytes((ROOT / "rust-toolchain.toml").read_bytes())
        # Reuse the canonical font fixtures so this lane needs no font CDN.
        template = ROOT / "proa_cli/src/site_scaffold/templates/origin"
        shutil.copy2(template / "proa-fonts.lock", project / "proa-fonts.lock")
        shutil.copytree(template / "public/fonts", project / "public/fonts", dirs_exist_ok=True)
        manifest = project / "Cargo.toml"
        source = manifest.read_text()
        dependencies = tomllib.loads(source)
        # Use checkout paths for Proa and workspace settings for registry
        # dependencies (including rustls rather than native TLS).
        for section in ("dependencies", "build-dependencies", "dev-dependencies"):
            for name, spec in dependencies.get(section, {}).items():
                options = dict(spec) if isinstance(spec, dict) else {"version": spec}
                if name in packages:
                    options.pop("version", None)
                    options["path"] = str(packages[name])
                elif name in workspace_dependencies:
                    shared = workspace_dependencies[name]
                    shared = dict(shared) if isinstance(shared, dict) else {"version": shared}
                    features = sorted(set(shared.get("features", [])) | set(options.get("features", [])))
                    options = shared
                    if features:
                        options["features"] = features
                else:
                    continue
                fields = ", ".join(f"{key} = {json.dumps(value)}" for key, value in options.items())
                source = re.sub(r"(?m)^" + re.escape(name) + r"\s*=.*$",
                                lambda _: f"{name} = {{ {fields} }}", source)
        manifest.write_text(source + "\n[workspace]\n")
        # Keep the workspace's dependency versions: CI fetches this exact lock
        # before running acceptance, including on runners with an empty cache.
        (project / "Cargo.lock").write_bytes((ROOT / "Cargo.lock").read_bytes())
        run(["cargo", "tree", "--offline", "--depth", "0"], project, environment)
        run(["cargo", "test", "--locked", "--lib"], project, environment)
        run([cli, "test", "--root", project, "--locked", "--test", "atlas"], project, environment)


if __name__ == "__main__":
    main()
