#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Verify and assemble the durable proa-cli GitHub Release asset set."""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
import tarfile
from pathlib import Path, PurePosixPath


TARGETS = (
    "x86_64-unknown-linux-gnu",
)
VERSION_PATTERN = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.]+)?$")
CHECKSUM_PATTERN = re.compile(r"^([0-9a-f]{64})  ([^/\\]+)$")


class VerificationError(RuntimeError):
    """A release directory does not satisfy the installer contract."""


def validate_version(version: str) -> None:
    if not VERSION_PATTERN.fullmatch(version):
        raise VerificationError(f"invalid proa-cli release version: {version!r}")


def expected_archives(version: str) -> tuple[str, ...]:
    validate_version(version)
    return tuple(sorted(f"proa-cli-{version}-{target}.tar.gz" for target in TARGETS))


def checksum_name(version: str) -> str:
    validate_version(version)
    return f"proa-cli-{version}-SHA256SUMS"


def expected_published_assets(version: str) -> tuple[str, ...]:
    return tuple(sorted((*expected_archives(version), checksum_name(version))))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def expected_archive_files(version: str, target: str) -> set[str]:
    root = f"proa-cli-{version}-{target}"
    binaries = {"proa", "cargo-proa"}
    return {f"{root}/{name}" for name in (*sorted(binaries), "README.md", "LICENSE")}


def validate_member_name(name: str, archive: Path) -> str:
    normalized = name.rstrip("/")
    path = PurePosixPath(normalized)
    if not normalized or path.is_absolute() or ".." in path.parts or "\\" in normalized:
        raise VerificationError(f"unsafe member {name!r} in {archive.name}")
    return normalized


def verify_tar_archive(path: Path, version: str, target: str) -> None:
    files: set[str] = set()
    with tarfile.open(path, "r:gz") as archive:
        for member in archive.getmembers():
            name = validate_member_name(member.name, path)
            if member.isdir():
                continue
            if not member.isfile():
                raise VerificationError(
                    f"unsupported non-file member {member.name!r} in {path.name}"
                )
            if name in files:
                raise VerificationError(f"duplicate member {name!r} in {path.name}")
            if member.size <= 0:
                raise VerificationError(f"empty release file {name!r} in {path.name}")
            if PurePosixPath(name).name in {"proa", "cargo-proa"} and member.mode & 0o111 == 0:
                raise VerificationError(f"binary {name!r} is not executable in {path.name}")
            files.add(name)

    expected = expected_archive_files(version, target)
    if files != expected:
        raise VerificationError(
            f"{path.name} members differ: expected {sorted(expected)}, found {sorted(files)}"
        )


def target_from_archive(name: str, version: str) -> str:
    prefix = f"proa-cli-{version}-"
    if not name.startswith(prefix):
        raise VerificationError(f"unexpected release archive name: {name}")
    if name.endswith(".tar.gz"):
        return name[len(prefix) : -len(".tar.gz")]
    raise VerificationError(f"unexpected release archive extension: {name}")


def read_sidecar(path: Path, archive_name: str) -> str:
    try:
        line = path.read_text(encoding="utf-8").strip()
    except UnicodeError as error:
        raise VerificationError(f"{path.name} is not UTF-8") from error
    match = CHECKSUM_PATTERN.fullmatch(line)
    if match is None or match.group(2) != archive_name:
        raise VerificationError(
            f"{path.name} must contain one sha256sum line for {archive_name}"
        )
    return match.group(1)


def verify_distribution(directory: Path, version: str) -> Path:
    validate_version(version)
    if not directory.is_dir():
        raise VerificationError(f"release directory does not exist: {directory}")

    expected = set(expected_archives(version))
    actual = {
        path.name
        for path in directory.iterdir()
        if path.is_file() and (path.name.endswith(".tar.gz") or path.suffix == ".zip")
    }
    if actual != expected:
        raise VerificationError(
            f"release archives differ: expected {sorted(expected)}, found {sorted(actual)}"
        )

    checksum_lines: list[str] = []
    for archive_name in sorted(expected):
        archive = directory / archive_name
        target = target_from_archive(archive_name, version)
        if target not in TARGETS:
            raise VerificationError(f"unexpected target in {archive_name}")
        verify_tar_archive(archive, version, target)

        sidecar = directory / f"{archive_name}.sha256"
        if not sidecar.is_file():
            raise VerificationError(f"missing checksum sidecar: {sidecar.name}")
        expected_digest = read_sidecar(sidecar, archive_name)
        actual_digest = sha256_file(archive)
        if actual_digest != expected_digest:
            raise VerificationError(
                f"checksum mismatch for {archive_name}: expected {expected_digest}, got {actual_digest}"
            )
        checksum_lines.append(f"{actual_digest}  {archive_name}\n")

    unexpected_sidecars = {
        path.name
        for path in directory.glob("*.sha256")
        if path.name.removesuffix(".sha256") not in expected
    }
    if unexpected_sidecars:
        raise VerificationError(f"unexpected checksum sidecars: {sorted(unexpected_sidecars)}")

    destination = directory / checksum_name(version)
    temporary = destination.with_name(f".{destination.name}.tmp")
    temporary.write_text("".join(checksum_lines), encoding="utf-8", newline="\n")
    temporary.replace(destination)
    return destination


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    verify = subparsers.add_parser("verify", help="verify archives and assemble SHA256SUMS")
    verify.add_argument("directory", type=Path)
    verify.add_argument("--version", required=True)

    names = subparsers.add_parser("names", help="print the exact published asset names")
    names.add_argument("--version", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.command == "names":
            for name in expected_published_assets(args.version):
                print(name)
            return 0

        destination = verify_distribution(args.directory, args.version)
        print(f"verified the proa-cli archive and wrote {destination}")
        return 0
    except (OSError, tarfile.TarError, VerificationError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
