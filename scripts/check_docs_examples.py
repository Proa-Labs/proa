#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Compile the source printed in checked MDX fences. Link browser examples through the candidate CLI."""

import argparse
from collections import defaultdict
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

from check_registry_install import use_project_toolchain

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "proa_marketing/src/docs"
FENCE = re.compile(r'^```(rust|toml) (?=[^\n]*\bcheck="(\w+(?:/\w+)*)")[^\n]*\n(.*?)^```[ \t]*$', re.M | re.S)


CODE = re.compile(r'^```(\w+)([^\n]*)\n(.*?)^```[ \t]*$', re.M | re.S)


def check_corpus(project, env):
    project.mkdir()
    checker = ROOT / "scripts/check_docs_snippet_syntax.rs"
    (project / "Cargo.toml").write_text(
        '[workspace]\n[package]\nname="doc-syntax"\nversion="0.0.0"\nedition="2024"\n'
        + '[[bin]]\nname="doc-syntax"\npath=' + json.dumps(str(checker))
        + '\n[dependencies]\nsyn={version="2", features=["full"]}\n'
    )
    versions = {name: tomllib.loads((ROOT / directory / "Cargo.toml").read_text())["package"]["version"]
                for name, directory in (("proa", "proa"), ("rsjs", "rsjs"), ("proa-cli", "proa_cli"))}
    snippets = project / "snippets"
    snippets.mkdir()
    count = 0
    for page in sorted(DOCS.rglob("*.mdx")):
        content = page.read_text()
        for block in CODE.finditer(content):
            language, _, body = block.groups()
            line = content.count("\n", 0, block.start()) + 1
            location = f"{page.relative_to(DOCS)}:{line}"
            if language == "toml":
                for name, version in re.findall(
                    r'^\s*(proa|rsjs|proa-cli)\s*=\s*(?:\{\s*version\s*=\s*)?"=?([0-9.]+)"', body, re.M
                ):
                    if versions[name].split(".")[:len(version.split("."))] != version.split("."):
                        raise ValueError(f"{location}: {name} {version} is stale (candidate {versions[name]})")
            if language != "rust":
                continue
            if re.search(r'FrameworkBuilder::[^;{}]*?\.build\s*\(', body, re.S):
                raise ValueError(f"{location}: use FrameworkBuilder::try_build()")
            if "DISCOVERY_LINKS" in body:
                raise ValueError(f"{location}: the internal DISCOVERY_LINKS constant is unavailable to applications")
            source = snippets / ("-".join(page.relative_to(DOCS).with_suffix("").parts) + f"-L{line}.rs")
            source.write_text(body)
            count += 1
    for name in ("nested-layouts.svg", "nested-layouts-content.svg", "page-and-route.svg"):
        diagram = (ROOT / "proa_marketing/public/images/docs" / name).read_text()
        if ">app.rs<" in diagram or ">routes.rs<" not in diagram:
            raise ValueError(f"{name} must show the starter route module")
    print(f"Checking Rust syntax in {count} documented fences", flush=True)
    subprocess.run(["cargo", "run", "--quiet", "--", snippets], cwd=project, env=env, check=True)


def check(project, group, modules, dependencies, cli, env):
    source = project / "src"
    source.mkdir(parents=True)
    children = defaultdict(set)
    for module, blocks in modules.items():
        parts = module.split("/")
        path = source.joinpath(*parts).with_suffix(".rs")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("\n".join(blocks))
        for index, part in enumerate(parts):
            children[tuple(parts[:index])].add(part)
    for parent, names in children.items():
        declarations = "".join(f"pub(crate) mod {name};\n" for name in sorted(names))
        if parent:
            source.joinpath(*parent, "mod.rs").write_text(declarations)
        else:
            (source / "main.rs").write_text("#![allow(dead_code)]\n" + declarations + "fn main() {}\n")
    (project / "Cargo.toml").write_text(
        f'[workspace]\n[package]\nname = "{group}"\nversion = "0.0.0"\nedition = "2024"\n'
        + "[dependencies]\n" + dependencies
    )
    if group == "doc-islands":
        (project / "proa.config.rs").write_text(
            'use proa::config::{ApplicationConfig, Config, Result};\n'
            'pub async fn config() -> Result<Config> { Ok(Config { application: Some('
            'ApplicationConfig { binary: "doc-islands", ..Default::default() }), ..Config::default() }) }\n'
        )
        command = [str(cli), "build", "--no-check", "--profile", "dev"]
    else:
        command = ["cargo", "check"]
    print(f"{group}: compiling {', '.join(sorted(modules))}", flush=True)
    subprocess.run(command, cwd=project, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    args = parser.parse_args()
    groups = {name: defaultdict(list) for name in ("doc-islands", "doc-services")}
    dependencies = []
    for page in sorted(DOCS.rglob("*.mdx")):
        for language, name, body in FENCE.findall(page.read_text()):
            if language == "toml":
                if name != "dependencies" or not body.startswith("[dependencies]\n"):
                    raise ValueError(f"expected a dependencies table in {page}")
                dependencies.append(body.removeprefix("[dependencies]\n"))
            else:
                group = "doc-services" if page.stem in {"authentication", "transactional-email", "observability", "billing"} else "doc-islands"
                groups[group][name].append(body)
    # Renaming a fence must not silently remove the regression from CI.
    for group, expected in [("doc-islands", {"workbench", "shipping", "components/stock_tile", "cart", "queue", "search_preview"}),
                            ("doc-services", {"sessions", "mail", "billing", "telemetry"})]:
        if missing := expected.difference(groups[group]):
            raise ValueError(f"{group} missing checked examples: {sorted(missing)}")

    libraries = "".join(f'{name} = {{ path = {json.dumps(str(ROOT / name))} }}\n'
                        for name in ("proa", "rsjs"))
    libraries += 'serde = { version = "1", features = ["derive"] }\n'
    env = os.environ.copy()
    use_project_toolchain(env)
    with tempfile.TemporaryDirectory(prefix="proa-doc-examples-") as tmp:
        check_corpus(Path(tmp) / "syntax", env)
        check(Path(tmp) / "islands", "doc-islands", groups["doc-islands"], libraries, args.cli.resolve(), env)
        check(Path(tmp) / "services", "doc-services", groups["doc-services"],
              'axum = "0.8"\n' + "\n".join(dependencies), args.cli.resolve(), env)


if __name__ == "__main__":
    main()
