#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import hashlib
import io
import json
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
[ "$*" = 'install --locked proa-cli --git https://github.com/Proa-Labs/proa' ] || exit 91
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

    def run_unix_fallback(self, arguments: list[str], *, latest: str = "",
                          cargo_status: int = 0, architecture: str = "x86_64") -> tuple[subprocess.CompletedProcess[str], list[str]]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binaries = root / "bin"
            binaries.mkdir()
            fixtures = {
                "cargo": """#!/bin/sh
if [ "$1" = --version ]; then echo 'cargo 1.93.0'; exit 0; fi
printf '%s\\n' "$@" >> "$FIXTURE_ARGUMENTS"
[ "$FIXTURE_CARGO_STATUS" = 0 ] || exit "$FIXTURE_CARGO_STATUS"
mkdir -p "$CARGO_HOME/bin"
printf '#!/bin/sh\\necho proa 9.8.7\\n' > "$CARGO_HOME/bin/proa"
cp "$CARGO_HOME/bin/proa" "$CARGO_HOME/bin/cargo-proa"
chmod +x "$CARGO_HOME/bin/proa" "$CARGO_HOME/bin/cargo-proa"
""",
                "rustc": "#!/bin/sh\necho 'rustc 1.93.0'\n",
                "uname": "#!/bin/sh\ncase \"$1\" in -s) echo Linux ;; -m) echo \"$FIXTURE_ARCH\" ;; *) echo Linux ;; esac\n",
                "curl": """#!/usr/bin/env python3
import json, os, pathlib, sys
if sys.argv[1:] == ['--version']:
    print('curl fixture'); sys.exit(0)
if sys.argv[-1].endswith('/latest.json') and os.environ['FIXTURE_LATEST']:
    pathlib.Path(sys.argv[sys.argv.index('--output') + 1]).write_text(
        json.dumps({'version': os.environ['FIXTURE_LATEST']}))
    print('200 100'); sys.exit(0)
print('404 0'); sys.exit(22)
""",
            }
            for name, source in fixtures.items():
                path = binaries / name
                path.write_text(source)
                path.chmod(0o755)
            environment = {key: value for key, value in os.environ.items()
                           if not key.startswith(("CARGO_", "PROA_"))}
            environment.update(CARGO_HOME=str(root / "cargo-home"),
                               PATH=str(binaries) + os.pathsep + os.environ["PATH"],
                               PROA_HOME=str(root / "proa-home"), PROA_LOG="off", NO_COLOR="1",
                               FIXTURE_ARGUMENTS=str(root / "arguments"), FIXTURE_LATEST=latest,
                               FIXTURE_CARGO_STATUS=str(cargo_status), FIXTURE_ARCH=architecture)
            result = subprocess.run(
                ["sh", str(ROOT / "scripts/install.sh"), "--no-modify-path",
                 "--no-tailwind", "--yes", *arguments], cwd=root, env=environment,
                capture_output=True, text=True, timeout=30)
            path = root / "arguments"
            return result, path.read_text().splitlines() if path.exists() else []

    def test_missing_archive_preserves_selected_release(self) -> None:
        for arguments, latest in [(["--version", VERSION], ""), ([], VERSION),
                                  (["--build-from-source", "--version", VERSION], ""),
                                  (["--repo", "PROA-LABS/PROA", "--version", VERSION], "")]:
            with self.subTest(arguments=arguments):
                result, actual = self.run_unix_fallback(arguments, latest=latest)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(actual, ["install", "--locked", "proa-cli", "--registry",
                                          "crates-io", "--version", "=" + VERSION])

    def test_unsupported_architecture_resolves_channel_version_before_source_build(self) -> None:
        result, actual = self.run_unix_fallback([], latest=VERSION, architecture="s390x")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(actual, ["install", "--locked", "proa-cli", "--registry",
                                  "crates-io", "--version", "=" + VERSION])

    def test_missing_metadata_uses_latest_registry_release(self) -> None:
        result, actual = self.run_unix_fallback([])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(actual, ["install", "--locked", "proa-cli", "--registry", "crates-io"])

    def test_main_snapshot_fallback_pins_full_revision(self) -> None:
        version = main_snapshot_version("0.1.1-beta.1")
        result, actual = self.run_unix_fallback(["--version", version])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(actual, ["install", "--locked", "proa-cli", "--git",
                                  "https://github.com/Proa-Labs/proa", "--rev", FULL_COMMIT])

    def test_custom_repository_fallback_pins_release_tag(self) -> None:
        result, actual = self.run_unix_fallback(["--version", VERSION, "--repo", "example/proa"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(actual, ["install", "--locked", "proa-cli", "--git",
                                  "https://github.com/example/proa", "--tag", "proa-cli-v" + VERSION])

    def test_failed_selected_release_never_retries_git_head(self) -> None:
        result, actual = self.run_unix_fallback(["--version", VERSION], cargo_status=91)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(actual.count("install"), 1)
        self.assertNotIn("--git", actual)
        self.assertIn("cargo install failed", result.stdout + result.stderr)

    def test_malformed_main_revision_is_rejected_before_cargo(self) -> None:
        result, actual = self.run_unix_fallback(["--version", "0.1.1-main.short"])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(actual, [])
        self.assertIn("full 40-character commit SHA", result.stdout + result.stderr)

    def test_prebuilt_only_never_invokes_cargo_on_missing_archive(self) -> None:
        result, actual = self.run_unix_fallback(["--version", VERSION, "--prebuilt-only"])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(actual, [])

    @unittest.skipUnless(shutil.which("pwsh"), "requires PowerShell")
    def test_windows_source_install_preserves_version_and_revision(self) -> None:
        # Load the real functions through the parser without running Invoke-Main;
        # Windows PATH/registry operations need not run to exercise Cargo calls.
        harness = r"""
param([string]$Installer)
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($Installer, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
foreach ($name in @('Get-SourceInstallArguments', 'Install-FromSource')) {
    $definition = $ast.Find({ param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name
    }, $true)
    Invoke-Expression $definition.Extent.Text
}
function Get-CargoBinDir { return '/tmp/cargo/bin' }
function Test-CommandExists { return $true }
function Set-PreviousVersion {}
function Write-LogSection {}
function Write-StepStart {}
function Write-Blank {}
function Get-EnvValue { return 'true' }
function Write-LogLine {}
function Write-StepOk {}
function Write-KeyValue {}
function Get-DisplayPath { param($Path); return $Path }
function Stop-Install { param($Message); throw $Message }
function Invoke-NativeCommand { param($Program, $Arguments)
    $script:Captured = @($Program) + $Arguments; return 0
}
$cases = @(
    @('Proa-Labs/proa', '9.8.7', $false),
    @('Proa-Labs/proa', '9.8.7', $true),
    @('Proa-Labs/proa', '', $false),
    @('Proa-Labs/proa', '', $true),
    @('Proa-Labs/proa', ('0.1.1-main.' + ('0' * 40) + '.beta.1'), $false),
    @('example/proa', '9.8.7', $false)
)
foreach ($case in $cases) {
    $null = Install-FromSource $case[0] '/tmp/cargo/bin' $false $case[1] $case[2]
    ConvertTo-Json -Compress -InputObject $script:Captured
}
try {
    $null = Install-FromSource 'Proa-Labs/proa' '/tmp/cargo/bin' $false '0.1.1-main.short'
    throw 'malformed snapshot accepted'
} catch {
    if ($_.Exception.Message -ne 'main snapshot version must contain a full 40-character commit SHA') { throw }
}
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test.ps1"
            path.write_text(harness)
            result = subprocess.run(["pwsh", "-NoLogo", "-NoProfile", "-File", str(path),
                                     str(ROOT / "scripts/install.ps1")],
                                    capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        actual = [json.loads(line) for line in result.stdout.splitlines()]
        base = ["cargo", "install", "--locked", "proa-cli"]
        self.assertEqual(actual, [
            base + ["--registry", "crates-io", "--version", "=9.8.7"],
            base + ["--registry", "crates-io", "--version", "=9.8.7"],
            base + ["--registry", "crates-io"],
            base + ["--git", "https://github.com/Proa-Labs/proa"],
            base + ["--git", "https://github.com/Proa-Labs/proa", "--rev", FULL_COMMIT],
            base + ["--git", "https://github.com/example/proa", "--tag", "proa-cli-v9.8.7"],
        ])

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
