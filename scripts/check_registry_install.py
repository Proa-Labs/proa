#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Exercise generated sites against crates.io with a released or candidate CLI."""

from __future__ import annotations

import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import tempfile
import time
from threading import Thread
import tomllib
import urllib.error
import urllib.request


def use_project_toolchain(environment: dict[str, str]) -> None:
    # Temporary projects live outside the checkout and cannot inherit its rustup pin.
    if not environment.get("RUSTUP_TOOLCHAIN"):
        pin = Path(__file__).resolve().parents[1] / "rust-toolchain.toml"
        with pin.open("rb") as manifest:
            environment["RUSTUP_TOOLCHAIN"] = tomllib.load(manifest)["toolchain"]["channel"]


def registry_environment(work: Path) -> dict[str, str]:
    # A fresh Cargo home has no checkout config, source overrides, or credentials.
    environment = {
        key: value for key, value in os.environ.items()
        if not key.startswith(("CARGO_", "PROA_", "RSJS_", "SCCACHE_"))
        and key not in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER")
    }
    use_project_toolchain(environment)
    environment.update(CARGO_HOME=tempfile.mkdtemp(prefix="cargo-home-", dir=work),
                       CARGO_TARGET_DIR=str(work / "target"), CARGO_PROFILE_DEV_DEBUG="0",
                       CARGO_PROFILE_TEST_DEBUG="0", RUST_TEST_THREADS="2")
    return environment


def run(arguments: list[str], cwd: Path, environment: dict[str, str]) -> None:
    print("$ " + " ".join(arguments), flush=True)
    subprocess.run(arguments, cwd=cwd, env=environment, check=True)


def registry_metadata(project: Path, environment: dict[str, str]) -> dict:
    command = ["cargo", "metadata", "--format-version", "1"]
    for attempt in range(3):
        print(f"$ cargo metadata --format-version 1 (attempt {attempt + 1}/3)", flush=True)
        result = subprocess.run(command, cwd=project, env=environment,
                                capture_output=True, text=True)
        if result.stderr:
            print(result.stderr, file=sys.stderr, end="", flush=True)
        if result.returncode == 0:
            return json.loads(result.stdout)
        pending_version = re.search(
            r"(?:failed to select a version for the requirement|no matching package named) "
            r"`(?:proa(?:[_-]|\b)|rsjs)[^`]*`",
            result.stderr,
        )
        if attempt == 2 or not pending_version:
            result.check_returncode()
        delay = 5 * (attempt + 1)
        print(f"Registry version may still be indexing; retrying resolution in {delay}s", flush=True)
        time.sleep(delay)


def generate_additions(proa: Path, project: Path, environment: dict[str, str], template: str) -> None:
    # Compile generated modules in the same facade-only application as the starter.
    # Exercise both render traits, Markdown, children, recipes, and layout shapes.
    commands = [
        ["component", "generated-notice", "--markdown"],
        ["component", "generated-async-notice", "--async", "--markdown"],
        ["component", "generated-panel", "--children", "--recipe", "--markdown"],
        ["component", "generated-async-panel", "--async", "--children", "--markdown"],
        ["layout", "generated-section", "--section"],
        ["layout", "generated-document", "--document", "--replace-root"],
    ]
    # ORIGIN uses its own Atlas page composition rather than src/pages routes.
    if template != "origin":
        commands.append(["page", "/generated"])
        if template != "minimal":
            commands.append(["page", "/generated-html", "--no-markdown"])
            commands.append(["page", "/guides/{slug}", "--markdown"])
    for command in commands:
        run([str(proa), "generate", *command, "--root", str(project)], project, environment)


FIRST_ISLAND = '''use proa::rsjs::{rsjs, signal};
use proa::{DataLoader, WebContext, WebRenderSync, WriteError};

#[derive(Debug, Clone, Copy)]
pub struct FirstIsland;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for FirstIsland {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <div data-first-island>
                <button type="button" on_click={|_| count.update(|c| c + 1)}>"+"</button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}
'''


def inject_first_island(project: Path) -> None:
    """Make the first interactive edit to the default application.

    A site that declared its modules in the binary passed every check until
    this edit, then could not build its integration tests.
    """
    (project / "src/components/first_island.rs").write_text(FIRST_ISLAND, encoding="utf-8")
    modules = project / "src/components/mod.rs"
    modules.write_text(modules.read_text(encoding="utf-8") + "pub mod first_island;\n", encoding="utf-8")
    island = "crate::components::first_island::FirstIsland"
    home = project / "src/pages/home.rs"
    # The home page is async, so the sync island is lifted into its template.
    source, rendered = re.subn(
        r"(?m)^( +)<main>\n", lambda main: f"{main[0]}{main[1]}    {{proa::to_async({island})}}\n",
        home.read_text(encoding="utf-8"), count=1)
    # The Markdown view has no counterpart for a browser-only control.
    source, omitted = re.subn(
        r"(?m)^( +)md! \{", lambda view: f"{view[1]}proa::omit!({island});\n{view[0]}", source, count=1)
    assert rendered == 1 and omitted == 1, "the island must fit the default home page"
    source = source.replace("title: TITLE.into(),", 'title: TITLE.into(), canonical: "/".into(),', 1)
    routes = project / "src/routes.rs"
    routing = routes.read_text(encoding="utf-8")
    routing = routing.replace("let app = FrameworkBuilder::new()", """let mount = if base_path.as_str().is_empty() { "/" } else { base_path.as_str() };
    let urls = proa::HeadUrlPolicy::new("https://site.example", mount).unwrap();
    let app = FrameworkBuilder::new()""", 1)
    routes.write_text(routing.replace(".with_head(proa::HeadConfig::default())",
                                      ".with_head(proa::HeadConfig::default().with_urls(urls))", 1), encoding="utf-8")
    home.write_text(source, encoding="utf-8")


def smoke_http(binary: Path, project: Path, environment: dict[str, str], template: str,
               cdn: str | None = None) -> None:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    server_environment = environment | {"PROA_HOST": "127.0.0.1", "PORT": str(port)}
    if cdn:
        server_environment.update(PROA_BASE_PATH="/sub", PROA_ASSET_PREFIX=cdn)
    base = f"http://127.0.0.1:{port}"
    with (project / "server.log").open("w") as log:
        server = subprocess.Popen([str(binary)], cwd=project, env=server_environment,
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
            if cdn:
                verify_mounted_site(base, cdn)
                return
            # The stylesheet's name carries a hash of its bytes, so the page is
            # the only place its URL can be read from.
            with urllib.request.urlopen(base + "/", timeout=60) as response:
                stylesheet = re.search(
                    rb'href="(/_proa/immutable/styles\.[0-9a-f]{16}\.css)"', response.read()
                )
                assert stylesheet, "the page links no hashed stylesheet"
            paths = ["/", "/healthz", "/readyz", stylesheet.group(1).decode()]
            if template != "origin":
                paths += ["/generated"]
                if template != "minimal":
                    paths += ["/generated.md", "/generated-html", "/guides/a%2Fb", "/guides/a%2Fb.md"]
            if template != "minimal":
                paths += ["/index.md"]
            if template == "origin":
                paths += ["/static/origin.js"]
            for path in paths:
                with urllib.request.urlopen(base + path, timeout=60) as response:
                    body = response.read()
                    assert response.status == 200 and body, path
                    if path == "/":
                        assert "text/html" in response.headers.get("Content-Type", "")
                        assert b"<html" in body and b"</html>" in body
                        if template == "app":
                            assert b"<rsjs-island" in body, "the injected island did not render"
                    if path == "/guides/a%2Fb":
                        assert "</guides/a%2Fb.md>" in response.headers.get("Link", "")
                    if path.endswith(".js"):
                        assert "javascript" in response.headers.get("Content-Type", "")
                    if path.endswith(".md"):
                        assert "text/markdown" in response.headers.get("Content-Type", "")
                    print(f"HTTP {path}: {response.status}, {len(body)} bytes", flush=True)
        finally:
            if server.poll() is None:
                server.send_signal(signal.SIGINT)
                try:
                    server.wait(timeout=35)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


def verify_mounted_site(origin: str, cdn: str) -> None:
    # The mounting reverse proxy strips /sub before forwarding to Axum.
    with urllib.request.urlopen(origin + "/", timeout=30) as page:
        html = page.read().decode()
        assert page.headers["Cache-Control"] == "private, no-store"
        assert "</sub/index.md>" in page.headers["Link"]
    assert 'rel="canonical" href="https://site.example/sub/"' in html
    with urllib.request.urlopen(origin + "/index.md", timeout=30) as markdown:
        assert markdown.headers["Cache-Control"] == "private, no-store"
    assets = set(re.findall(r'(?:src|href)="([^"]*/_proa/immutable/[^"]+)"', html))
    assert any(url.endswith(".css") for url in assets)
    assert any("/rsjs/" in url and url.endswith(".js") for url in assets)
    for url in sorted(assets):
        assert url.startswith(cdn + "/_proa/immutable/"), url
        asset_origin = origin + url.removeprefix(cdn)
        with urllib.request.urlopen(url, timeout=10) as external, urllib.request.urlopen(asset_origin, timeout=10) as local:
            assert external.read() == local.read(), url
            assert "immutable" in local.headers["Cache-Control"], url
            etag = local.headers["ETag"]
        conditional = urllib.request.Request(asset_origin, headers={"If-None-Match": etag})
        try:
            with urllib.request.urlopen(conditional, timeout=10) as response:
                raise AssertionError(f"{url}: expected 304, got {response.status}")
        except urllib.error.HTTPError as error:
            with error:
                assert error.code == 304, (url, error.code)
    print(f"Mounted HTML and Markdown, canonical URL, and {len(assets)} CDN assets verified", flush=True)


def smoke_cdn(binary: Path, project: Path, environment: dict[str, str]) -> None:
    directory = Path(str(binary) + ".assets") / "public"
    with ThreadingHTTPServer(("127.0.0.1", 0), partial(SimpleHTTPRequestHandler, directory=str(directory))) as server:
        thread = Thread(target=server.serve_forever)
        thread.start()
        try:
            smoke_http(binary, project, environment, "app", f"http://127.0.0.1:{server.server_port}")
        finally:
            server.shutdown()
            thread.join()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group()
    source.add_argument("--version", help="specific released proa-cli version (default: latest stable)")
    source.add_argument("--cli", type=Path, help="verify a candidate CLI instead of installing one")
    parser.add_argument("--template", action="append", choices=("minimal", "app", "marketing", "docs", "origin"),
                        help="template to verify (repeatable; default: all)")
    parser.add_argument("--work-dir", type=Path, help="keep generated projects and logs here")
    arguments = parser.parse_args()
    work = arguments.work_dir or Path(tempfile.mkdtemp(prefix="proa-registry-install-"))
    work = work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    print(f"Registry acceptance artifacts: {work}", flush=True)
    environment = registry_environment(work)
    print(f"Registry acceptance Rust toolchain: {environment['RUSTUP_TOOLCHAIN']}", flush=True)
    if arguments.cli:
        proa = arguments.cli.resolve()
    else:
        destination = work / "installed"
        install = ["cargo", "install", "proa-cli", "--locked",
                   "--root", str(destination), "--target-dir", str(work / "target")]
        if arguments.version:
            install += ["--version", "=" + arguments.version]
        run(install, work, environment)
        proa = destination / "bin" / ("proa.exe" if os.name == "nt" else "proa")
    environment["PATH"] = str(proa.parent) + os.pathsep + environment.get("PATH", "")
    for name in ("proa", "cargo-proa"):
        run([str(proa.with_name(name + (".exe" if os.name == "nt" else ""))), "--help"], work, environment)
    run(["cargo", "proa", "--help"], work, environment)
    templates = arguments.template or ("minimal", "app", "marketing", "docs", "origin")
    for template in templates:
        project = work / f"registry-smoke-{template}"
        run([str(proa), "new", "site", str(project), "--template", template,
             "--yes", "--no-git", "--offline"], work, environment)
        generate_additions(proa, project, environment, template)
        if template == "app":
            inject_first_island(project)
        metadata = registry_metadata(project, environment)
        proa_packages = [package for package in metadata["packages"]
                         if package["name"].startswith(("proa_", "rsjs"))
                         or package["name"] in ("proa", "itoa_lut")]
        assert proa_packages, "scaffold must resolve Proa libraries"
        for package in proa_packages:
            assert package["source"] == "registry+https://github.com/rust-lang/crates.io-index", package
        print(f"{template}: {len(proa_packages)} Proa dependencies resolved from crates.io", flush=True)
        application = next(package for package in metadata["packages"]
                           if Path(package["manifest_path"]).resolve() == (project / "Cargo.toml").resolve())
        if template == "app":
            run([str(proa), "check", "--root", str(project), "--locked"], project, environment)
        tests = [target["name"] for target in application["targets"] if "test" in target["kind"]]
        if any("lib" in target["kind"] for target in application["targets"]):
            run(["cargo", "test", "--locked", "--lib"], project, environment)
        if template == "app":
            run([str(proa), "test", "--root", str(project), "--locked"], project, environment)
        else:
            for target in tests:
                run([str(proa), "test", "--root", str(project), "--locked", "--test", target],
                    project, environment)
        artifact = work / (template + (".exe" if os.name == "nt" else ""))
        run([str(proa), "build", "--root", str(project), "--locked",
             "--profile", "release" if template == "app" else "dev", "--artifact-out", str(artifact)], project, environment)
        smoke_http(artifact, project, environment, template)
        if template == "app":
            smoke_cdn(artifact, project, environment)
    print("Registry install and linked-site acceptance passed", flush=True)


if __name__ == "__main__":
    main()
