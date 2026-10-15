#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Measure cold, no-op, and one-link rebuilds plus runtime SSR on native Linux."""

import argparse
import datetime
import hashlib
import http.client
import json
import os
import re
from pathlib import Path
import shutil
import statistics
import subprocess
import time
import urllib.request

def run(command, *, cwd, env, report, label):
    log = report / f"{label}.log"
    resources = report / f"{label}.resources"
    started = time.monotonic()
    with log.open("w") as output:
        process = subprocess.Popen(
            ["/usr/bin/time", "-v", "-o", str(resources), *command],
            cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, bufsize=1,
        )
        for line in process.stdout:
            output.write(line)
            print(f"{datetime.datetime.now(datetime.timezone.utc).isoformat()} {line}", end="", flush=True)
        code = process.wait()
    result = {"stage": label, "seconds": round(time.monotonic() - started, 3), "exit_code": code}
    with (report / "stages.jsonl").open("a") as output:
        output.write(json.dumps(result) + "\n")
    if code:
        raise subprocess.CalledProcessError(code, command)


def build(incoming, cache, output, report, units, lto, label):
    source = cache / "source"
    target = cache / "target"
    for directory in (output, report, target):
        directory.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    for name in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"):
        env.pop(name, None)
    env.update(CARGO_TARGET_DIR=str(target), CARGO_HOME=str(cache / "cargo"), CARGO_TERM_COLOR="never")
    env.update(TAILWIND_BIN="/usr/local/bin/tailwindcss", PROA_MARKETING_BUILD_CSS_ONLY="1")
    run(["bash", str(incoming / "scripts/watch-marketing.sh")], cwd=incoming, env=env,
        report=report, label=f"{label}-css")
    run(["python3", str(incoming / "scripts/sync_build_context.py"), str(incoming), str(source)],
        cwd=incoming, env=env, report=report, label=f"{label}-sync")
    # The build driver is a tool, not a published benchmark binary. Its fast
    # profile is independent of the fully optimized application profile.
    run(["cargo", "build", "--profile", "release-fast", "--locked", "-p", "proa-cli", "--bin", "proa"],
        cwd=source, env=env, report=report, label=f"{label}-cli")
    env.update(CARGO_PROFILE_RELEASE_OPT_LEVEL="3", CARGO_PROFILE_RELEASE_CODEGEN_UNITS=str(units),
               CARGO_PROFILE_RELEASE_LTO=lto)
    env["CARGO_LOG"] = "cargo::core::compiler::fingerprint=info"
    run([str(target / "release-fast/proa"), "build", "--root", str(source),
         "--package", "proa_marketing", "--bin", "proa_marketing", "--locked", "--no-check",
         "--no-assets", "--artifact-out", str(output / "proa_marketing"), "--", "cargo", "build",
         "--profile", "release", "--timings"],
        cwd=source, env=env, report=report, label=f"{label}-linked")
    for name, origin in (("static", source / "proa_marketing/public"),
                         ("showcase", source / "proa_showcase_web/public")):
        shutil.copytree(origin, output / name, dirs_exist_ok=True)
    shutil.copytree(source / "proa_showcase/src/espn/data/images", output / "static/images", dirs_exist_ok=True)
    for index, timing in enumerate(target.rglob("cargo-timing.html")):
        shutil.copyfile(timing, report / f"{label}-cargo-timing-{index}.html")
    with (output / "proa_marketing").open("rb") as binary:
        binary_sha256 = hashlib.file_digest(binary, "sha256").hexdigest()
    metadata = {
        "codegen_units": units, "lto": lto, "opt_level": 3,
        "descriptor_wrapper_sha256": hashlib.sha256((source / "proa_cli/src/rsjs/build.rs").read_bytes()).hexdigest(),
        "rustc": subprocess.check_output(["rustc", "-vV"], text=True),
        "cpus": os.cpu_count(),
        "cpu_model": next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
                           if line.startswith("model name")), "unknown"),
        "binary_bytes": (output / "proa_marketing").stat().st_size,
        "binary_sha256": binary_sha256,
    }
    (report / f"{label}-build.json").write_text(json.dumps(metadata, indent=2) + "\n")



PAGES = ("ebay-srp", "nike-catalog", "nike-pdp", "espn-gamecast", "github-pr-diff")


def runtime(binary, artifacts, report):
    env = os.environ | {
        "PORT": "5050", "HOST": "127.0.0.1", "PROA_COMPANIONS": "0", "RUST_LOG": "error",
        "STATIC_DIR": str(artifacts / "static"), "SHOWCASE_PUBLIC_DIR": str(artifacts / "showcase"),
    }
    with (report / "server.log").open("w") as log:
        process = subprocess.Popen([str(binary)], env=env, cwd=artifacts, stdout=log, stderr=log)
        try:
            for _ in range(300):
                if process.poll() is not None:
                    raise RuntimeError("release server exited; see server.log")
                try:
                    with urllib.request.urlopen("http://127.0.0.1:5050/healthz", timeout=1):
                        break
                except OSError:
                    time.sleep(0.1)
            else:
                raise RuntimeError("release server did not become healthy")
            results = {}
            connection = http.client.HTTPConnection("127.0.0.1", 5050, timeout=30)
            for page in PAGES:
                samples = []
                body = json.dumps({"page": page})
                for index in range(1100):
                    connection.request("POST", "/api/render", body,
                                       {"Content-Type": "application/json", "x-proa-silent": "1"})
                    response = connection.getresponse()
                    rendered = response.read()
                    if response.status != 200 or len(rendered) < 100:
                        raise RuntimeError(f"{page}: HTTP {response.status}: {rendered[:100]}")
                    value = float(response.getheader("x-render-us"))
                    if value <= 0:
                        raise RuntimeError(f"{page}: renderer did not report positive CPU time")
                    if index >= 100:
                        samples.append(value)
                results[page] = {"median_ssr_us": statistics.median(samples),
                                 "p95_ssr_us": sorted(samples)[949], "samples": len(samples)}
            connection.close()
            (report / "runtime.json").write_text(json.dumps(results, indent=2) + "\n")
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--variants", nargs="+", default=["1:thin", "16:thin", "8:thin", "16:false", "16:off"])
    args = parser.parse_args()
    incoming = Path("/workspace")
    original = incoming / "proa_marketing/src/pages/home.rs"
    contents = original.read_bytes()
    # Change exactly one authored href, keeping the rest of the page intact.
    pattern = rb'(href=")([^"\r\n]+)("\s+data-video-cta="intro")'
    def change_link(match):
        separator = b"&" if b"?" in match[2] else b"?"
        return match[1] + match[2] + separator + b"release-benchmark=1" + match[3]
    changed, count = re.subn(pattern, change_link, contents)
    if count != 1:
        raise RuntimeError("expected exactly one intro video CTA for the benchmark edit")
    for variant in args.variants:
        units, lto = variant.split(":")
        root = Path("/cache") / f"cgu{units}-{lto}"
        report = Path("/release/report") / root.name
        output = Path("/release/artifacts") / root.name
        original.write_bytes(contents)
        try:
            for label in ("cold", "noop", "link-change"):
                if label == "link-change":
                    original.write_bytes(changed)
                build(incoming, root, output, report, int(units), lto, label)
            runtime(output / "proa_marketing", output, report)
        finally:
            original.write_bytes(contents)
        # Retain measurements and binaries for later A/B browser/runtime checks.
        # Drop target caches between variants to fit the worker disk.
        shutil.rmtree(root)
