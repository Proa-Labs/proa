#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Reconcile a rematerialized Docker context into a content-stable workspace."""

from __future__ import annotations

import os
import shutil
import stat
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path


CHUNK_SIZE = 1024 * 1024


class SyncError(RuntimeError):
    pass


@dataclass
class SyncStats:
    unchanged_files: int = 0
    updated_files: int = 0
    unchanged_links: int = 0
    updated_links: int = 0
    created_directories: int = 0
    updated_directories: int = 0
    deleted_entries: int = 0


def same_file_bytes(source: Path, destination: Path, size: int) -> bool:
    if destination.stat().st_size != size:
        return False
    with source.open("rb") as source_file, destination.open("rb") as destination_file:
        while True:
            source_chunk = source_file.read(CHUNK_SIZE)
            destination_chunk = destination_file.read(CHUNK_SIZE)
            if source_chunk != destination_chunk:
                return False
            if not source_chunk:
                return True


def remove_entry(path: Path) -> None:
    metadata = path.lstat()
    if stat.S_ISDIR(metadata.st_mode):
        shutil.rmtree(path)
    else:
        path.unlink()


def changed_mtime(previous_mtime_ns: int | None) -> int:
    now = time.time_ns()
    if previous_mtime_ns is None:
        return now
    return max(now, previous_mtime_ns + 1)


def set_changed_mtime(path: Path, previous_mtime_ns: int | None, *, symlink: bool = False) -> None:
    timestamp = changed_mtime(previous_mtime_ns)
    os.utime(
        path,
        ns=(timestamp, timestamp),
        follow_symlinks=not symlink,
    )


def replace_regular_file(
    source: Path,
    destination: Path,
    source_mode: int,
    previous_mtime_ns: int | None,
) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    file_descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{destination.name}.proa-sync-",
        dir=destination.parent,
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(file_descriptor, "wb") as output, source.open("rb") as input_file:
            shutil.copyfileobj(input_file, output, length=CHUNK_SIZE)
        temporary.chmod(source_mode)
        set_changed_mtime(temporary, previous_mtime_ns)
        if destination.exists() and destination.is_dir() and not destination.is_symlink():
            shutil.rmtree(destination)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


def sync_regular_file(
    source: Path,
    destination: Path,
    source_metadata: os.stat_result,
    stats: SyncStats,
) -> None:
    source_mode = stat.S_IMODE(source_metadata.st_mode)
    destination_metadata: os.stat_result | None = None
    try:
        destination_metadata = destination.lstat()
    except FileNotFoundError:
        pass

    if (
        destination_metadata is not None
        and stat.S_ISREG(destination_metadata.st_mode)
        and stat.S_IMODE(destination_metadata.st_mode) == source_mode
        and same_file_bytes(source, destination, source_metadata.st_size)
    ):
        stats.unchanged_files += 1
        return

    previous_mtime_ns = (
        destination_metadata.st_mtime_ns if destination_metadata is not None else None
    )
    replace_regular_file(source, destination, source_mode, previous_mtime_ns)
    stats.updated_files += 1


def sync_symlink(
    source: Path,
    destination: Path,
    stats: SyncStats,
) -> None:
    source_target = os.readlink(source)
    destination_metadata: os.stat_result | None = None
    try:
        destination_metadata = destination.lstat()
    except FileNotFoundError:
        pass

    if destination_metadata is not None and stat.S_ISLNK(destination_metadata.st_mode):
        if os.readlink(destination) == source_target:
            stats.unchanged_links += 1
            return

    previous_mtime_ns = (
        destination_metadata.st_mtime_ns if destination_metadata is not None else None
    )
    file_descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{destination.name}.proa-sync-",
        dir=destination.parent,
    )
    os.close(file_descriptor)
    temporary = Path(temporary_name)
    temporary.unlink()
    try:
        os.symlink(source_target, temporary)
        set_changed_mtime(temporary, previous_mtime_ns, symlink=True)
        if destination_metadata is not None and stat.S_ISDIR(destination_metadata.st_mode):
            shutil.rmtree(destination)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)
    stats.updated_links += 1


def ensure_directory(
    source_metadata: os.stat_result,
    destination: Path,
    stats: SyncStats,
) -> tuple[bool, int | None]:
    source_mode = stat.S_IMODE(source_metadata.st_mode)
    destination_metadata: os.stat_result | None = None
    try:
        destination_metadata = destination.lstat()
    except FileNotFoundError:
        pass

    previous_mtime_ns = (
        destination_metadata.st_mtime_ns if destination_metadata is not None else None
    )
    if destination_metadata is None:
        destination.mkdir(mode=0o700)
        stats.created_directories += 1
        return True, previous_mtime_ns
    if not stat.S_ISDIR(destination_metadata.st_mode):
        remove_entry(destination)
        destination.mkdir(mode=0o700)
        stats.created_directories += 1
        return True, previous_mtime_ns
    if stat.S_IMODE(destination_metadata.st_mode) != source_mode:
        stats.updated_directories += 1
        return True, previous_mtime_ns
    return False, previous_mtime_ns


def sync_directory(
    source: Path,
    destination: Path,
    relative: Path,
    stats: SyncStats,
) -> None:
    source_metadata = source.lstat()
    directory_changed, previous_mtime_ns = ensure_directory(
        source_metadata,
        destination,
        stats,
    )

    source_names: set[str] = set()
    with os.scandir(source) as entries:
        for entry in entries:
            source_names.add(entry.name)
            source_path = source / entry.name
            destination_path = destination / entry.name
            entry_relative = relative / entry.name
            metadata = entry.stat(follow_symlinks=False)

            if stat.S_ISREG(metadata.st_mode):
                sync_regular_file(source_path, destination_path, metadata, stats)
            elif stat.S_ISLNK(metadata.st_mode):
                sync_symlink(source_path, destination_path, stats)
            elif stat.S_ISDIR(metadata.st_mode):
                sync_directory(
                    source_path,
                    destination_path,
                    entry_relative,
                    stats,
                )
            else:
                raise SyncError(f"unsupported special file in build context: {entry_relative}")

    with os.scandir(destination) as entries:
        stale_names = [entry.name for entry in entries if entry.name not in source_names]
    for name in stale_names:
        remove_entry(destination / name)
        stats.deleted_entries += 1
        directory_changed = True

    source_mode = stat.S_IMODE(source_metadata.st_mode)
    if stat.S_IMODE(destination.lstat().st_mode) != source_mode:
        destination.chmod(source_mode)
        directory_changed = True
    if directory_changed:
        set_changed_mtime(destination, previous_mtime_ns)


def canonical_directories(incoming_argument: str, cached_argument: str) -> tuple[Path, Path]:
    incoming = Path(incoming_argument).resolve(strict=True)
    if not incoming.is_dir():
        raise SyncError(f"incoming build context is not a directory: {incoming_argument}")

    cached_path = Path(cached_argument)
    cached_path.mkdir(parents=True, exist_ok=True)
    cached = cached_path.resolve(strict=True)
    if cached == Path("/"):
        raise SyncError("refusing to synchronize into /")
    if incoming == cached:
        raise SyncError("incoming and cached build directories must differ")
    if incoming in cached.parents:
        raise SyncError("cached directory must not be inside the incoming build context")
    if cached in incoming.parents:
        raise SyncError("incoming build context must not be inside the cached directory")
    return incoming, cached


def synchronize(incoming_argument: str, cached_argument: str) -> SyncStats:
    incoming, cached = canonical_directories(incoming_argument, cached_argument)
    stats = SyncStats()
    sync_directory(incoming, cached, Path(), stats)
    return stats


def main(arguments: list[str]) -> int:
    if len(arguments) != 2:
        print(
            f"usage: {Path(sys.argv[0]).name} <incoming-directory> <cached-directory>",
            file=sys.stderr,
        )
        return 2
    try:
        stats = synchronize(arguments[0], arguments[1])
    except (OSError, SyncError) as error:
        print(f"build context sync failed: {error}", file=sys.stderr)
        return 1

    print(
        "build context sync: "
        f"unchanged_files={stats.unchanged_files} "
        f"updated_files={stats.updated_files} "
        f"unchanged_links={stats.unchanged_links} "
        f"updated_links={stats.updated_links} "
        f"created_directories={stats.created_directories} "
        f"updated_directories={stats.updated_directories} "
        f"deleted_entries={stats.deleted_entries}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
