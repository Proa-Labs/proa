#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Build the linked Linux release and retain real stage/resource measurements."""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


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
    env.update(CARGO_PROFILE_RELEASE_CODEGEN_UNITS=str(units), CARGO_PROFILE_RELEASE_LTO=lto)
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
        "cpus": os.cpu_count(), "binary_bytes": (output / "proa_marketing").stat().st_size,
        "binary_sha256": binary_sha256,
    }
    (report / f"{label}-build.json").write_text(json.dumps(metadata, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--incoming", type=Path, default=Path("/workspace"))
    parser.add_argument("--cache", type=Path, default=Path("/cache"))
    parser.add_argument("--output", type=Path, default=Path("/release/artifacts"))
    parser.add_argument("--report", type=Path, default=Path("/release/report"))
    parser.add_argument("--units", type=int, choices=(1, 8, 16), default=16)
    parser.add_argument("--lto", choices=("thin", "off"), default="thin")
    parser.add_argument("--label", default="release")
    args = parser.parse_args()
    build(args.incoming.resolve(), args.cache.resolve(), args.output.resolve(), args.report.resolve(),
          args.units, args.lto, args.label)
