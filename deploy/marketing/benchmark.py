#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Measure cold, no-op, and one-link rebuilds plus runtime SSR on native Linux."""

import argparse
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

from build import build


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
