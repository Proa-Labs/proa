#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import hashlib
import io
import os
import re
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path

from verify_cli_release_assets import (
    TARGETS,
    VerificationError,
    expected_archive_files,
    expected_archives,
    expected_published_assets,
    sha256_file,
    verify_distribution,
)


ROOT = Path(__file__).resolve().parents[1]
VERSION = "9.8.7"
MAX_DISTRIBUTION_VERSION_LENGTH = 64
FULL_COMMIT = "0" * 40
DISTRIBUTION_VERSION_PATTERN = re.compile(
    r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.]+)?$"
)


def cli_manifest_version() -> str:
    in_package = False
    manifest = ROOT / "proa_cli" / "Cargo.toml"
    for line in manifest.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            in_package = stripped == "[package]"
            continue
        if in_package:
            match = re.fullmatch(r'version\s*=\s*"([^"]+)"(?:\s*#.*)?', stripped)
            if match:
                return match.group(1)
    raise AssertionError(f"could not read [package].version from {manifest}")


def main_snapshot_version(cli_version: str, commit: str = FULL_COMMIT) -> str:
    core, separator, prerelease = cli_version.partition("-")
    version = f"{core}-main.{commit}"
    return f"{version}.{prerelease}" if separator else version


def assert_distribution_version(test: unittest.TestCase, version: str) -> None:
    test.assertLessEqual(
        len(version),
        MAX_DISTRIBUTION_VERSION_LENGTH,
        f"{version!r} exceeds the download bridge's version limit",
    )
    test.assertRegex(
        version,
        DISTRIBUTION_VERSION_PATTERN,
        "proa-cli package versions must fit the release and download grammar",
    )


def add_tar_file(archive: tarfile.TarFile, name: str, *, executable: bool = False) -> None:
    payload = f"fixture:{name}\n".encode()
    info = tarfile.TarInfo(name)
    info.size = len(payload)
    info.mode = 0o755 if executable else 0o644
    archive.addfile(info, io.BytesIO(payload))


def make_archive(directory: Path, name: str) -> None:
    prefix = f"proa-cli-{VERSION}-"
    target = name[len(prefix) : -len(".tar.gz")]
    with tarfile.open(directory / name, "w:gz") as archive:
        for member in sorted(expected_archive_files(VERSION, target)):
            add_tar_file(
                archive,
                member,
                executable=Path(member).name in {"proa", "cargo-proa"},
            )

    digest = sha256_file(directory / name)
    (directory / f"{name}.sha256").write_text(
        f"{digest}  {name}\n", encoding="utf-8", newline="\n"
    )


def make_distribution(directory: Path) -> None:
    for name in expected_archives(VERSION):
        make_archive(directory, name)


class ReleaseVersionContractTests(unittest.TestCase):
    def test_manifest_version_fits_stable_and_main_distribution_identities(self) -> None:
        version = cli_manifest_version()
        assert_distribution_version(self, version)
        assert_distribution_version(self, main_snapshot_version(version))

    def test_main_identity_length_budget_is_explicit(self) -> None:
        fits = main_snapshot_version("0.1.1-beta.1234567")
        too_long = main_snapshot_version("0.1.1-beta.12345678")
        self.assertEqual(len(fits), MAX_DISTRIBUTION_VERSION_LENGTH)
        assert_distribution_version(self, fits)
        self.assertGreater(len(too_long), MAX_DISTRIBUTION_VERSION_LENGTH)

    def test_distribution_grammar_rejects_unsupported_semver_forms(self) -> None:
        for version in ["0.1.1-alpha-beta", "0.1.1+build.1"]:
            self.assertIsNone(DISTRIBUTION_VERSION_PATTERN.fullmatch(version))


class ReleaseAssetVerificationTests(unittest.TestCase):
    def test_expected_contract_has_one_ubuntu_archive_and_one_checksum(self) -> None:
        archives = expected_archives(VERSION)
        self.assertEqual(len(archives), 1)
        self.assertEqual(TARGETS, ("x86_64-unknown-linux-gnu",))
        self.assertEqual(
            expected_published_assets(VERSION),
            (
                "proa-cli-9.8.7-SHA256SUMS",
                "proa-cli-9.8.7-x86_64-unknown-linux-gnu.tar.gz",
            ),
        )

    def test_verifier_checks_archives_and_assembles_sorted_checksums(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            make_distribution(directory)

            sums = verify_distribution(directory, VERSION)

            lines = sums.read_text(encoding="utf-8").splitlines()
            self.assertEqual([line.split("  ", 1)[1] for line in lines], list(expected_archives(VERSION)))
            for line in lines:
                digest, name = line.split("  ", 1)
                self.assertEqual(digest, hashlib.sha256((directory / name).read_bytes()).hexdigest())

    def test_verifier_rejects_a_missing_target(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            make_distribution(directory)
            (directory / expected_archives(VERSION)[0]).unlink()

            with self.assertRaisesRegex(VerificationError, "release archives differ"):
                verify_distribution(directory, VERSION)

    def test_verifier_rejects_a_sidecar_checksum_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            make_distribution(directory)
            archive = expected_archives(VERSION)[0]
            (directory / f"{archive}.sha256").write_text(
                f"{'0' * 64}  {archive}\n", encoding="utf-8"
            )

            with self.assertRaisesRegex(VerificationError, "checksum mismatch"):
                verify_distribution(directory, VERSION)

    def test_verifier_rejects_an_unexpected_archive_member(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            make_distribution(directory)
            archive_name = next(name for name in expected_archives(VERSION) if name.endswith(".tar.gz"))
            archive_path = directory / archive_name
            target = archive_name.removeprefix(f"proa-cli-{VERSION}-").removesuffix(".tar.gz")
            with tarfile.open(archive_path, "w:gz") as archive:
                for member in sorted(expected_archive_files(VERSION, target)):
                    add_tar_file(
                        archive,
                        member,
                        executable=Path(member).name in {"proa", "cargo-proa"},
                    )
                add_tar_file(archive, "../escape")
            digest = sha256_file(archive_path)
            (directory / f"{archive_name}.sha256").write_text(
                f"{digest}  {archive_name}\n", encoding="utf-8"
            )

            with self.assertRaisesRegex(VerificationError, "unsafe member"):
                verify_distribution(directory, VERSION)


class InstallerContractTests(unittest.TestCase):
    def test_unix_help_documents_fail_closed_ci_mode(self) -> None:
        result = subprocess.run(
            ["sh", str(ROOT / "scripts/install.sh"), "--help"],
            check=True,
            capture_output=True,
            text=True,
            timeout=10,
            env={**os.environ, "PROA_LOG": "off"},
        )
        self.assertIn("--prebuilt-only", result.stdout)
        self.assertIn("PROA_PREBUILT_ONLY", result.stdout)

    def test_unix_source_install_needs_no_private_cargo_index(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binaries = root / "bin"
            binaries.mkdir()
            cargo = binaries / "cargo"
            cargo.write_text("""#!/bin/sh
if [ "$1" = --version ]; then echo 'cargo 1.93.0'; exit 0; fi
[ "$*" = 'install --git https://github.com/Proa-Labs/proa --locked proa-cli' ] || exit 91
[ -z "${CARGO_REGISTRIES_PROA_INDEX+x}" ] || exit 92
[ "$CARGO_NET_GIT_FETCH_WITH_CLI" = true ] || exit 93
mkdir -p "$CARGO_HOME/bin"
printf '#!/bin/sh\\necho proa 0.1.1\\n' > "$CARGO_HOME/bin/proa"
cp "$CARGO_HOME/bin/proa" "$CARGO_HOME/bin/cargo-proa"
chmod +x "$CARGO_HOME/bin/proa" "$CARGO_HOME/bin/cargo-proa"
""")
            cargo.chmod(0o755)
            environment = {key: value for key, value in os.environ.items()
                           if not key.startswith(("CARGO_REGISTRIES_", "PROA_"))}
            environment.update(CARGO_HOME=str(root / "cargo-home"),
                               PATH=str(binaries) + os.pathsep + os.environ["PATH"],
                               PROA_HOME=str(root / "proa-home"), PROA_LOG="off",
                               CARGO_NET_GIT_FETCH_WITH_CLI="true", NO_COLOR="1")
            result = subprocess.run(
                ["sh", str(ROOT / "scripts/install.sh"), "--build-from-source",
                 "--no-modify-path", "--no-tailwind", "--yes"],
                cwd=root, env=environment, capture_output=True, text=True, timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertTrue((root / "cargo-home/bin/proa").is_file())

    def test_unix_installer_rejects_opposing_source_modes(self) -> None:
        result = subprocess.run(
            [
                "sh",
                str(ROOT / "scripts/install.sh"),
                "--build-from-source",
                "--prebuilt-only",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
            env={**os.environ, "PROA_LOG": "off", "NO_COLOR": "1"},
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--build-from-source and --prebuilt-only cannot be combined", result.stderr)

    @unittest.skipUnless(shutil.which("pwsh"), "PowerShell is not installed")
    def test_windows_help_documents_fail_closed_ci_mode(self) -> None:
        result = subprocess.run(
            [
                "pwsh",
                "-NoLogo",
                "-NoProfile",
                "-File",
                str(ROOT / "scripts/install.ps1"),
                "-Help",
            ],
            check=True,
            capture_output=True,
            text=True,
            timeout=20,
            env={**os.environ, "PROA_LOG": "off"},
        )
        self.assertIn("-PrebuiltOnly", result.stdout)
        self.assertIn("PROA_PREBUILT_ONLY", result.stdout)

    @unittest.skipUnless(
        shutil.which("pwsh") and os.name == "nt", "requires Windows PowerShell"
    )
    def test_windows_installer_rejects_opposing_source_modes(self) -> None:
        result = subprocess.run(
            [
                "pwsh",
                "-NoLogo",
                "-NoProfile",
                "-File",
                str(ROOT / "scripts/install.ps1"),
                "-BuildFromSource",
                "-PrebuiltOnly",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=20,
            env={**os.environ, "PROA_LOG": "off", "NO_COLOR": "1"},
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "-BuildFromSource and -PrebuiltOnly cannot be combined",
            result.stdout + result.stderr,
        )


if __name__ == "__main__":
    unittest.main()
