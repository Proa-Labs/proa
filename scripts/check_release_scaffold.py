#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Check release-plz propagates library releases into the CLI's scaffold catalog."""

from __future__ import annotations

import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def run(arguments: list[str], directory: Path, environment: dict[str, str]) -> str:
    result = subprocess.run(arguments, cwd=directory, env=environment,
                            capture_output=True, text=True, timeout=300)
    if result.returncode:
        raise RuntimeError(f"{' '.join(arguments)}\n{result.stdout}{result.stderr}")
    return result.stdout


def check(work: Path, message: str, expected: str, environment: dict[str, str]) -> None:
    project = work / "project"
    project.mkdir(parents=True)
    # Fixtures live outside the checkout, so preserve its compiler selection.
    shutil.copy2(ROOT / "rust-toolchain.toml", project / "rust-toolchain.toml")
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
    toml_version = workspace["workspace"]["dependencies"]["toml"]["version"]
    (project / "Cargo.toml").write_text('''[workspace]
members = ["proa_core", "proa_cli"]
resolver = "2"
[workspace.dependencies]
proa_core = { path = "proa_core", version = "1.0.0" }
''')
    for folder, name in [("proa_core", "proa_core"), ("proa_cli", "proa-cli")]:
        package = project / folder
        (package / "src").mkdir(parents=True)
        manifest = f'[package]\nname = "{name}"\nversion = "1.0.0"\nedition = "2021"\n'
        if folder == "proa_cli":
            manifest += (f'\n[build-dependencies]\ntoml = "{toml_version}"\n'
                         '\n[dev-dependencies]\nproa_core = { workspace = true }\n')
            shutil.copy2(ROOT / "proa_cli/build.rs", package / "build.rs")
            (package / "src/main.rs").write_text(
                'fn main() { print!("{}", include_str!(concat!(env!("OUT_DIR"), "/scaffold-versions.toml"))); }\n')
        else:
            (package / "src/lib.rs").write_text('pub fn library() {}\n')
        (package / "Cargo.toml").write_text(manifest)

    # Keep the real release policy for these packages. The fixture checks version
    # propagation from commits; API compatibility is checked on the real crates.
    sections = re.split(r"(?m)(?=^\[\[package\]\])", (ROOT / "release-plz.toml").read_text())
    selected = [section for section in sections[1:]
                if tomllib.loads(section)["package"][0]["name"] in {"proa_core", "proa-cli"}]
    config = sections[0] + "".join(selected)
    config = config.replace('name = "proa_core"', 'name = "proa_core"\nsemver_check = false')
    (project / "release-plz.toml").write_text(config)
    (project / ".gitignore").write_text('/target/\n')
    run(["cargo", "generate-lockfile"], project, environment)
    shutil.copytree(project, work / "registry")
    run(["git", "init", "--quiet"], project, environment)
    run(["git", "config", "user.name", "Proa release test"], project, environment)
    run(["git", "config", "user.email", "release-test@example.invalid"], project, environment)
    run(["git", "config", "commit.gpgsign", "false"], project, environment)
    run(["git", "remote", "add", "origin", "https://github.com/Proa-Labs/proa"], project, environment)
    run(["git", "add", "."], project, environment)
    run(["git", "commit", "--quiet", "-m", "initial release"], project, environment)
    for package in ["proa_core", "proa-cli"]:
        run(["git", "tag", f"{package}-v1.0.0"], project, environment)
    (project / "proa_core/src/lib.rs").write_text('pub fn library() {}\npub fn added() {}\n')
    run(["git", "add", "."], project, environment)
    run(["git", "commit", "--quiet", "-m", message], project, environment)
    run(["release-plz", "update", "--config", "release-plz.toml",
         "--registry-manifest-path", str(work / "registry/Cargo.toml")], project, environment)

    def manifest(path: str) -> dict:
        return tomllib.loads((project / path / "Cargo.toml").read_text())

    assert manifest("proa_core")["package"]["version"] == expected
    assert manifest("proa_cli")["package"]["version"] == "1.0.1", "CLI must bump for a scaffold-only dependency change"
    root = manifest("")
    assert root["workspace"]["dependencies"]["proa_core"]["version"] == expected
    catalog = tomllib.loads(run(["cargo", "run", "--quiet", "--locked", "-p", "proa-cli"], project, environment))
    assert catalog["proa_core"] == expected, "scaffold catalog must follow release-plz's dependency update"
    print(f"{message}: library {expected}, CLI 1.0.1, generated dependency {catalog['proa_core']}", flush=True)


def main() -> None:
    environment = {key: value for key, value in os.environ.items()
                   if key not in {"GIT_TOKEN", "GITHUB_TOKEN", "GH_TOKEN", "CARGO_REGISTRY_TOKEN", "CARGO_REGISTRIES_CRATES_IO_TOKEN"}}
    with tempfile.TemporaryDirectory(prefix="proa-release-scaffold-") as raw:
        work = Path(raw).resolve()
        environment["CARGO_TARGET_DIR"] = str(work / "target")
        for message, expected in [("fix: library repair", "1.0.1"), ("feat: library addition", "1.1.0")]:
            check(work / expected, message, expected, environment)


if __name__ == "__main__":
    main()
