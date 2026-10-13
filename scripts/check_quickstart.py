#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Exercise the documented Basic app edits with a locally built Proa CLI."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import tempfile
import textwrap
import time
import urllib.error
import urllib.request

from check_registry_install import run


ROOT = Path(__file__).resolve().parents[1]
STARTER_SELECTORS = (".topbar", ".hero", ".wordmark", ".metrics")


def fences(section: str, language: str) -> list[str]:
    return re.findall(r"```" + language + r"\n(.*?)\n```", section, re.DOTALL)


def apply_documented_edits(project: Path, guide: str) -> str:
    setup = guide.split("## 2. Create A Site\n", 1)[1].split("## 3. Run The Server", 1)[0]
    starter = project / "styles/starter.css"
    stylesheet = project / "styles/input.css"
    if not starter.exists() or './starter.css' not in stylesheet.read_text(encoding="utf-8"):
        if not starter.exists():
            (project / "public/styles.css").rename(starter)
        stylesheet.write_text(fences(setup, "css")[0] + "\n", encoding="utf-8")
    edit = guide.split("## 4. Edit The First Page\n", 1)[1].split("## 5. Add A Route", 1)[0]
    html = fences(edit, "rust")[0]
    markdown = fences(edit, "markdown")[0]
    home = project / "src/pages/home.rs"
    source = home.read_text(encoding="utf-8")
    pattern = r'(?m)^( +)<h1>.*?</h1>\s*<p class="lede">.*?</p>'
    source, count = re.subn(pattern, lambda match: textwrap.indent(html, match[1]),
                            source, count=1, flags=re.DOTALL)
    assert count == 1, "the documented hero edit must match the Basic app"
    pattern = r"(THE SERVER ALREADY DID THE WORK\n\n)( +).*?(\n\n +CLIENT)"
    source, count = re.subn(pattern, lambda match: match[1] + textwrap.indent(markdown, match[2])
                            + match[3], source, count=1, flags=re.DOTALL)
    assert count == 1, "the documented Markdown edit must match the Basic app"
    home.write_text(source, encoding="utf-8")
    route = guide.split("## 5. Add A Route\n", 1)[1].split("## 6.", 1)[0]
    about, modules, routes = fences(route, "rust")
    (project / "src/pages/about.rs").write_text(about + "\n", encoding="utf-8")
    (project / "src/pages/mod.rs").write_text(modules + "\n", encoding="utf-8")
    path = project / "src/routes.rs"
    source, count = re.subn(r"(?m)^( +)let \(router, metadata\) = router_from_spec\(vec!\[.*?^\1\]\);",
                            lambda match: textwrap.indent(routes, match[1]),
                            path.read_text(encoding="utf-8"), count=1, flags=re.DOTALL)
    assert count == 1, "the documented route table must match the Basic app"
    path.write_text(source, encoding="utf-8")
    return markdown


def verify_http(binary: Path, project: Path, environment: dict[str, str], copy: str) -> None:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    with (project / "server.log").open("w") as log:
        server = subprocess.Popen([str(binary)], cwd=project,
                                  env=environment | {"PROA_HOST": "127.0.0.1", "PORT": str(port)},
                                  stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 30
            while True:
                if server.poll() is not None:
                    raise RuntimeError(f"server exited; see {project / 'server.log'}")
                try:
                    with urllib.request.urlopen(base + "/healthz", timeout=2) as response:
                        assert response.status == 200
                    break
                except (urllib.error.URLError, TimeoutError):
                    if time.monotonic() >= deadline:
                        raise
                    time.sleep(0.1)
            for path, accept, content_type in [
                ("/", "text/html", "text/html"),
                ("/index.md", "text/markdown", "text/markdown"),
                ("/", "text/markdown", "text/markdown"),
                ("/about", "text/html", "text/html"),
                ("/readyz", "*/*", None),
                ("/static/styles.css", "text/css", "text/css"),
            ]:
                request = urllib.request.Request(base + path, headers={"Accept": accept})
                with urllib.request.urlopen(request, timeout=30) as response:
                    body = response.read().decode()
                    assert response.status == 200 and body, path
                    if content_type:
                        assert content_type in response.headers.get("Content-Type", ""), path
                    if path in ("/", "/index.md"):
                        assert all(line in body for line in copy.splitlines() if line), path
                    if path.endswith(".css"):
                        assert all(selector in body for selector in STARTER_SELECTORS), path
                    print(f"HTTP {path} ({accept}): {response.status}, {len(body)} characters", flush=True)
        finally:
            if server.poll() is None:
                server.send_signal(signal.SIGINT)
                try:
                    server.wait(timeout=35)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


def verify_content_lint(cli: Path, project: Path, environment: dict[str, str], copy: str) -> None:
    home = project / "src/pages/home.rs"
    original = home.read_text(encoding="utf-8")
    html, markdown = original.split("impl<L: DataLoader> MdRender<L> for HomePage", 1)
    heading = copy.splitlines()[0]
    markdown, count = re.subn(r"(?m)^ *" + re.escape(heading) + r"\n\n", "", markdown, count=1)
    assert count == 1
    try:
        home.write_text(html + "impl<L: DataLoader> MdRender<L> for HomePage"
                        + markdown, encoding="utf-8")
        result = subprocess.run([str(cli), "lint", "src", "--deny", "warnings"],
                                cwd=project, env=environment, capture_output=True, text=True, check=False)
        assert result.returncode != 0, "lint must reject a heading omitted from Markdown"
        assert "render/content-coverage" in result.stdout + result.stderr, result.stdout + result.stderr
        (project / "omission-lint.log").write_text(result.stdout + result.stderr, encoding="utf-8")
    finally:
        home.write_text(original, encoding="utf-8")
    print("Content lint correctly rejects a Markdown-only omission", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True, help="proa binary to verify")
    parser.add_argument("--work-dir", type=Path, help="keep generated project and logs here")
    parser.add_argument("--profile", default="dev", choices=("dev", "release"))
    parser.add_argument("--source-only", action="store_true",
                        help="check documented edits without compiling a generated app")
    arguments = parser.parse_args()
    cli = arguments.cli.resolve()
    work = (arguments.work_dir or Path(tempfile.mkdtemp(prefix="proa-quickstart-"))).resolve()
    work.mkdir(parents=True, exist_ok=True)
    project = work / "hello-proa"
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(("PROA_", "RSJS_", "SCCACHE_"))
                   and key not in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_TARGET_DIR")}
    environment.update(CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0", RUST_TEST_THREADS="2")
    environment["PATH"] = str(cli.parent) + os.pathsep + environment.get("PATH", "")
    run([str(cli), "--version"], work, environment)
    run([str(cli), "new", "site", str(project), "--yes", "--no-git", "--offline"], work, environment)
    copy = apply_documented_edits(project, (ROOT / "proa_marketing/src/docs/index.mdx").read_text(encoding="utf-8"))
    for command in (["fmt", "--check", "--verify", "src"], ["lint", "src", "--deny", "warnings"],
                    ["site", "check"]):
        run([str(cli), *command], project, environment)
    verify_content_lint(cli, project, environment, copy)
    if arguments.source_only:
        print(f"Quickstart source checks passed; artifacts: {work}", flush=True)
        return
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1"], cwd=project, env=environment, text=True))
    for package in metadata["packages"]:
        if package["name"].startswith(("proa_", "rsjs")):
            assert package["source"] == "registry+https://github.com/rust-lang/crates.io-index", package
    run([str(cli), "test", "--locked"], project, environment)
    artifact = work / ("hello-proa-app.exe" if os.name == "nt" else "hello-proa-app")
    run([str(cli), "build", "--locked", "--profile", arguments.profile,
         "--artifact-out", str(artifact)], project, environment)
    css = (project / "public/styles.css").read_text(encoding="utf-8")
    assert all(selector in css for selector in STARTER_SELECTORS), "Tailwind discarded the starter design"
    verify_http(artifact, project, environment, copy)
    print(f"Quickstart passed; artifacts: {work}", flush=True)


if __name__ == "__main__":
    main()
