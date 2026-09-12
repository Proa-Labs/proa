// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use proa_cli::rsjs::build::{
    finish_descriptor_run, run_descriptor_cargo, DescriptorCargoRequest, DescriptorCargoRunError,
    DescriptorCargoUnitReport,
};
use proa_cli::rsjs::cache::{run_descriptor_cargo_cached, DescriptorCacheError};
use tempfile::TempDir;

fn cargo_program() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn request(root: &Path, staging: PathBuf, package: Option<&str>) -> DescriptorCargoRequest {
    let mut cargo_args = vec![OsString::from("build"), OsString::from("--offline")];
    if let Some(package) = package {
        cargo_args.push(OsString::from("-p"));
        cargo_args.push(OsString::from(package));
    }
    DescriptorCargoRequest {
        cargo: cargo_program(),
        wrapper: PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
        working_directory: root.to_path_buf(),
        staging_directory: staging,
        target_directory: root.join("target/proa-test/descriptor-target"),
        source_exclusion_directory: root.join("target"),
        cargo_args,
        environment: BTreeMap::new(),
    }
}

fn unit_for_package<'a>(
    units: &'a [DescriptorCargoUnitReport],
    package: &str,
) -> &'a DescriptorCargoUnitReport {
    units
        .iter()
        .find(|unit| unit.cargo_package_name.as_deref() == Some(package))
        .unwrap_or_else(|| panic!("missing descriptor unit for {package}"))
}

/// The unit for one exact crate. A package with a build script reports a
/// `build_script_build` unit next to its lib unit, and `units` is ordered by
/// unit-id bytes, so positional selection would pick a platform-dependent
/// unit: the ids embed HOST/TARGET/rustc, flipping the sort order between
/// hosts.
fn unit_for_crate<'a>(
    units: &'a [DescriptorCargoUnitReport],
    crate_name: &str,
) -> &'a DescriptorCargoUnitReport {
    units
        .iter()
        .find(|unit| unit.crate_name == crate_name)
        .unwrap_or_else(|| panic!("missing descriptor unit for crate {crate_name}"))
}

fn write_noop_fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"descriptor_cache_noop\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn cached() -> u32 { 7 }\n").unwrap();
}

fn write_out_dir_fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"descriptor_cache_out_dir\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n\n[workspace]\n",
    )
    .unwrap();
    fs::write(
        root.join("build.rs"),
        r#"
fn main() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let mode = std::env::var("GEN_MODE").unwrap();
    std::fs::write(
        out.join("generated.rs"),
        format!("pub const GENERATED: &str = {mode:?};\n"),
    )
    .unwrap();
    println!("cargo::rerun-if-env-changed=GEN_MODE");
    println!("cargo::rerun-if-changed=build.rs");
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\npub fn generated() -> &'static str { GENERATED }\n",
    )
    .unwrap();
}

fn cached_generation_file(
    cache: &Path,
    kind: &str,
    unit: proa_cli::rsjs::build::DescriptorCargoUnitId,
    extension: &str,
) -> PathBuf {
    let directory = cache.join(kind).join(unit.to_string());
    let mut paths = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("failed to read `{}`: {error}", directory.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some(extension))
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(
        paths.len(),
        1,
        "expected one immutable {kind} generation for {unit}, found {paths:?}"
    );
    paths.pop().unwrap()
}

fn only_file(directory: &Path) -> PathBuf {
    let mut files = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read `{}`: {error}", directory.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(files.len(), 1, "expected one file in {directory:?}");
    files.pop().unwrap()
}

fn target_dirty_journal(target: &Path) -> PathBuf {
    let lock_directory = target.parent().unwrap().join(".proa-rsjs-target-locks");
    let mut locks = fs::read_dir(&lock_directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("lock"))
        .collect::<Vec<_>>();
    locks.sort();
    assert_eq!(
        locks.len(),
        1,
        "expected one target lock in {lock_directory:?}"
    );
    let lock = locks.pop().unwrap();
    lock.with_extension("dirty.json")
}

fn write_dirty_generation(target: &Path, staging: &Path) -> PathBuf {
    let journal = target_dirty_journal(target);
    fs::write(
        &journal,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 5,
            "target_directory": target,
            "staging_directory": staging,
        }))
        .unwrap(),
    )
    .unwrap();
    journal
}

fn leave_completed_interrupted_generation(
    root: &Path,
    staging: PathBuf,
) -> proa_cli::rsjs::build::DescriptorCargoReport {
    let request = request(root, staging.clone(), None);
    let target = request.target_directory.clone();
    let report = finish_descriptor_run(run_descriptor_cargo(&request).unwrap());
    assert!(staging.is_dir());
    write_dirty_generation(&target, &staging);
    report
}

#[test]
fn identical_noop_build_recovers_exact_units_from_the_explicit_cache() {
    let temp = TempDir::new().unwrap();
    write_noop_fixture(temp.path());

    let cache = temp.path().join("target/proa-test/descriptor-cache");
    let first_staging = temp.path().join("target/proa-test/descriptor-staging-1");
    let first = finish_descriptor_run(
        run_descriptor_cargo_cached(&request(temp.path(), first_staging.clone(), None), &cache)
            .unwrap(),
    );
    assert_eq!(first.units.len(), 1);
    assert!(first.cargo_artifacts.iter().any(|artifact| !artifact.fresh));
    let first_unit = first.units[0].unit;

    let second_staging = temp.path().join("target/proa-test/descriptor-staging-2");
    let second = finish_descriptor_run(
        run_descriptor_cargo_cached(&request(temp.path(), second_staging.clone(), None), &cache)
            .unwrap(),
    );
    assert_eq!(
        second
            .units
            .iter()
            .map(|unit| unit.unit)
            .collect::<Vec<_>>(),
        vec![first_unit]
    );
    assert!(second.cargo_artifacts.iter().all(|artifact| artifact.fresh));
    assert!(
        !second_staging.exists(),
        "successful cache hits must remove nonce staging"
    );
    assert!(cached_generation_file(&cache, "objects", first_unit, "o").is_file());
    assert!(cached_generation_file(&cache, "units", first_unit, "json").is_file());

    let third = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path()
                    .join("target/proa-test/descriptor-staging-warm-3"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );
    assert_eq!(third.units[0].unit, first_unit);
    assert!(third.cargo_artifacts.iter().all(|artifact| artifact.fresh));

    let cached_sidecar = cached_generation_file(&cache, "units", first_unit, "json");
    let original_sidecar = fs::read(&cached_sidecar).unwrap();
    let mut sidecar_json: serde_json::Value = serde_json::from_slice(&original_sidecar).unwrap();
    sidecar_json["descriptor_object_hash"] = serde_json::Value::String("00".repeat(32));
    fs::write(
        &cached_sidecar,
        serde_json::to_vec_pretty(&sidecar_json).unwrap(),
    )
    .unwrap();
    let error = run_descriptor_cargo_cached(
        &request(
            temp.path(),
            temp.path().join("target/proa-test/descriptor-staging-3"),
            None,
        ),
        &cache,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        DescriptorCacheError::CachedSidecarHashMismatch { unit, .. } if unit == first_unit
    ));
    fs::write(&cached_sidecar, original_sidecar).unwrap();
}

#[test]
fn completed_interrupted_generation_reuses_target_and_staged_unit() {
    let temp = TempDir::new().unwrap();
    write_noop_fixture(temp.path());
    let cache = temp.path().join("target/proa-test/descriptor-cache");
    finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path()
                    .join("target/proa-test/descriptor-staging-initial"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );

    fs::write(
        temp.path().join("src/lib.rs"),
        "pub fn cached() -> u32 { 8 }\n",
    )
    .unwrap();
    let interrupted_staging = temp
        .path()
        .join("target/proa-test/descriptor-staging-interrupted");
    let interrupted =
        leave_completed_interrupted_generation(temp.path(), interrupted_staging.clone());
    assert!(interrupted
        .cargo_artifacts
        .iter()
        .any(|artifact| !artifact.fresh));
    let interrupted_unit = interrupted.units[0].unit;

    let unused_new_staging = temp
        .path()
        .join("target/proa-test/descriptor-staging-unused");
    let recovered = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(temp.path(), unused_new_staging.clone(), None),
            &cache,
        )
        .unwrap(),
    );
    assert!(recovered
        .cargo_artifacts
        .iter()
        .all(|artifact| artifact.fresh));
    assert_eq!(recovered.units[0].unit, interrupted_unit);
    assert!(!interrupted_staging.exists());
    assert!(!unused_new_staging.exists());
    assert!(
        !target_dirty_journal(&temp.path().join("target/proa-test/descriptor-target")).exists()
    );
}

#[test]
fn journal_left_after_completed_commit_reuses_snapshot_without_rebuilding() {
    let temp = TempDir::new().unwrap();
    write_noop_fixture(temp.path());
    let cache = temp.path().join("target/proa-test/descriptor-cache");
    let target = temp.path().join("target/proa-test/descriptor-target");
    finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path()
                    .join("target/proa-test/descriptor-staging-initial"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );

    let already_removed_staging = temp
        .path()
        .join("target/proa-test/descriptor-staging-already-removed");
    write_dirty_generation(&target, &already_removed_staging);
    let recovered = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path()
                    .join("target/proa-test/descriptor-staging-unused"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );
    assert!(recovered
        .cargo_artifacts
        .iter()
        .all(|artifact| artifact.fresh));
    assert!(!already_removed_staging.exists());
    assert!(!target_dirty_journal(&target).exists());
}

#[test]
fn source_change_after_interruption_discards_stale_staged_unit() {
    let temp = TempDir::new().unwrap();
    write_noop_fixture(temp.path());
    let cache = temp.path().join("target/proa-test/descriptor-cache");
    finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path()
                    .join("target/proa-test/descriptor-staging-initial"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );

    fs::write(
        temp.path().join("src/lib.rs"),
        "pub fn cached() -> u32 { 8 }\n",
    )
    .unwrap();
    let interrupted_staging = temp
        .path()
        .join("target/proa-test/descriptor-staging-interrupted");
    let interrupted =
        leave_completed_interrupted_generation(temp.path(), interrupted_staging.clone());
    let stale_unit = interrupted.units[0].unit;

    fs::write(
        temp.path().join("src/lib.rs"),
        "pub fn cached() -> u32 { 9 }\n",
    )
    .unwrap();
    let recovered = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path()
                    .join("target/proa-test/descriptor-staging-unused"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );
    assert!(recovered
        .cargo_artifacts
        .iter()
        .any(|artifact| !artifact.fresh));
    assert_ne!(recovered.units[0].unit, stale_unit);
    assert!(!interrupted_staging.exists());
}

#[test]
fn out_dir_change_after_interruption_replaces_same_unit_generation() {
    let temp = TempDir::new().unwrap();
    write_out_dir_fixture(temp.path());
    let cache = temp.path().join("target/proa-test/descriptor-cache");
    let request_for_mode = |staging: &str, mode: &str| {
        let mut request = request(
            temp.path(),
            temp.path().join("target/proa-test").join(staging),
            None,
        );
        request
            .environment
            .insert(OsString::from("GEN_MODE"), OsString::from(mode));
        request
    };

    let initial = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request_for_mode("descriptor-staging-initial", "v1"),
            &cache,
        )
        .unwrap(),
    );
    let interrupted_staging = temp
        .path()
        .join("target/proa-test/descriptor-staging-interrupted");
    let interrupted_request = request_for_mode("descriptor-staging-interrupted", "v2");
    let interrupted = finish_descriptor_run(run_descriptor_cargo(&interrupted_request).unwrap());
    write_dirty_generation(&interrupted_request.target_directory, &interrupted_staging);

    let initial_lib = unit_for_crate(&initial.units, "descriptor_cache_out_dir");
    let interrupted_lib = unit_for_crate(&interrupted.units, "descriptor_cache_out_dir");
    assert_eq!(initial_lib.unit, interrupted_lib.unit);
    assert_ne!(
        initial_lib.descriptor_object_hash,
        interrupted_lib.descriptor_object_hash
    );

    let recovered = finish_descriptor_run(
        run_descriptor_cargo_cached(&request_for_mode("descriptor-staging-unused", "v3"), &cache)
            .unwrap(),
    );
    let recovered_lib = unit_for_crate(&recovered.units, "descriptor_cache_out_dir");
    assert_eq!(recovered_lib.unit, interrupted_lib.unit);
    assert_ne!(
        recovered_lib.descriptor_object_hash,
        interrupted_lib.descriptor_object_hash
    );
    assert!(recovered
        .cargo_artifacts
        .iter()
        .any(|artifact| !artifact.fresh));
    assert!(!interrupted_staging.exists());
}

#[test]
fn repeated_interruption_reuses_a_completed_recovery_attempt() {
    let temp = TempDir::new().unwrap();
    write_out_dir_fixture(temp.path());
    let cache = temp.path().join("target/proa-test/descriptor-cache");
    let request_for_mode = |staging: PathBuf, mode: &str| {
        let mut request = request(temp.path(), staging, None);
        request
            .environment
            .insert(OsString::from("GEN_MODE"), OsString::from(mode));
        request
    };

    finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request_for_mode(
                temp.path()
                    .join("target/proa-test/descriptor-staging-initial"),
                "v1",
            ),
            &cache,
        )
        .unwrap(),
    );
    let target = temp.path().join("target/proa-test/descriptor-target");
    let interrupted_staging = temp
        .path()
        .join("target/proa-test/descriptor-staging-interrupted");
    let first_interrupted = request_for_mode(interrupted_staging.clone(), "v2");
    finish_descriptor_run(run_descriptor_cargo(&first_interrupted).unwrap());
    let journal = write_dirty_generation(&target, &interrupted_staging);

    // Model a recovery process that completed Cargo's isolated writer attempt
    // and then died before committing cache indexes or clearing the journal.
    fs::remove_file(&journal).unwrap();
    let completed_attempt = interrupted_staging
        .join(".proa-rsjs-recovery-attempts")
        .join("attempt-123-0");
    let second_interrupted = request_for_mode(completed_attempt, "v3");
    let second = finish_descriptor_run(run_descriptor_cargo(&second_interrupted).unwrap());
    write_dirty_generation(&target, &interrupted_staging);

    let recovered = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request_for_mode(
                temp.path()
                    .join("target/proa-test/descriptor-staging-unused"),
                "v3",
            ),
            &cache,
        )
        .unwrap(),
    );
    assert!(recovered
        .cargo_artifacts
        .iter()
        .all(|artifact| artifact.fresh));
    assert_eq!(
        unit_for_crate(&recovered.units, "descriptor_cache_out_dir").descriptor_object_hash,
        unit_for_crate(&second.units, "descriptor_cache_out_dir").descriptor_object_hash
    );
    assert!(!interrupted_staging.exists());
    assert!(!target_dirty_journal(&target).exists());
}

#[test]
fn interrupted_atomic_sidecar_temporary_does_not_block_exact_recovery() {
    let temp = TempDir::new().unwrap();
    write_noop_fixture(temp.path());
    let cache = temp.path().join("target/proa-test/descriptor-cache");
    finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path()
                    .join("target/proa-test/descriptor-staging-initial"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );

    fs::write(
        temp.path().join("src/lib.rs"),
        "pub fn cached() -> u32 { 8 }\n",
    )
    .unwrap();
    let interrupted_staging = temp
        .path()
        .join("target/proa-test/descriptor-staging-interrupted");
    let interrupted =
        leave_completed_interrupted_generation(temp.path(), interrupted_staging.clone());
    let unit = interrupted.units[0].unit;
    let sidecar = interrupted_staging
        .join("units")
        .join(format!("{unit}.json"));
    fs::write(
        interrupted_staging
            .join("units")
            .join(format!(".{unit}.json.123.0.tmp")),
        fs::read(sidecar).unwrap(),
    )
    .unwrap();

    let recovered = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path()
                    .join("target/proa-test/descriptor-staging-unused"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );
    assert!(recovered
        .cargo_artifacts
        .iter()
        .all(|artifact| artifact.fresh));
    assert_eq!(recovered.units[0].unit, unit);
}

#[test]
fn corrupted_interrupted_object_is_a_hard_failure_without_target_deletion() {
    let temp = TempDir::new().unwrap();
    write_noop_fixture(temp.path());
    let cache = temp.path().join("target/proa-test/descriptor-cache");
    finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path()
                    .join("target/proa-test/descriptor-staging-initial"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );

    fs::write(
        temp.path().join("src/lib.rs"),
        "pub fn cached() -> u32 { 8 }\n",
    )
    .unwrap();
    let interrupted_staging = temp
        .path()
        .join("target/proa-test/descriptor-staging-interrupted");
    let interrupted =
        leave_completed_interrupted_generation(temp.path(), interrupted_staging.clone());
    let unit = interrupted.units[0].unit;
    fs::write(
        interrupted_staging
            .join("objects")
            .join(format!("{unit}.o")),
        b"corrupt interrupted descriptor object",
    )
    .unwrap();

    let error = run_descriptor_cargo_cached(
        &request(
            temp.path(),
            temp.path()
                .join("target/proa-test/descriptor-staging-unused"),
            None,
        ),
        &cache,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        DescriptorCacheError::DescriptorCargo(DescriptorCargoRunError::Sidecars(_))
    ));
    assert!(interrupted_staging.is_dir());
    assert!(temp
        .path()
        .join("target/proa-test/descriptor-target")
        .is_dir());
    assert!(
        target_dirty_journal(&temp.path().join("target/proa-test/descriptor-target")).is_file()
    );
}

#[test]
fn corrupt_generation_object_is_a_hard_failure() {
    let temp = TempDir::new().unwrap();
    write_noop_fixture(temp.path());
    let cache = temp.path().join("target/proa-test/descriptor-cache");
    let first = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path().join("target/proa-test/descriptor-staging-1"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );
    let unit = first.units[0].unit;
    let object = cached_generation_file(&cache, "objects", unit, "o");
    fs::write(&object, b"corrupt descriptor object").unwrap();
    let error = run_descriptor_cargo_cached(
        &request(
            temp.path(),
            temp.path().join("target/proa-test/descriptor-staging-2"),
            None,
        ),
        &cache,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        DescriptorCacheError::DescriptorObjectHashMismatch { unit: actual, .. } if actual == unit
    ));
}

#[test]
fn missing_generation_object_forces_one_owned_rebuild() {
    let temp = TempDir::new().unwrap();
    write_noop_fixture(temp.path());
    let cache = temp.path().join("target/proa-test/descriptor-cache");
    let first = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path().join("target/proa-test/descriptor-staging-1"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );
    let unit = first.units[0].unit;
    let object = cached_generation_file(&cache, "objects", unit, "o");
    fs::remove_file(&object).unwrap();

    let recovered = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path().join("target/proa-test/descriptor-staging-2"),
                None,
            ),
            &cache,
        )
        .unwrap(),
    );
    assert!(recovered
        .cargo_artifacts
        .iter()
        .all(|artifact| !artifact.fresh));
    assert_eq!(recovered.units[0].unit, unit);
    assert!(object.is_file());
}

#[test]
fn unobserved_assets_reuse_cache_and_crash_authority_recovers_once() {
    let temp = TempDir::new().unwrap();
    write_noop_fixture(temp.path());
    let docs = temp.path().join("src/docs/guide.mdx");
    let styles = temp.path().join("styles/site.css");
    fs::create_dir_all(docs.parent().unwrap()).unwrap();
    fs::create_dir_all(styles.parent().unwrap()).unwrap();
    fs::write(&docs, "# First draft\n").unwrap();
    fs::write(&styles, "body { color: black; }\n").unwrap();
    let cache_a = temp.path().join("target/proa-test/descriptor-cache-a");
    let first = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path().join("target/proa-test/descriptor-staging-1"),
                None,
            ),
            &cache_a,
        )
        .unwrap(),
    );
    let first_unit = first.units[0].unit;

    // These package-local assets are not rustc inputs. Cargo reports the unit
    // fresh, and the descriptor cache must preserve that result instead of
    // converting an ordinary docs/styles edit into a clean target rebuild.
    fs::write(&docs, "# Revised draft\n").unwrap();
    fs::write(&styles, "body { color: rebeccapurple; }\n").unwrap();
    let assets_reused = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path().join("target/proa-test/descriptor-staging-2"),
                None,
            ),
            &cache_a,
        )
        .unwrap(),
    );
    assert!(assets_reused
        .cargo_artifacts
        .iter()
        .all(|artifact| artifact.fresh));
    assert_eq!(assets_reused.units[0].unit, first_unit);

    let snapshot = only_file(&cache_a.join("snapshots"));
    fs::remove_file(&snapshot).unwrap();
    let snapshot_recovered = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path().join("target/proa-test/descriptor-staging-3"),
                None,
            ),
            &cache_a,
        )
        .unwrap(),
    );
    assert!(snapshot_recovered
        .cargo_artifacts
        .iter()
        .all(|artifact| !artifact.fresh));
    assert!(snapshot.is_file());

    // Simulate a process dying after publishing its target-scoped dirty
    // journal but before producing any recoverable unit evidence. A different
    // empty cache namespace sharing this exact target must see the same
    // journal, fail closed when exact reuse cannot be proven, and force one
    // marker-owned complete build.
    let target = temp.path().join("target/proa-test/descriptor-target");
    let interrupted_staging = temp.path().join("target/proa-test/interrupted-staging");
    fs::create_dir_all(&interrupted_staging).unwrap();
    fs::write(
        interrupted_staging.join(".proa-rsjs-descriptor-staging-v1"),
        b"proa-rsjs-descriptor-staging-v1\n",
    )
    .unwrap();
    let journal = target_dirty_journal(&target);
    fs::write(
        &journal,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 5,
            "target_directory": target,
            "staging_directory": interrupted_staging,
        }))
        .unwrap(),
    )
    .unwrap();

    let uncached_error = run_descriptor_cargo(&request(
        temp.path(),
        temp.path()
            .join("target/proa-test/descriptor-staging-uncached"),
        None,
    ))
    .unwrap_err();
    let DescriptorCargoRunError::CachedCrashJournalRequiresRecovery(error_journal) =
        &uncached_error
    else {
        panic!("unexpected uncached dirty-journal error: {uncached_error:?}");
    };
    assert_eq!(
        fs::canonicalize(error_journal).unwrap(),
        fs::canonicalize(&journal).unwrap(),
        "the refusal must identify the exact canonical target journal"
    );
    assert!(!temp
        .path()
        .join("target/proa-test/descriptor-staging-uncached")
        .exists());

    let cache_b = temp.path().join("target/proa-test/descriptor-cache-b");
    let cross_cache_recovered = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path().join("target/proa-test/descriptor-staging-4"),
                None,
            ),
            &cache_b,
        )
        .unwrap(),
    );
    assert!(cross_cache_recovered
        .cargo_artifacts
        .iter()
        .all(|artifact| !artifact.fresh));
    assert!(!interrupted_staging.exists());
    assert!(!journal.exists());
}

#[test]
fn one_leaf_edit_rebuilds_its_dependents_and_reuses_the_unchanged_unit() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"stable\", \"leaf\", \"app\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    for package in ["stable", "leaf", "app"] {
        fs::create_dir_all(temp.path().join(package).join("src")).unwrap();
    }
    fs::write(
        temp.path().join("stable/Cargo.toml"),
        "[package]\nname = \"descriptor_cache_stable\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("stable/src/lib.rs"),
        "pub fn stable() -> u32 { 3 }\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("leaf/Cargo.toml"),
        "[package]\nname = \"descriptor_cache_leaf\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("leaf/src/lib.rs"),
        "pub fn leaf() -> u32 { 5 }\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("app/Cargo.toml"),
        "[package]\nname = \"descriptor_cache_app\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\
         [dependencies]\n\
         descriptor_cache_stable = { path = \"../stable\" }\n\
         descriptor_cache_leaf = { path = \"../leaf\" }\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("app/src/lib.rs"),
        "pub fn app() -> u32 { descriptor_cache_stable::stable() + descriptor_cache_leaf::leaf() }\n",
    )
    .unwrap();

    let cache = temp.path().join("target/proa-test/descriptor-cache");
    let first = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                temp.path().join("target/proa-test/descriptor-staging-1"),
                Some("descriptor_cache_app"),
            ),
            &cache,
        )
        .unwrap(),
    );
    assert_eq!(first.units.len(), 3);
    let first_stable = unit_for_package(&first.units, "descriptor_cache_stable").unit;
    let first_leaf = unit_for_package(&first.units, "descriptor_cache_leaf").unit;
    let first_app = unit_for_package(&first.units, "descriptor_cache_app").unit;

    fs::write(
        temp.path().join("leaf/src/lib.rs"),
        "pub fn leaf() -> u32 { 11 }\n",
    )
    .unwrap();

    let second_staging = temp.path().join("target/proa-test/descriptor-staging-2");
    let second = finish_descriptor_run(
        run_descriptor_cargo_cached(
            &request(
                temp.path(),
                second_staging.clone(),
                Some("descriptor_cache_app"),
            ),
            &cache,
        )
        .unwrap(),
    );
    assert_eq!(second.units.len(), 3);
    let second_stable = unit_for_package(&second.units, "descriptor_cache_stable");
    let second_leaf = unit_for_package(&second.units, "descriptor_cache_leaf");
    let second_app = unit_for_package(&second.units, "descriptor_cache_app");
    assert_eq!(second_stable.unit, first_stable);
    assert_ne!(second_leaf.unit, first_leaf);
    assert_ne!(second_app.unit, first_app);
    assert!(
        !second_staging.exists(),
        "mixed fresh/cache runs must remove nonce staging after rebasing every unit to CAS"
    );
    assert!(cached_generation_file(&cache, "objects", second_stable.unit, "o").is_file());
    assert!(cached_generation_file(&cache, "units", second_stable.unit, "json").is_file());

    let cargo_freshness = second
        .cargo_artifacts
        .iter()
        .map(|artifact| (artifact.target_name.as_str(), artifact.fresh))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(cargo_freshness.get("descriptor_cache_stable"), Some(&true));
    assert_eq!(cargo_freshness.get("descriptor_cache_leaf"), Some(&false));
    assert_eq!(cargo_freshness.get("descriptor_cache_app"), Some(&false));

    assert!(cache.join("artifacts").read_dir().unwrap().next().is_some());
    assert!(cache.join("snapshots").read_dir().unwrap().next().is_some());
    assert!(cache.join("units").read_dir().unwrap().all(|entry| {
        let unit_directory = entry.unwrap().path();
        unit_directory.is_dir()
            && unit_directory.read_dir().unwrap().all(|entry| {
                entry
                    .unwrap()
                    .path()
                    .extension()
                    .and_then(|value| value.to_str())
                    == Some("json")
            })
    }));
}
