#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Compile each service guide using only the code and dependencies printed on its page."""

import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[1]
GUIDES = ROOT / "proa_marketing/src/docs/guides"
EXAMPLES = {"authentication": "sessions", "billing": "billing", "observability": "telemetry",
            "transactional-email": "mail"}
FENCE = re.compile(r'^```(rust|toml) [^\n]*?\bcheck="service/(\w+)"[^\n]*\n(.*?)^```[ \t]*$', re.M | re.S)


def write_example(workspace, guide, module):
    page = GUIDES / f"{guide}.mdx"
    blocks = FENCE.findall(page.read_text(encoding="utf-8"))
    manifests = [body for lang, key, body in blocks if (lang, key) == ("toml", "dependencies")]
    sources = [body for lang, key, body in blocks if (lang, key) == ("rust", module)]
    if len(manifests) != 1 or not sources or len(blocks) != 1 + len(sources):
        raise ValueError(f"{page}: expected one dependency table and checked Rust for {module}")
    manifest = manifests[0]
    if set(tomllib.loads(manifest)) != {"dependencies"}:
        raise ValueError(f"{page}: expected [dependencies]")
    crate = workspace / guide
    (crate / "src").mkdir(parents=True)
    (crate / "src/lib.rs").write_text("\n".join(sources), encoding="utf-8")
    (crate / "Cargo.toml").write_text(
        f'[package]\nname="guide-{guide}"\nversion="0.0.0"\nedition="2024"\n' + manifest,
        encoding="utf-8")


def main():
    # The checkout's rust-toolchain.toml also applies to these generated crates.
    scratch = ROOT / "target"
    scratch.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=scratch, prefix="service-guides-") as directory:
        workspace = Path(directory)
        for guide, module in EXAMPLES.items():
            write_example(workspace, guide, module)
        (workspace / "Cargo.toml").write_text(
            '[workspace]\nresolver="3"\nmembers=' + json.dumps(list(EXAMPLES)) + '\n', encoding="utf-8")
        target = Path(os.environ.get("CARGO_TARGET_DIR", scratch)).resolve()
        subprocess.run(["cargo", "check", "--workspace", "--target-dir", str(target)], cwd=workspace, check=True)


if __name__ == "__main__":
    main()
