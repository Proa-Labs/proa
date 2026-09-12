#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Discover unittest cases once and run balanced shards in child processes."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys
import unittest
from collections.abc import Iterable


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--start-directory", required=True)
    parser.add_argument("--pattern", default="test*.py")
    parser.add_argument("--top-level-directory")
    parser.add_argument("--jobs", type=int, default=min(os.cpu_count() or 1, 4))
    return parser.parse_args()


def iter_tests(suite: unittest.TestSuite) -> Iterable[unittest.TestCase]:
    for item in suite:
        if isinstance(item, unittest.TestSuite):
            yield from iter_tests(item)
        else:
            yield item


def discover_test_ids(args: argparse.Namespace) -> list[str]:
    repository_root = str(Path.cwd().resolve())
    if repository_root not in sys.path:
        sys.path.insert(0, repository_root)
    loader = unittest.TestLoader()
    suite = loader.discover(
        start_dir=args.start_directory,
        pattern=args.pattern,
        top_level_dir=args.top_level_directory,
    )
    if loader.errors:
        raise RuntimeError("\n\n".join(loader.errors))
    identifiers = sorted(test.id() for test in iter_tests(suite))
    if len(identifiers) != len(set(identifiers)):
        raise RuntimeError("unittest discovery returned duplicate test identifiers")
    return identifiers


def partition(items: list[str], jobs: int) -> list[list[str]]:
    shard_count = min(max(jobs, 1), len(items))
    shards = [[] for _ in range(shard_count)]
    for index, item in enumerate(items):
        shards[index % shard_count].append(item)
    return shards


def child_environment(start_directory: str) -> dict[str, str]:
    environment = os.environ.copy()
    import_root = str(Path(start_directory).resolve())
    repository_root = str(Path.cwd().resolve())
    existing = environment.get("PYTHONPATH")
    python_paths = [repository_root, import_root]
    if existing:
        python_paths.append(existing)
    environment["PYTHONPATH"] = os.pathsep.join(python_paths)
    return environment


def run_shards(shards: list[list[str]], environment: dict[str, str]) -> int:
    processes: list[tuple[int, subprocess.Popen[bytes]]] = []
    for index, identifiers in enumerate(shards, start=1):
        print(
            f"starting unittest shard {index}/{len(shards)} "
            f"with {len(identifiers)} tests",
            flush=True,
        )
        process = subprocess.Popen(
            [sys.executable, "-m", "unittest", *identifiers],
            env=environment,
        )
        processes.append((index, process))

    failed = False
    try:
        for index, process in processes:
            return_code = process.wait()
            print(f"unittest shard {index} exited with {return_code}", flush=True)
            failed |= return_code != 0
    except BaseException:
        for _, process in processes:
            if process.poll() is None:
                process.terminate()
        for _, process in processes:
            process.wait()
        raise
    return 1 if failed else 0


def main() -> int:
    args = parse_args()
    if args.jobs < 1:
        raise ValueError("--jobs must be at least one")
    identifiers = discover_test_ids(args)
    if not identifiers:
        raise RuntimeError("unittest discovery found no tests")
    shards = partition(identifiers, args.jobs)
    print(
        f"discovered {len(identifiers)} tests across {len(shards)} shards",
        flush=True,
    )
    return run_shards(shards, child_environment(args.start_directory))


if __name__ == "__main__":
    raise SystemExit(main())
