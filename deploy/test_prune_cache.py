# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Unit tests for the marketing Cloud Build Cargo/RSJS cache pruner."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from marketing import prune_cache


T_OLD = 1_700_000_000_000_000_000
T_MID = 1_700_000_100_000_000_000
T_SINCE = 1_700_000_200_000_000_000
T_FRESH = 1_700_000_300_000_000_000


def write_bytes(path: Path, data: bytes, mtime_ns: int) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    os.utime(path, ns=(mtime_ns, mtime_ns))
    return path


def set_dir_mtime(path: Path, mtime_ns: int) -> None:
    os.utime(path, ns=(mtime_ns, mtime_ns))


def make_cargo_unit(
    profile_dir: Path,
    pkg: str,
    hash_hex: str,
    *,
    kind_json: str,
    mtime_ns: int,
    dep_files: list[tuple[str, int]] | None = None,
    with_build_dir: bool = False,
) -> None:
    unit_name = f"{pkg}-{hash_hex}"
    fp_dir = profile_dir / ".fingerprint" / unit_name
    write_bytes(fp_dir / kind_json, b"{}", mtime_ns)
    write_bytes(fp_dir / "invoked.timestamp", b"", mtime_ns)
    set_dir_mtime(fp_dir, mtime_ns)

    if dep_files:
        for filename, size in dep_files:
            write_bytes(profile_dir / "deps" / filename, b"x" * size, mtime_ns)

    if with_build_dir:
        build_dir = profile_dir / "build" / unit_name
        write_bytes(build_dir / "output", b"cargo:rerun-if-changed=build.rs\n", mtime_ns)
        set_dir_mtime(build_dir, mtime_ns)


class PruneCacheTests(unittest.TestCase):
    def test_prunes_stale_rsjs_scopes_and_ephemeral_directories(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "target"
            rsjs = target / "proa-rsjs"

            # Old RSJS scope (stale wrapper hash)
            old_desc = (
                rsjs / "shared" / "oldscope00000001" / "descriptor-target-policy-v1-persistent-v1"
            )
            make_cargo_unit(
                old_desc / "release",
                "proa_marketing",
                "1111111111111111",
                kind_json="bin-proa_marketing.json",
                mtime_ns=T_OLD,
                dep_files=[("proa_marketing-1111111111111111", 4096)],
            )
            old_final = rsjs / "final-target" / "oldscope00000001" / "rsjs-final" / "wrapper-v1"
            make_cargo_unit(
                old_final / "release",
                "proa_marketing",
                "2222222222222222",
                kind_json="bin-proa_marketing.json",
                mtime_ns=T_OLD,
                dep_files=[("proa_marketing-2222222222222222", 4096)],
            )

            # Active RSJS scope: deps are old (warm cache hit), only cargo-timings is fresh.
            active_shared = rsjs / "shared" / "newscope00000002"
            active_desc = active_shared / "descriptor-target-policy-v2-persistent-v2"
            make_cargo_unit(
                active_desc / "release",
                "syn",
                "aaaaaaaaaaaaaaaa",
                kind_json="lib-syn.json",
                mtime_ns=T_OLD,
                dep_files=[("libsyn-aaaaaaaaaaaaaaaa.rlib", 2048)],
            )
            write_bytes(active_desc / "cargo-timings" / "cargo-timing.html", b"<html/>", T_FRESH)
            # Older schema version inside the active scope should be removed.
            write_bytes(
                active_shared / "descriptor-target-policy-v1-persistent-v1" / "release" / "deps" / "old",
                b"old",
                T_OLD,
            )
            write_bytes(active_shared / "descriptor-cache-v4" / "index.json", b"{}", T_OLD)
            write_bytes(active_shared / "descriptor-cache-v5" / "index.json", b"{}", T_MID)
            write_bytes(active_shared / "descriptor-cache-v5" / "entry.bin", b"keep", T_OLD)

            active_final_root = rsjs / "final-target" / "newscope00000002"
            active_final = active_final_root / "rsjs-final" / "wrapper-v2"
            make_cargo_unit(
                active_final / "release",
                "syn",
                "bbbbbbbbbbbbbbbb",
                kind_json="lib-syn.json",
                mtime_ns=T_OLD,
                dep_files=[("libsyn-bbbbbbbbbbbbbbbb.rlib", 2048)],
            )
            write_bytes(active_final / "cargo-timings" / "cargo-timing.html", b"<html/>", T_FRESH)
            write_bytes(
                active_final_root / "rsjs-final" / "wrapper-v1" / "release" / "deps" / "stale",
                b"stale",
                T_OLD,
            )
            write_bytes(
                active_final_root / "rsjs-final-artifact-snapshots" / "abc-755" / "proa_marketing",
                b"binary" * 1000,
                T_FRESH,
            )
            for idx in range(6):
                write_bytes(
                    active_final_root / "rsjs-final-unit-maps" / f"map-{idx}.json",
                    b"{}",
                    T_OLD + idx * 1_000_000_000,
                )
                write_bytes(
                    active_final_root / "rsjs-final-unit-plans" / f"plan-{idx}.json",
                    b"{}",
                    T_OLD + idx * 1_000_000_000,
                )
                write_bytes(
                    rsjs / "plans" / f"semantic-{idx}.json",
                    b"{}",
                    T_OLD + idx * 1_000_000_000,
                )

            # Per-invocation staging directory + dotfile lock
            write_bytes(rsjs / "inv-deadbeef" / "descriptor-staging" / "nonce" / "obj", b"x", T_OLD)
            write_bytes(rsjs / ".proa-rsjs-target-locks", b"", T_OLD)

            # Ephemeral top-level directories
            write_bytes(target / "proa-output" / "assets" / "app.js", b"console.log(1)", T_FRESH)
            write_bytes(target / "cargo-timings" / "cargo-timing.html", b"<html/>", T_FRESH)
            write_bytes(target / "release-fast" / "deps" / "marker", b"", T_OLD)
            write_bytes(target / "release-fast" / "incremental" / "inc.bin", b"inc", T_FRESH)

            # Old and active proa-config directories
            write_bytes(target / "proa-config" / "old-cfg" / "target" / "release" / "runner", b"old", T_OLD)
            write_bytes(target / "proa-config" / "new-cfg" / "stamp-cache.json", b"{}", T_FRESH)

            stats = prune_cache.prune_target_directory(
                target,
                since_ns=T_SINCE,
                keep_metadata=2,
            )

            self.assertEqual(stats.active_scope, "newscope00000002")
            self.assertFalse((rsjs / "shared" / "oldscope00000001").exists())
            self.assertFalse((rsjs / "final-target" / "oldscope00000001").exists())
            self.assertTrue(active_desc.exists())
            self.assertFalse((active_shared / "descriptor-target-policy-v1-persistent-v1").exists())
            self.assertFalse((active_shared / "descriptor-cache-v4").exists())
            self.assertTrue((active_shared / "descriptor-cache-v5" / "entry.bin").exists())
            self.assertTrue(active_final.exists())
            self.assertFalse((active_final_root / "rsjs-final" / "wrapper-v1").exists())
            self.assertFalse((active_final_root / "rsjs-final-artifact-snapshots").exists())
            self.assertFalse((rsjs / "inv-deadbeef").exists())
            self.assertTrue((rsjs / ".proa-rsjs-target-locks").exists())
            self.assertFalse((target / "proa-output").exists())
            self.assertFalse((target / "cargo-timings").exists())
            self.assertFalse((active_desc / "cargo-timings").exists())
            self.assertFalse((active_final / "cargo-timings").exists())
            self.assertFalse((target / "release-fast" / "incremental").exists())
            self.assertFalse((target / "proa-config" / "old-cfg").exists())
            self.assertTrue((target / "proa-config" / "new-cfg").exists())
            self.assertEqual(
                sorted(p.name for p in (rsjs / "plans").iterdir()),
                ["semantic-4.json", "semantic-5.json"],
            )
            self.assertEqual(
                sorted(p.name for p in (active_final_root / "rsjs-final-unit-maps").iterdir()),
                ["map-4.json", "map-5.json"],
            )
            self.assertGreater(stats.freed_bytes, 0)

    def test_wrapper_snapshot_pruning_prefers_exact_wrapper_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "target"
            wrapper = write_bytes(target / "release-fast" / "proa", b"current-proa-binary", T_OLD)
            digest = hashlib.sha256(b"current-proa-binary").hexdigest()

            snap_root = target / "proa-rsjs" / "wrapper-snapshots-v1"
            keep_snap = snap_root / f"{digest}-755"
            stale_snap = snap_root / ("0" * 64 + "-755")
            write_bytes(keep_snap / "proa", b"current-proa-binary", T_OLD)
            # Even if the stale snapshot has a newer mtime, exact digest wins.
            write_bytes(stale_snap / "proa", b"stale-proa-binary", T_FRESH)

            prune_cache.prune_target_directory(target, wrapper_path=wrapper)
            self.assertTrue(keep_snap.exists())
            self.assertFalse(stale_snap.exists())

    def test_prunes_only_superseded_units_matching_same_package_and_kind(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "target"
            profile = target / "release-fast"

            # Rebuilt binary target for hyphenated package proa-cli:
            # old hash must be pruned, new hash kept.
            make_cargo_unit(
                profile,
                "proa-cli",
                "1111111111111111",
                kind_json="bin-proa.json",
                mtime_ns=T_OLD,
                dep_files=[("proa_cli-1111111111111111", 8192), ("proa_cli-1111111111111111.d", 64)],
            )
            make_cargo_unit(
                profile,
                "proa-cli",
                "2222222222222222",
                kind_json="bin-proa.json",
                mtime_ns=T_FRESH,
                dep_files=[("proa_cli-2222222222222222", 8192), ("proa_cli-2222222222222222.d", 64)],
            )
            # Unchanged library & build-script targets of the SAME package must NOT be pruned.
            make_cargo_unit(
                profile,
                "proa-cli",
                "3333333333333333",
                kind_json="lib-proa_cli.json",
                mtime_ns=T_OLD,
                dep_files=[("libproa_cli-3333333333333333.rlib", 4096)],
            )
            make_cargo_unit(
                profile,
                "proa-cli",
                "4444444444444444",
                kind_json="run-build-script-build-script-build.json",
                mtime_ns=T_OLD,
                with_build_dir=True,
            )

            # Cargo reused one version of syn and rebuilt the other.
            make_cargo_unit(
                profile,
                "syn",
                "aaaaaaaaaaaaaaaa",
                kind_json="lib-syn.json",
                mtime_ns=T_OLD,
                dep_files=[("libsyn-aaaaaaaaaaaaaaaa.rlib", 2048)],
            )
            make_cargo_unit(
                profile,
                "syn",
                "bbbbbbbbbbbbbbbb",
                kind_json="lib-syn.json",
                mtime_ns=T_FRESH,
                dep_files=[("libsyn-bbbbbbbbbbbbbbbb.rlib", 2048)],
            )

            # Old orphan artifact in deps/ and build/ with no .fingerprint entry
            write_bytes(profile / "deps" / "liborphan-9999999999999999.rlib", b"o" * 1024, T_OLD)
            write_bytes(profile / "build" / "orphan-9999999999999999" / "output", b"o", T_OLD)

            fp_root = (profile / ".fingerprint").resolve()
            current = Path(tmp) / "cargo-fingerprints"
            current.write_text(
                f"{fp_root}/proa-cli-2222222222222222/bin-proa\n"
                f"{fp_root}/syn-aaaaaaaaaaaaaaaa/lib-syn\n"
                f"{fp_root}/syn-bbbbbbbbbbbbbbbb/lib-syn\n"
            )
            stats = prune_cache.prune_target_directory(
                target, since_ns=T_SINCE, cargo_fingerprints=current
            )

            self.assertEqual(stats.superseded_units, 1)
            self.assertEqual(stats.orphan_entries, 2)
            self.assertFalse((profile / ".fingerprint" / "proa-cli-1111111111111111").exists())
            self.assertFalse((profile / "deps" / "proa_cli-1111111111111111").exists())
            self.assertTrue((profile / ".fingerprint" / "proa-cli-2222222222222222").exists())
            self.assertTrue((profile / "deps" / "proa_cli-2222222222222222").exists())
            self.assertTrue((profile / ".fingerprint" / "proa-cli-3333333333333333").exists())
            self.assertTrue((profile / "deps" / "libproa_cli-3333333333333333.rlib").exists())
            self.assertTrue((profile / "build" / "proa-cli-4444444444444444").exists())
            self.assertTrue((profile / "deps" / "libsyn-aaaaaaaaaaaaaaaa.rlib").exists())
            self.assertTrue((profile / "deps" / "libsyn-bbbbbbbbbbbbbbbb.rlib").exists())
            self.assertFalse((profile / "deps" / "liborphan-9999999999999999.rlib").exists())
            self.assertFalse((profile / "build" / "orphan-9999999999999999").exists())

    @unittest.skipUnless(shutil.which("cargo") and shutil.which("awk"), "requires Cargo and awk")
    def test_warm_cargo_build_reuses_both_versions_after_pruning(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            crates = (("v1", "shared", 1), ("v2", "shared", 2), ("app", "app", 1))
            for directory, package, version in crates:
                crate = root / directory
                (crate / "src").mkdir(parents=True)
                (crate / "Cargo.toml").write_text(
                    f'[package]\nname="{package}"\nversion="{version}.0.0"\nedition="2021"\n'
                )
            for directory in ("v1", "v2"):
                (root / directory / "src/lib.rs").write_text("pub fn value() -> u32 { 1 }")
            app = root / "app"
            with (app / "Cargo.toml").open("a") as manifest:
                manifest.write(
                    '[dependencies]\n'
                    'v1={package="shared",path="../v1"}\n'
                    'v2={package="shared",path="../v2"}\n'
                )
            (app / "src/main.rs").write_text("fn main() { assert!(v1::value() + v2::value() > 0); }")
            target = root / "target"
            fingerprints = root / "cargo-fingerprints"

            def build() -> dict[str, bool]:
                result = subprocess.run(
                    [
                        "cargo", "build", "--offline", "--release", "--manifest-path",
                        str(app / "Cargo.toml"), "--message-format=json-render-diagnostics",
                    ],
                    cwd=repo,
                    env={
                        **os.environ, "CARGO_TARGET_DIR": str(target), "CARGO_TERM_COLOR": "never",
                        "CARGO_LOG": "cargo::core::compiler::fingerprint=debug",
                    },
                    check=True, capture_output=True, text=True,
                )
                fingerprints.write_text("")
                subprocess.run(
                    [
                        "awk", "-v", f"fingerprints={fingerprints}", "-f",
                        str(repo / "deploy/marketing/cargo-fingerprints.awk"),
                    ],
                    input=result.stderr, check=True, capture_output=True, text=True,
                )
                return {
                    Path(message["manifest_path"]).parent.name: message["fresh"]
                    for message in map(json.loads, result.stdout.splitlines())
                    if message["reason"] == "compiler-artifact"
                }

            build()
            marker = root / "compile-start"
            marker.touch()
            (root / "v2/src/lib.rs").write_text("pub fn value() -> u32 { 2 }")
            self.assertEqual(build(), {"v1": True, "v2": False, "app": False})
            used = [Path(line) for line in fingerprints.read_text().splitlines()]
            self.assertEqual(len({path.parent for path in used if path.name == "lib-shared"}), 2)
            self.assertEqual(prune_cache.main([
                str(target), "--since-file", str(marker), "--cargo-fingerprints", str(fingerprints)
            ]), 0)
            self.assertEqual(build(), {"v1": True, "v2": True, "app": True})

    def test_budget_evicts_unused_units_before_warm_dependencies(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "target"
            profile = target / "release"

            make_cargo_unit(
                profile,
                "oldest_crate",
                "1111111111111111",
                kind_json="lib-oldest_crate.json",
                mtime_ns=T_OLD,
                dep_files=[("liboldest_crate-1111111111111111.rlib", 10_000)],
            )
            make_cargo_unit(
                profile,
                "middle_crate",
                "2222222222222222",
                kind_json="lib-middle_crate.json",
                mtime_ns=T_MID,
                dep_files=[("libmiddle_crate-2222222222222222.rlib", 10_000)],
            )
            make_cargo_unit(
                profile,
                "fresh_crate",
                "3333333333333333",
                kind_json="lib-fresh_crate.json",
                mtime_ns=T_FRESH,
                dep_files=[("libfresh_crate-3333333333333333.rlib", 10_000)],
            )

            current = Path(tmp) / "cargo-fingerprints"
            current.write_text(
                f"{profile.resolve()}/.fingerprint/oldest_crate-1111111111111111/lib-oldest_crate\n"
            )
            stats = prune_cache.prune_target_directory(
                target,
                since_ns=T_SINCE,
                max_bytes=22_000,
                cargo_fingerprints=current,
            )

            self.assertEqual(stats.budget_evicted_units, 1)
            self.assertLessEqual(stats.bytes_after, 22_000)
            self.assertTrue((profile / "deps" / "liboldest_crate-1111111111111111.rlib").exists())
            self.assertFalse((profile / "deps" / "libmiddle_crate-2222222222222222.rlib").exists())
            self.assertTrue((profile / "deps" / "libfresh_crate-3333333333333333.rlib").exists())

    def test_over_budget_resets_descriptor_generations_and_matching_target_together(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "target"
            rsjs = target / "proa-rsjs"
            shared = rsjs / "shared" / "scope"
            cache = shared / "descriptor-cache-v5"
            descriptor = shared / "descriptor-target-policy-v2"
            for generation, mtime in (("a", T_OLD), ("b", T_FRESH)):
                write_bytes(cache / "objects" / "unit" / f"{generation}.o", b"x" * 2048, mtime)
                write_bytes(cache / "units" / "unit" / f"{generation}.json", b"sidecar", mtime)
            write_bytes(cache / "artifacts" / "index.json", b"index", T_FRESH)
            write_bytes(descriptor / "release" / "deps" / "artifact", b"compiled", T_FRESH)
            final = write_bytes(
                rsjs / "final-target" / "scope" / "rsjs-final" / "wrapper-v1" / "release" / "bin",
                b"final", T_FRESH,
            )

            within = prune_cache.prune_target_directory(target, max_bytes=8192)
            self.assertFalse(within.descriptor_cache_reset)
            self.assertTrue((cache / "objects" / "unit" / "a.o").is_file())
            self.assertTrue(descriptor.is_dir())

            stats = prune_cache.prune_target_directory(target, since_ns=T_SINCE, max_bytes=1024)
            self.assertTrue(stats.descriptor_cache_reset)
            self.assertEqual(stats.bytes_after, len(b"final"))
            self.assertFalse(cache.exists())
            self.assertFalse(descriptor.exists())
            self.assertEqual(final.read_bytes(), b"final")

            # An oversized final output survives for deployment, but cannot be archived.
            self.assertEqual(prune_cache.main([str(target), "--max-bytes", "4"]), 3)
            self.assertEqual(prune_cache.main([str(target), "--max-bytes", "5"]), 0)
            self.assertEqual(final.read_bytes(), b"final")

    def test_cli_since_file_and_error_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            target = root / "target"
            profile = target / "release"
            since_file = write_bytes(root / ".compile-start", b"", T_SINCE)

            make_cargo_unit(
                profile,
                "demo",
                "1111111111111111",
                kind_json="lib-demo.json",
                mtime_ns=T_OLD,
                dep_files=[("libdemo-1111111111111111.rlib", 1024)],
            )
            make_cargo_unit(
                profile,
                "demo",
                "2222222222222222",
                kind_json="lib-demo.json",
                mtime_ns=T_FRESH,
                dep_files=[("libdemo-2222222222222222.rlib", 1024)],
            )

            exit_code = prune_cache.main([str(target), "--since-file", str(since_file)])
            self.assertEqual(exit_code, 0)
            # Without current Cargo evidence, another version rebuilding is ambiguous.
            self.assertTrue((profile / "deps" / "libdemo-1111111111111111.rlib").exists())
            self.assertTrue((profile / "deps" / "libdemo-2222222222222222.rlib").exists())

            self.assertEqual(prune_cache.main(["/"]), 1)
            self.assertEqual(prune_cache.main([str(root / "missing")]), 1)


if __name__ == "__main__":
    unittest.main()
