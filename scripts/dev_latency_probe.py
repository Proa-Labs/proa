#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Measure save-to-HTTP-response latency in a warmed compile_time_probe fixture."""

import argparse
import json
import os
from pathlib import Path
import queue
import re
import shlex
import signal
import subprocess
import threading
import time
import tomllib
import urllib.error
import urllib.request


def response(url):
    try:
        with urllib.request.urlopen(url, timeout=0.2) as request:
            return request.read().decode()
    except (OSError, urllib.error.URLError):
        return ""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--source", default="src/page_0.rs", help="Fixture source containing EDIT_MARKER_A/B")
    parser.add_argument("--url-path", default="/page/0", help="HTTP route that exposes the edit marker")
    parser.add_argument("--profile", default="nodebug")
    parser.add_argument("--hot-static", action="store_true")
    parser.add_argument("--islands", action="store_true")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--debounce-ms", type=int, help="Override the CLI quiet window; omitted uses its default")
    parser.add_argument("--port", type=int, default=39147)
    parser.add_argument("--toolchain", help="Explicit rustup toolchain for Cargo and rustc")
    parser.add_argument("--rustflags", help="Explicit compiler flags; parsed as words, never executed")
    parser.add_argument("--configuration", default="default", help="Label and isolated target namespace")
    args = parser.parse_args()
    if args.samples < 1:
        parser.error("--samples must be positive")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", args.configuration):
        parser.error("--configuration must be a single alphanumeric label")
    root = args.fixture.resolve()
    package = tomllib.loads((root / "Cargo.toml").read_text())["package"]["name"]
    environment = dict(os.environ)
    for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_INCREMENTAL"):
        environment.pop(key, None)
    environment["CARGO_TARGET_DIR"] = str(root / "target" if args.configuration == "default" else root / "target" / args.configuration)
    if args.toolchain:
        environment["RUSTUP_TOOLCHAIN"] = args.toolchain
    if args.rustflags is not None:
        environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(shlex.split(args.rustflags))
    cargo = ["cargo", "run", "--offline", "--profile", args.profile]
    if args.islands:
        cargo += ["--features", "islands"]
    command = [str(args.cli.resolve()), "dev", "--root", str(root), "--no-assets", "--port", str(args.port),
               "--", *cargo]
    if args.debounce_ms is not None:
        command[2:2] = ["--debounce-ms", str(args.debounce_ms)]
    if args.hot_static:
        command.insert(2, "--hot-static")
    url = f"http://127.0.0.1:{args.port}{args.url_path}"
    if response(url):
        parser.error("The probe port is already serving HTTP; choose another --port")
    events = queue.Queue()
    process = subprocess.Popen(command, cwd=root, env=environment, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True, start_new_session=True)
    raw = []

    def collect(stream, kind):
        for line in stream:
            event = {"time": time.perf_counter(), "stream": kind, "line": line.rstrip()}
            raw.append(event)
            events.put(event)

    for stream, kind in ((process.stdout, "stdout"), (process.stderr, "stderr")):
        threading.Thread(target=collect, args=(stream, kind), daemon=True).start()
    page = root / args.source
    original = page.read_text()
    records = []
    log_dir = root / "target/probe-logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    name = f"watch-{args.configuration}-{args.profile}-{'islands' if args.islands else 'ssr'}-{args.debounce_ms if args.debounce_ms is not None else 'default'}"
    if args.hot_static:
        name += "-hot"
    try:
        deadline = time.perf_counter() + 180
        while not response(url):
            if process.poll() is not None or time.perf_counter() > deadline:
                raise RuntimeError("dev server did not start; inspect the recorded log")
            time.sleep(0.02)
        # Let startup logging drain; subsequent cycles are serialized by response.
        time.sleep(0.3)
        for sample in range(args.samples):
            while not events.empty():
                events.get_nowait()
            source = page.read_text()
            marker = "B" if "EDIT_MARKER_A" in source else "A"
            next_source = source.replace("EDIT_MARKER_A", f"EDIT_MARKER_{marker}") if marker == "B" else source.replace("EDIT_MARKER_B", "EDIT_MARKER_A")
            start = time.perf_counter()
            page.write_text(next_source)
            boundaries = {"save": 0.0}
            finishes = 0
            stage_timings_ms = {}
            deadline = start + 180
            while time.perf_counter() < deadline:
                while not events.empty():
                    event = events.get_nowait()
                    line = event["line"]
                    timestamp = event["time"] - start
                    timing = re.fullmatch(r"rsjs timing: (.*): ([0-9.]+) ms", line)
                    if timing:
                        stage_timings_ms[timing[1]] = float(timing[2])
                    if "static template change stabilized;" in line:
                        boundaries.setdefault("hot_published", timestamp)
                    elif "change stabilized; rebuilding" in line:
                        boundaries.setdefault("detected", timestamp)
                    elif "running linked RSJS Cargo dev build:" in line:
                        boundaries.setdefault("prepared", timestamp)
                    elif "Finished `" in line:
                        finishes += 1
                        boundaries.setdefault("descriptor_finished" if finishes == 1 else "final_finished", timestamp)
                    elif f"Compiling {package} " in line and finishes == 1:
                        boundaries.setdefault("final_compiling", timestamp)
                    elif "linked dev rebuild succeeded;" in line:
                        boundaries.setdefault("validated", timestamp)
                    elif "starting final linked server" in line:
                        boundaries.setdefault("launched", timestamp)
                if f"EDIT_MARKER_{marker}" in response(url):
                    boundaries["response"] = time.perf_counter() - start
                    break
                if process.poll() is not None:
                    raise RuntimeError("dev watcher exited during a rebuild")
                time.sleep(0.02)
            else:
                raise RuntimeError("updated HTTP response was not served within 180s")
            record = {"sample": sample, "boundaries": boundaries, "stage_timings_ms": stage_timings_ms}
            records.append(record)
            print(json.dumps(record), flush=True)
            time.sleep(0.3)
    finally:
        os.killpg(process.pid, signal.SIGTERM) if process.poll() is None else None
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        page.write_text(original)
        (log_dir / f"{name}.json").write_text(json.dumps({"command": command, "toolchain": args.toolchain,
            "rustflags": args.rustflags, "configuration": args.configuration, "build_timings": bool(environment.get("PROA_BUILD_TIMINGS")),
            "samples": records, "events": raw}, indent=2) + "\n")
    print(f"Results: {log_dir / (name + '.json')}")


if __name__ == "__main__":
    main()
