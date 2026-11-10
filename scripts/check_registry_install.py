#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Exercise generated sites against crates.io with a released or candidate CLI."""

from __future__ import annotations

import argparse
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
import urllib.error
import urllib.request


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
            r"`(?:proa[_-]|rsjs)[^`]*`",
            result.stderr,
        )
        if attempt == 2 or not pending_version:
            result.check_returncode()
        delay = 5 * (attempt + 1)
        print(f"Registry version may still be indexing; retrying resolution in {delay}s", flush=True)
        time.sleep(delay)


def smoke_http(binary: Path, project: Path, environment: dict[str, str], template: str) -> None:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    server_environment = environment | {"PROA_HOST": "127.0.0.1", "PORT": str(port)}
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
            paths = ["/", "/healthz", "/readyz", "/static/styles.css"]
            if template != "minimal":
                paths += ["/index.md"]
            if template == "origin":
                paths += ["/rsjs-runtime.js", "/rsjs-runtime-query.js", "/static/origin.js"]
            for path in paths:
                with urllib.request.urlopen(base + path, timeout=60) as response:
                    body = response.read()
                    assert response.status == 200 and body, path
                    if path == "/":
                        assert "text/html" in response.headers.get("Content-Type", "")
                        assert b"<html" in body and b"</html>" in body
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
    cargo_home = Path(tempfile.mkdtemp(prefix="cargo-home-", dir=work))
    # A fresh Cargo home has no checkout config, source overrides, or credentials.
    environment = {
        key: value for key, value in os.environ.items()
        if not key.startswith(("CARGO_", "PROA_", "RSJS_", "SCCACHE_"))
        and key not in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER")
    }
    environment.update(CARGO_HOME=str(cargo_home), CARGO_PROFILE_DEV_DEBUG="0",
                       CARGO_PROFILE_TEST_DEBUG="0", RUST_TEST_THREADS="2")
    environment["CARGO_TARGET_DIR"] = str(work / "target")
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
        metadata = registry_metadata(project, environment)
        proa_packages = [package for package in metadata["packages"]
                         if package["name"].startswith(("proa_", "rsjs"))
                         or package["name"] == "itoa_lut"]
        assert proa_packages, "scaffold must resolve Proa libraries"
        for package in proa_packages:
            assert package["source"] == "registry+https://github.com/rust-lang/crates.io-index", package
        print(f"{template}: {len(proa_packages)} Proa dependencies resolved from crates.io", flush=True)
        application = next(package for package in metadata["packages"]
                           if Path(package["manifest_path"]).resolve() == (project / "Cargo.toml").resolve())
        tests = [target["name"] for target in application["targets"] if "test" in target["kind"]]
        if any("lib" in target["kind"] for target in application["targets"]):
            run(["cargo", "test", "--locked", "--lib"], project, environment)
        for target in tests:
            run([str(proa), "test", "--root", str(project), "--locked", "--test", target],
                project, environment)
        artifact = work / (template + (".exe" if os.name == "nt" else ""))
        run([str(proa), "build", "--root", str(project), "--locked", "--profile", "dev",
             "--artifact-out", str(artifact)], project, environment)
        smoke_http(artifact, project, environment, template)
    print("Registry install and linked-site acceptance passed", flush=True)


if __name__ == "__main__":
    main()
