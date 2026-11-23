# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Unit tests for the marketing Cloud Build Cargo/RSJS cache pruner."""

from __future__ import annotations

import hashlib
import os
from pathlib import Path
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

            # Multi-version unchanged dependency (e.g. syn 1.x and syn 2.x): both MUST be kept.
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
                mtime_ns=T_MID,
                dep_files=[("libsyn-bbbbbbbbbbbbbbbb.rlib", 2048)],
            )

            # Old orphan artifact in deps/ and build/ with no .fingerprint entry
            write_bytes(profile / "deps" / "liborphan-9999999999999999.rlib", b"o" * 1024, T_OLD)
            write_bytes(profile / "build" / "orphan-9999999999999999" / "output", b"o", T_OLD)

            stats = prune_cache.prune_target_directory(target, since_ns=T_SINCE)

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

    def test_enforces_size_budget_by_evicting_oldest_non_fresh_units(self) -> None:
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

            stats = prune_cache.prune_target_directory(
                target,
                since_ns=T_SINCE,
                max_bytes=22_000,
            )

            self.assertEqual(stats.budget_evicted_units, 1)
            self.assertLessEqual(stats.bytes_after, 22_000)
            self.assertFalse((profile / "deps" / "liboldest_crate-1111111111111111.rlib").exists())
            self.assertTrue((profile / "deps" / "libmiddle_crate-2222222222222222.rlib").exists())
            self.assertTrue((profile / "deps" / "libfresh_crate-3333333333333333.rlib").exists())

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
            self.assertFalse((profile / "deps" / "libdemo-1111111111111111.rlib").exists())
            self.assertTrue((profile / "deps" / "libdemo-2222222222222222.rlib").exists())

            self.assertEqual(prune_cache.main(["/"]), 1)
            self.assertEqual(prune_cache.main([str(root / "missing")]), 1)


if __name__ == "__main__":
    unittest.main()
