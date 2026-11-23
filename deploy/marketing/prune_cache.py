#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Prune stale RSJS scopes and superseded Cargo units from a build cache."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import os
from pathlib import Path
import re
import shutil
import stat
import sys


DEFAULT_MAX_BYTES = 32 * 1024 * 1024 * 1024  # 32 GiB uncompressed
DEFAULT_KEEP_METADATA = 4
HASH_CHUNK_SIZE = 1024 * 1024

UNIT_DIR_RE = re.compile(r"^(?P<pkg>.+)-(?P<hash>[0-9a-f]{16})$")
DEPS_HASH_RE = re.compile(r"-(?P<hash>[0-9a-f]{16})(?:\..+)?$")

PROA_RSJS_STRUCTURAL_DIRS = frozenset(
    {"shared", "final-target", "plans", "wrapper-snapshots-v1", "s", "f", "p"}
)


class PruneError(RuntimeError):
    pass


@dataclass
class PruneStats:
    bytes_before: int = 0
    bytes_after: int = 0
    removed_entries: int = 0
    superseded_units: int = 0
    orphan_entries: int = 0
    budget_evicted_units: int = 0
    active_scope: str | None = None

    @property
    def freed_bytes(self) -> int:
        return max(0, self.bytes_before - self.bytes_after)


@dataclass
class CargoUnit:
    profile_dir: Path
    pkg: str
    hash_hex: str
    kind_key: tuple[str, ...]
    fingerprint_dir: Path
    build_dir: Path | None
    deps_files: list[Path]
    mtime_ns: int
    size_bytes: int


def entry_size_bytes(path: Path) -> int:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return 0
    if not stat.S_ISDIR(metadata.st_mode):
        return metadata.st_size
    total = 0
    stack = [path]
    while stack:
        current = stack.pop()
        try:
            with os.scandir(current) as entries:
                for entry in entries:
                    try:
                        entry_meta = entry.stat(follow_symlinks=False)
                    except FileNotFoundError:
                        continue
                    if stat.S_ISDIR(entry_meta.st_mode):
                        stack.append(Path(entry.path))
                    else:
                        total += entry_meta.st_size
        except FileNotFoundError:
            continue
    return total


def tree_max_mtime_ns(path: Path) -> int:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return 0
    if not stat.S_ISDIR(metadata.st_mode):
        return metadata.st_mtime_ns
    newest_file: int | None = None
    newest_dir = metadata.st_mtime_ns
    stack = [path]
    while stack:
        current = stack.pop()
        try:
            with os.scandir(current) as entries:
                for entry in entries:
                    try:
                        entry_meta = entry.stat(follow_symlinks=False)
                    except FileNotFoundError:
                        continue
                    if stat.S_ISDIR(entry_meta.st_mode):
                        if entry_meta.st_mtime_ns > newest_dir:
                            newest_dir = entry_meta.st_mtime_ns
                        stack.append(Path(entry.path))
                    else:
                        if newest_file is None or entry_meta.st_mtime_ns > newest_file:
                            newest_file = entry_meta.st_mtime_ns
        except FileNotFoundError:
            continue
    return newest_file if newest_file is not None else newest_dir


def shallow_max_mtime_ns(path: Path) -> int:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return 0
    if not stat.S_ISDIR(metadata.st_mode):
        return metadata.st_mtime_ns
    newest_file: int | None = None
    newest_dir = metadata.st_mtime_ns
    try:
        with os.scandir(path) as entries:
            for entry in entries:
                try:
                    entry_meta = entry.stat(follow_symlinks=False)
                except FileNotFoundError:
                    continue
                if stat.S_ISDIR(entry_meta.st_mode):
                    if entry_meta.st_mtime_ns > newest_dir:
                        newest_dir = entry_meta.st_mtime_ns
                else:
                    if newest_file is None or entry_meta.st_mtime_ns > newest_file:
                        newest_file = entry_meta.st_mtime_ns
    except FileNotFoundError:
        pass
    return newest_file if newest_file is not None else newest_dir


def remove_path(path: Path, stats: PruneStats) -> bool:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return False
    if stat.S_ISDIR(metadata.st_mode):
        shutil.rmtree(path)
    else:
        path.unlink()
    stats.removed_entries += 1
    return True


def list_subdirs(directory: Path, *, prefix: str | None = None) -> list[Path]:
    if not directory.is_dir() or directory.is_symlink():
        return []
    result: list[Path] = []
    with os.scandir(directory) as entries:
        for entry in entries:
            if entry.name.startswith("."):
                continue
            if prefix is not None and not entry.name.startswith(prefix):
                continue
            try:
                if stat.S_ISDIR(entry.stat(follow_symlinks=False).st_mode):
                    result.append(Path(entry.path))
            except FileNotFoundError:
                continue
    return result


def keep_newest_subdir(
    directory: Path,
    stats: PruneStats,
    *,
    prefix: str | None = None,
) -> Path | None:
    subdirs = list_subdirs(directory, prefix=prefix)
    if not subdirs:
        return None
    ranked = sorted(
        ((tree_max_mtime_ns(subdir), subdir.name, subdir) for subdir in subdirs),
        reverse=True,
    )
    keeper = ranked[0][2]
    for _, _, stale in ranked[1:]:
        remove_path(stale, stats)
    return keeper


def prune_metadata_entries(
    directory: Path,
    stats: PruneStats,
    *,
    keep: int,
    since_ns: int | None,
) -> None:
    if not directory.is_dir() or directory.is_symlink():
        return
    entries_with_mtime: list[tuple[int, str, Path]] = []
    with os.scandir(directory) as entries:
        for entry in entries:
            if entry.name.startswith("."):
                continue
            path = Path(entry.path)
            entries_with_mtime.append((tree_max_mtime_ns(path), entry.name, path))
    entries_with_mtime.sort(reverse=True)
    for index, (mtime_ns, _, path) in enumerate(entries_with_mtime):
        if index < keep:
            continue
        if since_ns is not None and mtime_ns >= since_ns:
            continue
        remove_path(path, stats)


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(HASH_CHUNK_SIZE):
            digest.update(chunk)
    return digest.hexdigest()


def prune_wrapper_snapshots(
    snapshot_root: Path,
    wrapper_path: Path | None,
    stats: PruneStats,
) -> None:
    subdirs = list_subdirs(snapshot_root)
    if len(subdirs) <= 1:
        return
    if wrapper_path is not None:
        try:
            meta = wrapper_path.lstat()
        except FileNotFoundError:
            meta = None
        if meta is not None and stat.S_ISREG(meta.st_mode):
            expected_prefix = f"{file_sha256(wrapper_path)}-"
            matching = [d for d in subdirs if d.name.startswith(expected_prefix)]
            if matching:
                matching_set = set(matching)
                for subdir in subdirs:
                    if subdir not in matching_set:
                        remove_path(subdir, stats)
                return
    keep_newest_subdir(snapshot_root, stats)


def select_active_rsjs_scope(rsjs_root: Path) -> str | None:
    shared_root = rsjs_root / "shared"
    final_root = rsjs_root / "final-target"
    scope_names: set[str] = set()
    for root in (shared_root, final_root):
        for subdir in list_subdirs(root):
            scope_names.add(subdir.name)
    if not scope_names:
        return None
    best_scope: str | None = None
    best_key: tuple[int, str] = (-1, "")
    for name in scope_names:
        newest = 0
        for root in (shared_root, final_root):
            candidate = root / name
            if candidate.is_dir() and not candidate.is_symlink():
                newest = max(newest, tree_max_mtime_ns(candidate))
        key = (newest, name)
        if key > best_key:
            best_key = key
            best_scope = name
    return best_scope


def prune_proa_rsjs(
    rsjs_root: Path,
    stats: PruneStats,
    *,
    since_ns: int | None,
    wrapper_path: Path | None,
    keep_metadata: int,
) -> list[Path]:
    if not rsjs_root.is_dir() or rsjs_root.is_symlink():
        return []

    active_cargo_targets: list[Path] = []
    # Determine active scope BEFORE removing cargo-timings so warm cache hits
    # still identify the active scope via freshly written timing files.
    active_scope = select_active_rsjs_scope(rsjs_root)
    stats.active_scope = active_scope

    shared_root = rsjs_root / "shared"
    if shared_root.is_dir() and not shared_root.is_symlink():
        for scope_dir in list_subdirs(shared_root):
            if scope_dir.name != active_scope:
                remove_path(scope_dir, stats)
        if active_scope is not None:
            active_shared = shared_root / active_scope
            if active_shared.is_dir() and not active_shared.is_symlink():
                active_descriptor = keep_newest_subdir(
                    active_shared, stats, prefix="descriptor-target-"
                )
                if active_descriptor is not None:
                    active_cargo_targets.append(active_descriptor)
                keep_newest_subdir(active_shared, stats, prefix="descriptor-cache-")

    final_root = rsjs_root / "final-target"
    if final_root.is_dir() and not final_root.is_symlink():
        for scope_dir in list_subdirs(final_root):
            if scope_dir.name != active_scope:
                remove_path(scope_dir, stats)
        if active_scope is not None:
            active_final = final_root / active_scope
            if active_final.is_dir() and not active_final.is_symlink():
                remove_path(active_final / "rsjs-final-artifact-snapshots", stats)
                active_wrapper = keep_newest_subdir(
                    active_final / "rsjs-final", stats, prefix="wrapper-v"
                )
                if active_wrapper is not None:
                    active_cargo_targets.append(active_wrapper)
                prune_metadata_entries(
                    active_final / "rsjs-final-unit-maps",
                    stats,
                    keep=keep_metadata,
                    since_ns=since_ns,
                )
                prune_metadata_entries(
                    active_final / "rsjs-final-unit-plans",
                    stats,
                    keep=keep_metadata,
                    since_ns=since_ns,
                )

    prune_wrapper_snapshots(rsjs_root / "wrapper-snapshots-v1", wrapper_path, stats)
    prune_metadata_entries(
        rsjs_root / "plans",
        stats,
        keep=keep_metadata,
        since_ns=since_ns,
    )

    # Remove per-invocation staging parent directories (e.g. <invocation-id>/descriptor-staging).
    with os.scandir(rsjs_root) as entries:
        for entry in entries:
            if entry.name.startswith(".") or entry.name in PROA_RSJS_STRUCTURAL_DIRS:
                continue
            try:
                if stat.S_ISDIR(entry.stat(follow_symlinks=False).st_mode):
                    remove_path(Path(entry.path), stats)
            except FileNotFoundError:
                continue

    return active_cargo_targets


def find_cargo_profile_dirs(cargo_target_dir: Path) -> list[Path]:
    if not cargo_target_dir.is_dir() or cargo_target_dir.is_symlink():
        return []
    profiles: list[Path] = []
    for subdir in list_subdirs(cargo_target_dir):
        if subdir.name in {"proa-rsjs", "proa-config", "proa-output", "cargo-timings"}:
            continue
        deps_dir = subdir / "deps"
        if deps_dir.is_dir() and not deps_dir.is_symlink():
            profiles.append(subdir)
    return profiles


def scan_profile_units(profile_dir: Path) -> tuple[list[CargoUnit], set[str]]:
    fingerprint_root = profile_dir / ".fingerprint"
    deps_root = profile_dir / "deps"
    build_root = profile_dir / "build"

    if not fingerprint_root.is_dir() or fingerprint_root.is_symlink():
        return [], set()

    deps_by_hash: dict[str, list[Path]] = {}
    if deps_root.is_dir() and not deps_root.is_symlink():
        with os.scandir(deps_root) as entries:
            for entry in entries:
                match = DEPS_HASH_RE.search(entry.name)
                if match:
                    deps_by_hash.setdefault(match.group("hash"), []).append(Path(entry.path))

    build_by_name: dict[str, Path] = {}
    if build_root.is_dir() and not build_root.is_symlink():
        with os.scandir(build_root) as entries:
            for entry in entries:
                if UNIT_DIR_RE.match(entry.name):
                    build_by_name[entry.name] = Path(entry.path)

    units: list[CargoUnit] = []
    live_hashes: set[str] = set()
    with os.scandir(fingerprint_root) as entries:
        for entry in entries:
            match = UNIT_DIR_RE.match(entry.name)
            if not match:
                continue
            try:
                if not stat.S_ISDIR(entry.stat(follow_symlinks=False).st_mode):
                    continue
            except FileNotFoundError:
                continue
            pkg = match.group("pkg")
            hash_hex = match.group("hash")
            live_hashes.add(hash_hex)
            fp_dir = Path(entry.path)
            json_markers: list[str] = []
            try:
                with os.scandir(fp_dir) as fp_entries:
                    for fp_entry in fp_entries:
                        if fp_entry.name.endswith(".json"):
                            json_markers.append(fp_entry.name)
            except FileNotFoundError:
                continue
            if not json_markers:
                continue

            build_dir = build_by_name.get(entry.name)
            deps_files = deps_by_hash.get(hash_hex, [])

            mtime_ns = shallow_max_mtime_ns(fp_dir)
            size_bytes = entry_size_bytes(fp_dir)
            if build_dir is not None:
                mtime_ns = max(mtime_ns, shallow_max_mtime_ns(build_dir))
                size_bytes += entry_size_bytes(build_dir)
            for dep_file in deps_files:
                try:
                    dep_meta = dep_file.lstat()
                except FileNotFoundError:
                    continue
                if dep_meta.st_mtime_ns > mtime_ns:
                    mtime_ns = dep_meta.st_mtime_ns
                size_bytes += dep_meta.st_size

            units.append(
                CargoUnit(
                    profile_dir=profile_dir,
                    pkg=pkg,
                    hash_hex=hash_hex,
                    kind_key=tuple(sorted(json_markers)),
                    fingerprint_dir=fp_dir,
                    build_dir=build_dir,
                    deps_files=deps_files,
                    mtime_ns=mtime_ns,
                    size_bytes=size_bytes,
                )
            )
    return units, live_hashes


def remove_cargo_unit(unit: CargoUnit, stats: PruneStats) -> None:
    remove_path(unit.fingerprint_dir, stats)
    if unit.build_dir is not None:
        remove_path(unit.build_dir, stats)
    for dep_file in unit.deps_files:
        remove_path(dep_file, stats)


def prune_superseded_profile_units(
    profile_dir: Path,
    stats: PruneStats,
    *,
    since_ns: int,
) -> None:
    units, live_hashes = scan_profile_units(profile_dir)
    if not live_hashes:
        return

    grouped: dict[tuple[str, tuple[str, ...]], list[CargoUnit]] = {}
    for unit in units:
        canonical_pkg = unit.pkg.replace("-", "_")
        grouped.setdefault((canonical_pkg, unit.kind_key), []).append(unit)

    for group_units in grouped.values():
        if len(group_units) < 2:
            continue
        has_fresh = any(unit.mtime_ns >= since_ns for unit in group_units)
        if not has_fresh:
            continue
        for unit in group_units:
            if unit.mtime_ns < since_ns:
                remove_cargo_unit(unit, stats)
                live_hashes.discard(unit.hash_hex)
                stats.superseded_units += 1

    # Remove orphaned deps/ and build/ entries whose hash has no .fingerprint dir
    # and was not created by the current build.
    deps_root = profile_dir / "deps"
    if deps_root.is_dir() and not deps_root.is_symlink():
        with os.scandir(deps_root) as entries:
            for entry in entries:
                match = DEPS_HASH_RE.search(entry.name)
                if not match or match.group("hash") in live_hashes:
                    continue
                try:
                    entry_meta = entry.stat(follow_symlinks=False)
                except FileNotFoundError:
                    continue
                if entry_meta.st_mtime_ns < since_ns:
                    if remove_path(Path(entry.path), stats):
                        stats.orphan_entries += 1

    build_root = profile_dir / "build"
    if build_root.is_dir() and not build_root.is_symlink():
        with os.scandir(build_root) as entries:
            for entry in entries:
                match = UNIT_DIR_RE.match(entry.name)
                if not match or match.group("hash") in live_hashes:
                    continue
                path = Path(entry.path)
                if tree_max_mtime_ns(path) < since_ns:
                    if remove_path(path, stats):
                        stats.orphan_entries += 1


def enforce_size_budget(
    target_dir: Path,
    profile_dirs: list[Path],
    stats: PruneStats,
    *,
    max_bytes: int,
    since_ns: int | None,
) -> int:
    current_bytes = entry_size_bytes(target_dir)
    if current_bytes <= max_bytes:
        return current_bytes

    candidates: list[CargoUnit] = []
    for profile_dir in profile_dirs:
        units, _ = scan_profile_units(profile_dir)
        for unit in units:
            if since_ns is not None and unit.mtime_ns >= since_ns:
                continue
            candidates.append(unit)

    candidates.sort(key=lambda unit: (unit.mtime_ns, -unit.size_bytes, unit.pkg, unit.hash_hex))
    for unit in candidates:
        if current_bytes <= max_bytes:
            break
        remove_cargo_unit(unit, stats)
        stats.budget_evicted_units += 1
        current_bytes = max(0, current_bytes - unit.size_bytes)

    return entry_size_bytes(target_dir)


def prune_target_directory(
    target_argument: str | Path,
    *,
    since_ns: int | None = None,
    wrapper_path: str | Path | None = None,
    max_bytes: int | None = DEFAULT_MAX_BYTES,
    keep_metadata: int = DEFAULT_KEEP_METADATA,
) -> PruneStats:
    target_dir = Path(target_argument).resolve(strict=True)
    if target_dir == Path("/"):
        raise PruneError("refusing to prune /")
    if not target_dir.is_dir():
        raise PruneError(f"target cache path is not a directory: {target_argument}")

    resolved_wrapper = Path(wrapper_path).resolve(strict=False) if wrapper_path is not None else None
    stats = PruneStats(bytes_before=entry_size_bytes(target_dir))

    cargo_target_dirs: list[Path] = [target_dir]
    cargo_target_dirs.extend(
        prune_proa_rsjs(
            target_dir / "proa-rsjs",
            stats,
            since_ns=since_ns,
            wrapper_path=resolved_wrapper,
            keep_metadata=keep_metadata,
        )
    )

    remove_path(target_dir / "proa-output", stats)

    active_config = keep_newest_subdir(target_dir / "proa-config", stats)
    if active_config is not None:
        config_target = active_config / "target"
        if config_target.is_dir() and not config_target.is_symlink():
            cargo_target_dirs.append(config_target)

    profile_dirs: list[Path] = []
    for cargo_target in cargo_target_dirs:
        remove_path(cargo_target / "cargo-timings", stats)
        for profile_dir in find_cargo_profile_dirs(cargo_target):
            if profile_dir.name in {"release", "release-fast"}:
                remove_path(profile_dir / "incremental", stats)
            profile_dirs.append(profile_dir)

    if since_ns is not None:
        for profile_dir in profile_dirs:
            prune_superseded_profile_units(profile_dir, stats, since_ns=since_ns)

    if max_bytes is not None:
        stats.bytes_after = enforce_size_budget(
            target_dir,
            profile_dirs,
            stats,
            max_bytes=max_bytes,
            since_ns=since_ns,
        )
    else:
        stats.bytes_after = entry_size_bytes(target_dir)

    return stats


def format_bytes(size: int) -> str:
    units = ("B", "KiB", "MiB", "GiB", "TiB")
    value = float(size)
    for unit in units:
        if value < 1024.0 or unit == units[-1]:
            return f"{value:.2f}{unit}" if unit != "B" else f"{int(value)}B"
        value /= 1024.0
    return f"{size}B"


def parse_args(arguments: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Prune stale RSJS scopes and superseded Cargo artifacts from a target cache."
    )
    parser.add_argument("target_dir", help="Path to CARGO_TARGET_DIR to prune.")
    since_group = parser.add_mutually_exclusive_group()
    since_group.add_argument(
        "--since",
        type=float,
        default=None,
        help="Unix epoch seconds marking the start of the current build.",
    )
    since_group.add_argument(
        "--since-file",
        default=None,
        help="File whose mtime marks the start of the current build.",
    )
    parser.add_argument(
        "--wrapper",
        default=None,
        help="Path to the current proa binary to retain its matching wrapper snapshot.",
    )
    parser.add_argument(
        "--max-bytes",
        type=int,
        default=DEFAULT_MAX_BYTES,
        help=f"Maximum uncompressed size budget for target_dir in bytes (default: {DEFAULT_MAX_BYTES}).",
    )
    parser.add_argument(
        "--keep-metadata",
        type=int,
        default=DEFAULT_KEEP_METADATA,
        help=f"Recent RSJS plan/unit-map metadata files to retain (default: {DEFAULT_KEEP_METADATA}).",
    )
    return parser.parse_args(arguments)


def main(arguments: list[str]) -> int:
    args = parse_args(arguments)
    since_ns: int | None = None
    if args.since is not None:
        since_ns = int(args.since * 1_000_000_000)
    elif args.since_file is not None:
        try:
            since_ns = Path(args.since_file).lstat().st_mtime_ns
        except OSError as error:
            print(f"cargo cache prune failed: cannot stat --since-file: {error}", file=sys.stderr)
            return 1

    try:
        stats = prune_target_directory(
            args.target_dir,
            since_ns=since_ns,
            wrapper_path=args.wrapper,
            max_bytes=args.max_bytes,
            keep_metadata=args.keep_metadata,
        )
    except (OSError, PruneError) as error:
        print(f"cargo cache prune failed: {error}", file=sys.stderr)
        return 1

    print(
        "cargo cache prune: "
        f"before={format_bytes(stats.bytes_before)} "
        f"after={format_bytes(stats.bytes_after)} "
        f"freed={format_bytes(stats.freed_bytes)} "
        f"removed_entries={stats.removed_entries} "
        f"superseded_units={stats.superseded_units} "
        f"orphan_entries={stats.orphan_entries} "
        f"budget_evicted_units={stats.budget_evicted_units} "
        f"active_scope={stats.active_scope or 'none'}"
    )
    if args.max_bytes is not None and stats.bytes_after > args.max_bytes:
        print(
            f"warning: target cache remains above budget "
            f"({format_bytes(stats.bytes_after)} > {format_bytes(args.max_bytes)})",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
