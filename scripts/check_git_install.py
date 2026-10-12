#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Install the committed CLI from Git outside the checkout, without registry config."""

from __future__ import annotations

import os
import json
from pathlib import Path
import subprocess
import tempfile
import re
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    # Sharing downloaded crates is fine; never share config or credentials.
    cache = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")) / "registry"
    with tempfile.TemporaryDirectory(prefix="proa-git-install-") as directory:
        work = Path(directory)
        cargo_home = work / "cargo-home"
        cargo_home.mkdir()
        if cache.is_dir():
            (cargo_home / "registry").symlink_to(cache.resolve(), target_is_directory=True)
        environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("CARGO_REGISTRIES_", "CARGO_REGISTRY_"))
        }
        environment.update(
            CARGO_HOME=str(cargo_home),
            CARGO_NET_GIT_FETCH_WITH_CLI="true",
            CARGO_PROFILE_DEV_DEBUG="0",
        )
        destination = work / "installed"
        subprocess.run(
            [
                "cargo", "install", "--git", ROOT.as_uri(), "--rev", revision,
                "--locked", "--debug", "--root", str(destination),
                "--target-dir", str(ROOT / "target" / "git-install"), "proa-cli",
            ],
            cwd=work, env=environment, check=True,
        )
        for binary in ("proa", "cargo-proa"):
            executable = destination / "bin" / (binary + (".exe" if os.name == "nt" else ""))
            subprocess.run([str(executable), "--help"], cwd=work, env=environment,
                           check=True, stdout=subprocess.DEVNULL)
        proa = destination / "bin" / ("proa.exe" if os.name == "nt" else "proa")
        for template in ("minimal", "origin"):
            project = work / f"smoke-{template}"
            subprocess.run(
                [str(proa), "new", "site", str(project), "--template", template,
                 "--yes", "--no-git", "--offline"],
                cwd=work, env=environment, check=True, stdout=subprocess.DEVNULL,
            )
            manifest = project / "Cargo.toml"
            # Test the exact candidate commit, even before the PR merges.
            def candidate_dependency(match: re.Match) -> str:
                name, value = match.groups()
                dependency = tomllib.loads(f"dependency = {value}")["dependency"]
                if isinstance(dependency, str):
                    dependency = f'version = "{dependency}"'
                else:
                    dependency = value.strip()[1:-1].strip()
                return f'{name} = {{ {dependency}, git = "{ROOT.as_uri()}", rev = "{revision}" }}'

            source = re.sub(
                r"^(proa_[a-z_]+|rsjs)\s*=\s*(.+)$",
                candidate_dependency,
                manifest.read_text(),
                flags=re.MULTILINE,
            )
            manifest.write_text(source)
            metadata = json.loads(subprocess.check_output(
                ["cargo", "metadata", "--format-version", "1"],
                cwd=project, env=environment, text=True,
            ))
            packages = {package["name"]: package for package in metadata["packages"]}
            for name in ("proa_core", "proa_macros", "proa_framework_axum"):
                assert packages[name]["source"].endswith(f"#{revision}"), name
            print(f"{template} scaffold resolves Proa from the candidate Git commit")
        print(f"Git install passed for {revision}: both CLI binaries run without a private registry")


if __name__ == "__main__":
    main()
