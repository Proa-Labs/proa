#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Install the released CLI and exercise generated sites using only crates.io."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def run(arguments: list[str], cwd: Path, environment: dict[str, str]) -> None:
    print("$ " + " ".join(arguments), flush=True)
    subprocess.run(arguments, cwd=cwd, env=environment, check=True)


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
            paths = ["/", "/healthz", "/readyz"]
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
    parser.add_argument("--version", help="specific released proa-cli version (default: latest stable)")
    parser.add_argument("--work-dir", type=Path, help="keep generated projects and logs here")
    arguments = parser.parse_args()
    work = arguments.work_dir or Path(tempfile.mkdtemp(prefix="proa-registry-install-"))
    work = work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    print(f"Registry acceptance artifacts: {work}", flush=True)
    cargo_home = work / "cargo-home"
    cargo_home.mkdir(exist_ok=True)
    # A fresh Cargo home has no checkout config, source overrides, or credentials.
    environment = {
        key: value for key, value in os.environ.items()
        if not key.startswith(("CARGO_", "PROA_", "RSJS_", "SCCACHE_"))
        and key not in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER")
    }
    environment.update(CARGO_HOME=str(cargo_home), CARGO_PROFILE_DEV_DEBUG="0",
                       CARGO_PROFILE_TEST_DEBUG="0", RUST_TEST_THREADS="2")
    destination = work / "installed"
    install = ["cargo", "install", "proa-cli", "--locked",
               "--root", str(destination), "--target-dir", str(work / "target")]
    if arguments.version:
        install += ["--version", "=" + arguments.version]
    run(install, work, environment)
    suffix = ".exe" if os.name == "nt" else ""
    environment["PATH"] = str(destination / "bin") + os.pathsep + environment.get("PATH", "")
    for name in ("proa", "cargo-proa"):
        run([str(destination / "bin" / (name + suffix)), "--help"], work, environment)
    run(["cargo", "proa", "--help"], work, environment)
    proa = destination / "bin" / ("proa" + suffix)
    for template in ("minimal", "app", "marketing", "docs", "origin"):
        project = work / f"registry-smoke-{template}"
        run([str(proa), "new", "site", str(project), "--template", template,
             "--yes", "--no-git", "--offline"], work, environment)
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1"], cwd=project,
            env=environment, text=True))
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
        assert tests, "generated site must contain integration tests"
        for target in tests:
            run([str(proa), "test", "--root", str(project), "--locked", "--test", target],
                project, environment)
        artifact = project / ("app" + suffix)
        run([str(proa), "build", "--root", str(project), "--locked", "--profile", "dev",
             "--artifact-out", str(artifact)], project, environment)
        smoke_http(artifact, project, environment, template)
    print("Registry install and linked-site acceptance passed", flush=True)


if __name__ == "__main__":
    main()
