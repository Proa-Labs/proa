// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::rsjs::build::{
    CargoArtifactTarget, DescriptorCodegenSidecar, DescriptorDependencyArtifact,
    DescriptorUnitSidecar, DESCRIPTOR_STAGING_MARKER_BYTES, DESCRIPTOR_STAGING_MARKER_FILE,
    DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION,
};
use tempfile::TempDir;

fn artifact_fixture(
    temp: &TempDir,
    referenced: PathBuf,
    uncovered: PathBuf,
) -> CargoCompilerArtifact {
    let manifest = temp.path().join("Cargo.toml");
    let source = temp.path().join("src/lib.rs");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(
        &manifest,
        "[package]\nname='cache_evidence'\nversion='0.0.0'\n",
    )
    .unwrap();
    fs::write(&source, "pub fn fixture() {}\n").unwrap();
    CargoCompilerArtifact {
        package_id: "path+file:///cache_evidence#0.0.0".to_string(),
        manifest_path: manifest,
        target: CargoArtifactTarget {
            name: "cache_evidence".to_string(),
            kind: BTreeSet::from(["lib".to_string()]),
            crate_types: BTreeSet::from(["lib".to_string()]),
            src_path: source,
            edition: "2021".to_string(),
        },
        profile: serde_json::json!({"opt_level":"0","test":false}),
        features: BTreeSet::new(),
        filenames: BTreeSet::from([referenced.clone(), uncovered]),
        executable: Some(referenced),
        fresh: false,
    }
}

fn only_coordinate(artifact: &CargoCompilerArtifact) -> String {
    collect_artifact_evidence(std::slice::from_ref(artifact))
        .unwrap()
        .into_iter()
        .next()
        .unwrap()
        .coordinate
}

fn cached_unit_fixture(
    temp: &TempDir,
) -> (
    ValidatedDescriptorUnit,
    CargoCompilerArtifact,
    PathBuf,
    PathBuf,
) {
    let package = temp.path().join("cached-package");
    let source = package.join("src/lib.rs");
    let manifest = package.join("Cargo.toml");
    let descriptor_target = package.join(".build");
    let dependency = temp.path().join("target/debug/deps/libdependency.rlib");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::create_dir_all(&descriptor_target).unwrap();
    fs::create_dir_all(dependency.parent().unwrap()).unwrap();
    fs::write(&source, "pub fn value() -> u32 { 1 }\n").unwrap();
    fs::write(
        &manifest,
        "[package]\nname='cached_package'\nversion='0.0.0'\n",
    )
    .unwrap();
    fs::write(&dependency, b"extern-v1").unwrap();
    let dep_info = descriptor_target.join("debug/deps/cached_package.d");
    fs::create_dir_all(dep_info.parent().unwrap()).unwrap();
    fs::write(
        &dep_info,
        format!(
            "{}: {}\n\n{}:\n",
            dep_info.display(),
            source.display(),
            source.display()
        ),
    )
    .unwrap();

    let unit = DescriptorCargoUnitId::from_hex(&"11".repeat(32)).unwrap();
    let dependency_hash = descriptor_input_content_hash(&dependency).unwrap();
    let canonical_source = source.canonicalize().unwrap();
    let validated = ValidatedDescriptorUnit {
        unit,
        sidecar: DescriptorUnitSidecar {
            schema_version: DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION,
            cargo_unit_id: unit.to_string(),
            cargo_manifest_path: Some(manifest.clone()),
            cargo_package_name: Some("cached_package".to_string()),
            cargo_package_version: Some("0.0.0".to_string()),
            cargo_profile: Some("debug".to_string()),
            cargo_profile_fingerprint: "22".repeat(32),
            cargo_primary_package: true,
            cargo_extra_filename: None,
            crate_name: "cached_package".to_string(),
            crate_types: vec!["lib".to_string()],
            target: None,
            cfgs: vec![],
            features: vec![],
            source_hash: "33".repeat(32),
            package_source_closure_hash: descriptor_package_source_closure_hash(
                &package,
                &descriptor_target,
            )
            .unwrap(),
            dependency_artifacts: BTreeMap::from([(
                "dependency".to_string(),
                DescriptorDependencyArtifact {
                    path: dependency.clone(),
                    content_hash: dependency_hash,
                },
            )]),
            normalized_rustc_inputs_hash: "44".repeat(32),
            logical_environment_hash: "46".repeat(32),
            dep_info_path: "debug/deps/cached_package.d".to_string(),
            dep_info_working_directory: package.canonicalize().unwrap(),
            out_dir: None,
            out_dir_dependencies: vec![],
            source_dependencies: vec![DescriptorSourceDependency {
                absolute_path: canonical_source.clone(),
                content_hash: descriptor_source_dependency_content_hash(&canonical_source).unwrap(),
            }],
            environment_dependencies: vec![],
            realized_environment_hash: "45".repeat(32),
            descriptor_codegen: DescriptorCodegenSidecar {
                opt_level: 0,
                codegen_units: 1,
                incremental: false,
                link_dead_code: true,
                lto: "off".to_string(),
                strip: "none".to_string(),
            },
            descriptor_object: format!("objects/{unit}.o"),
            descriptor_object_hash: "55".repeat(32),
        },
        sidecar_path: temp.path().join(format!("units/{unit}.json")),
        descriptor_object: temp.path().join(format!("objects/{unit}.o")),
    };
    let cargo = CargoCompilerArtifact {
        package_id: "registry+https://example.invalid/index#dependency@1.0.0".to_string(),
        manifest_path: temp.path().join("dependency/Cargo.toml"),
        target: CargoArtifactTarget {
            name: "dependency".to_string(),
            kind: BTreeSet::from(["lib".to_string()]),
            crate_types: BTreeSet::from(["lib".to_string()]),
            src_path: temp.path().join("dependency/src/lib.rs"),
            edition: "2021".to_string(),
        },
        profile: serde_json::json!({"opt_level":"0","test":false}),
        features: BTreeSet::new(),
        filenames: BTreeSet::from([dependency]),
        executable: None,
        fresh: true,
    };
    (validated, cargo, source, manifest)
}

fn direct_cached_unit_fixture(
    temp: &TempDir,
) -> (
    PathBuf,
    CachedUnitGeneration,
    DescriptorCargoUnitId,
    PathBuf,
) {
    let (mut validated, _cargo, _source, _manifest) = cached_unit_fixture(temp);
    let cache = temp.path().join("cache");
    let descriptor_bytes = b"content-addressed descriptor object";
    let descriptor_source = temp.path().join("descriptor-source.o");
    fs::write(&descriptor_source, descriptor_bytes).unwrap();
    validated.sidecar.normalized_rustc_inputs_hash = validated.unit.to_string();
    validated.sidecar.descriptor_object_hash =
        descriptor_object_content_hash(&descriptor_source).unwrap();
    let sidecar_bytes = serde_json::to_vec(&validated.sidecar).unwrap();
    let generation = CachedUnitGeneration {
        cargo_unit_id: validated.unit.to_string(),
        descriptor_object_hash: validated.sidecar.descriptor_object_hash.clone(),
        sidecar_hash: cached_sidecar_hash(&sidecar_bytes),
    };
    let object = cached_object_path(&cache, &generation);
    let sidecar = cached_sidecar_path(&cache, &generation);
    fs::create_dir_all(object.parent().unwrap()).unwrap();
    fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
    fs::write(&object, descriptor_bytes).unwrap();
    fs::write(&sidecar, sidecar_bytes).unwrap();
    (cache, generation, validated.unit, object)
}

#[test]
fn cargo_coordinate_reads_no_outputs_and_covers_semantic_selectors() {
    let temp = TempDir::new().unwrap();
    let outputs = temp.path().join("target/debug/deps");
    fs::create_dir_all(&outputs).unwrap();
    let referenced = outputs.join("libreferenced.rlib");
    let uncovered = outputs.join("missing-unreferenced-64mib.rmeta");
    File::create(&referenced)
        .unwrap()
        .set_len(64 * 1024 * 1024)
        .unwrap();
    let artifact = artifact_fixture(&temp, referenced.clone(), uncovered);

    // A missing Cargo output is intentional: coordinate collection must be
    // independent of output I/O, even with a large neighboring output.
    let baseline = only_coordinate(&artifact);
    fs::write(
        &referenced,
        b"post-Cargo mutation outside the cache contract",
    )
    .unwrap();
    assert_eq!(only_coordinate(&artifact), baseline);

    let mut changed = artifact.clone();
    changed.features.insert("feature-a".to_string());
    assert_ne!(only_coordinate(&changed), baseline);

    let mut changed = artifact.clone();
    changed.profile = serde_json::json!({"test": false, "opt_level": "3"});
    assert_ne!(only_coordinate(&changed), baseline);

    let mut changed = artifact.clone();
    changed.manifest_path = temp.path().join("alternate/Cargo.toml");
    assert_ne!(only_coordinate(&changed), baseline);

    let mut changed = artifact.clone();
    changed.target.src_path = temp.path().join("alternate/src/lib.rs");
    assert_ne!(only_coordinate(&changed), baseline);

    let mut changed = artifact.clone();
    changed
        .filenames
        .insert(outputs.join("libadditional.rmeta"));
    assert_ne!(only_coordinate(&changed), baseline);
}

#[test]
fn zero_wrapper_units_cannot_reuse_snapshot_for_any_rebuilt_artifact() {
    let temp = TempDir::new().unwrap();
    let outputs = temp.path().join("target/debug/deps");
    fs::create_dir_all(&outputs).unwrap();
    let first_path = outputs.join("libfirst.rlib");
    let second_path = outputs.join("libsecond.rlib");
    fs::write(&first_path, b"first").unwrap();
    fs::write(&second_path, b"second").unwrap();
    let mut rebuilt = artifact_fixture(&temp, first_path, outputs.join("first.rmeta"));
    rebuilt.fresh = false;
    let mut fresh = artifact_fixture(&temp, second_path, outputs.join("second.rmeta"));
    fresh.target.name = "second".to_string();
    fresh.target.src_path = temp.path().join("src/second.rs");
    fresh.fresh = true;
    let evidence = collect_artifact_evidence(&[fresh, rebuilt]).unwrap();

    assert!(matches!(
        reject_rebuilt_artifact_without_wrapper_units(&evidence),
        Err(DescriptorCacheError::RebuiltArtifactMissingFreshUnit { .. })
    ));
}

#[test]
fn canonical_json_encoding_matches_the_exact_golden_bytes() {
    let value = serde_json::json!({
        "z": null,
        "a": [true, false, 1, -2.5, "雪\n"],
        "nested": {"b": "\u{0}", "a": 0}
    });
    let encoded = canonical_json(&value);
    let actual = encoded
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
            actual,
            "6f00000000000000030000000000000001616100000000000000057466643100642d322e3500730000000000000004e99baa0a00000000000000066e65737465646f00000000000000020000000000000001616430000000000000000001627300000000000000010000000000000000017a6e"
        );
}

#[test]
fn replaceable_pointer_can_be_committed_more_than_once() {
    let temp = TempDir::new().unwrap();
    let pointer = temp.path().join("cache/snapshots/current.json");

    atomic_write_replaceable(&pointer, br#"{"generation":"first"}"#).unwrap();
    atomic_write_replaceable(&pointer, br#"{"generation":"second"}"#).unwrap();

    assert_eq!(
        fs::read(&pointer).unwrap(),
        b"{\"generation\":\"second\"}\n"
    );
    assert_eq!(
        fs::read_dir(pointer.parent().unwrap()).unwrap().count(),
        1,
        "atomic replacement must not strand temporary siblings"
    );
}

#[test]
fn cached_unit_validation_ignores_unobserved_assets_and_rejects_exact_extern_changes() {
    let temp = TempDir::new().unwrap();
    let (unit, cargo, _source, manifest) = cached_unit_fixture(&temp);
    let cached_ids = BTreeSet::from([unit.unit]);
    let units = BTreeMap::from([(unit.unit, &unit)]);
    let descriptor_target = temp.path().join("cached-package/.build");
    let package = manifest.parent().unwrap();
    let docs = package.join("src/docs/guide.mdx");
    let styles = package.join("styles/site.css");
    fs::create_dir_all(docs.parent().unwrap()).unwrap();
    fs::create_dir_all(styles.parent().unwrap()).unwrap();
    fs::write(&docs, "# First draft\n").unwrap();
    fs::write(&styles, "body { color: black; }\n").unwrap();

    validate_cached_unit_inputs(
        &units,
        &cached_ids,
        &descriptor_target,
        std::slice::from_ref(&cargo),
    )
    .unwrap();

    fs::write(&docs, "# Revised draft\n").unwrap();
    fs::write(&styles, "body { color: rebeccapurple; }\n").unwrap();
    validate_cached_unit_inputs(
        &units,
        &cached_ids,
        &descriptor_target,
        std::slice::from_ref(&cargo),
    )
    .unwrap();

    let dependency = &unit.sidecar.dependency_artifacts["dependency"].path;
    fs::write(dependency, b"extern-v2").unwrap();
    assert!(matches!(
        validate_cached_unit_inputs(
            &units,
            &cached_ids,
            &descriptor_target,
            std::slice::from_ref(&cargo),
        ),
        Err(DescriptorCacheError::StaleCachedDependency { .. })
    ));
}

#[test]
fn cached_unit_validation_rejects_changed_consumed_out_dir_input() {
    let temp = TempDir::new().unwrap();
    let (mut unit, _cargo, _source, _manifest) = cached_unit_fixture(&temp);
    let descriptor_target = temp.path().join("cached-package/.build");
    let out_dir = descriptor_target.join("debug/build/fixture/out");
    let generated = out_dir.join("generated.rs");
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(&generated, b"pub const GENERATED: u8 = 1;\n").unwrap();
    unit.sidecar.out_dir = Some("debug/build/fixture/out".to_string());
    unit.sidecar.out_dir_dependencies = vec![DescriptorOutDirDependency {
        relative_path: "generated.rs".to_string(),
        content_hash: descriptor_out_dir_dependency_content_hash(&generated).unwrap(),
    }];
    let dep_info = descriptor_target.join(&unit.sidecar.dep_info_path);
    let source = &unit.sidecar.source_dependencies[0].absolute_path;
    fs::write(
        &dep_info,
        format!(
            "{}: {} {}\n\n{}:\n{}:\n",
            dep_info.display(),
            source.display(),
            generated.display(),
            source.display(),
            generated.display()
        ),
    )
    .unwrap();
    let cached_ids = BTreeSet::from([unit.unit]);
    let units = BTreeMap::from([(unit.unit, &unit)]);

    validate_cached_observed_inputs(&units, &cached_ids, &descriptor_target).unwrap();

    fs::write(&generated, b"pub const GENERATED: u8 = 2;\n").unwrap();
    assert!(matches!(
        validate_cached_observed_inputs(&units, &cached_ids, &descriptor_target),
        Err(DescriptorCacheError::CachedOutDirDependencyMismatch { .. })
    ));
}

#[test]
fn cached_observed_input_validation_rejects_changed_source_bytes() {
    let temp = TempDir::new().unwrap();
    let (unit, _cargo, source, _manifest) = cached_unit_fixture(&temp);
    let descriptor_target = temp.path().join("cached-package/.build");
    let cached_ids = BTreeSet::from([unit.unit]);
    let units = BTreeMap::from([(unit.unit, &unit)]);

    validate_cached_observed_inputs(&units, &cached_ids, &descriptor_target).unwrap();
    fs::write(&source, "pub fn value() -> u32 { 9 }\n").unwrap();
    assert!(matches!(
        validate_cached_observed_inputs(&units, &cached_ids, &descriptor_target),
        Err(DescriptorCacheError::CachedOutDirDependencyMismatch { .. })
    ));
}

#[test]
fn cached_observed_environment_validation_is_exact_and_value_redacted() {
    let temp = TempDir::new().unwrap();
    let (mut unit, _cargo, _source, _manifest) = cached_unit_fixture(&temp);
    let descriptor_target = temp.path().join("cached-package/.build");
    let dep_info = descriptor_target.join(&unit.sidecar.dep_info_path);
    let source = &unit.sidecar.source_dependencies[0].absolute_path;
    let first_secret = "first-sensitive-value";
    fs::write(
        &dep_info,
        format!(
            "{}: {}\n\n{}:\n# env-dep:PROA_CACHE_SECRET={}\n",
            dep_info.display(),
            source.display(),
            source.display(),
            first_secret
        ),
    )
    .unwrap();
    unit.sidecar.environment_dependencies = vec![DescriptorEnvironmentDependency {
        name: "PROA_CACHE_SECRET".to_string(),
        value_hash: Some(descriptor_environment_dependency_value_hash(
            "PROA_CACHE_SECRET",
            first_secret,
        )),
    }];
    assert!(!serde_json::to_string(&unit.sidecar)
        .unwrap()
        .contains(first_secret));
    let cached_ids = BTreeSet::from([unit.unit]);
    let units = BTreeMap::from([(unit.unit, &unit)]);
    validate_cached_observed_inputs(&units, &cached_ids, &descriptor_target).unwrap();

    let second_secret = "second-sensitive-value";
    fs::write(
        &dep_info,
        format!(
            "{}: {}\n\n{}:\n# env-dep:PROA_CACHE_SECRET={}\n",
            dep_info.display(),
            source.display(),
            source.display(),
            second_secret
        ),
    )
    .unwrap();
    let error =
        validate_cached_observed_inputs(&units, &cached_ids, &descriptor_target).unwrap_err();
    let diagnostic = error.to_string();
    assert!(!diagnostic.contains(first_secret));
    assert!(!diagnostic.contains(second_secret));
    assert!(matches!(
        error,
        DescriptorCacheError::CachedOutDirDependencyMismatch { .. }
    ));
}

#[cfg(unix)]
#[test]
fn cached_unit_validation_rejects_symlinked_out_dir_component() {
    let temp = TempDir::new().unwrap();
    let (mut unit, _cargo, _source, _manifest) = cached_unit_fixture(&temp);
    let descriptor_target = temp.path().join("cached-package/.build");
    let real_out_dir = descriptor_target.join("debug/build/fixture/out");
    let generated = real_out_dir.join("generated.rs");
    fs::create_dir_all(&real_out_dir).unwrap();
    fs::write(&generated, b"pub const GENERATED: u8 = 1;\n").unwrap();
    std::os::unix::fs::symlink(
        descriptor_target.join("debug"),
        descriptor_target.join("debug-link"),
    )
    .unwrap();
    unit.sidecar.out_dir = Some("debug-link/build/fixture/out".to_string());
    unit.sidecar.out_dir_dependencies = vec![DescriptorOutDirDependency {
        relative_path: "generated.rs".to_string(),
        content_hash: descriptor_out_dir_dependency_content_hash(&generated).unwrap(),
    }];
    let cached_ids = BTreeSet::from([unit.unit]);
    let units = BTreeMap::from([(unit.unit, &unit)]);

    assert!(matches!(
        validate_cached_observed_inputs(&units, &cached_ids, &descriptor_target),
        Err(DescriptorCacheError::CachedOutDirDependencyMismatch { .. })
    ));
}

#[test]
fn cached_descriptor_objects_stay_in_cas_and_corruption_is_rejected() {
    let temp = TempDir::new().unwrap();
    let (cache, generation, unit, object) = direct_cached_unit_fixture(&temp);
    let generations = BTreeMap::from([(unit, generation)]);
    let staging = temp.path().join("nonce-staging");
    fs::create_dir_all(staging.join("objects")).unwrap();

    let validated = load_cached_units_direct(&cache, &generations).unwrap();
    assert_eq!(validated.len(), 1);
    assert_eq!(validated[0].descriptor_object, object);
    assert_eq!(
        validated[0].sidecar_path,
        cached_sidecar_path(&cache, &generations[&unit])
    );
    assert!(
        fs::read_dir(staging.join("objects"))
            .unwrap()
            .next()
            .is_none(),
        "a cache hit must not materialize an object in nonce staging"
    );

    fs::write(&object, b"corrupted descriptor object").unwrap();
    assert!(matches!(
        load_cached_units_direct(&cache, &generations),
        Err(DescriptorCacheError::DescriptorObjectHashMismatch {
            unit: corrupted,
            ..
        }) if corrupted == unit
    ));

    #[cfg(unix)]
    {
        let replacement = temp.path().join("replacement.o");
        fs::write(&replacement, b"content-addressed descriptor object").unwrap();
        fs::remove_file(&object).unwrap();
        std::os::unix::fs::symlink(&replacement, &object).unwrap();
        assert!(matches!(
            load_cached_units_direct(&cache, &generations),
            Err(DescriptorCacheError::UnsafeCacheEntry(path)) if path == object
        ));
    }
}

#[test]
fn direct_cache_hits_revalidate_sidecar_semantics() {
    let temp = TempDir::new().unwrap();
    let (cache, mut generation, unit, _object) = direct_cached_unit_fixture(&temp);
    let original_sidecar = cached_sidecar_path(&cache, &generation);
    let mut sidecar: DescriptorUnitSidecar =
        serde_json::from_slice(&fs::read(&original_sidecar).unwrap()).unwrap();
    sidecar.descriptor_codegen.incremental = true;
    let bytes = serde_json::to_vec(&sidecar).unwrap();
    generation.sidecar_hash = cached_sidecar_hash(&bytes);
    let forged_sidecar = cached_sidecar_path(&cache, &generation);
    fs::write(&forged_sidecar, bytes).unwrap();

    assert!(matches!(
        load_cached_units_direct(&cache, &BTreeMap::from([(unit, generation)])),
        Err(DescriptorCacheError::InvalidCachedSidecar {
            unit: invalid,
            ..
        }) if invalid == unit
    ));
}

#[test]
fn completed_staging_cleanup_requires_ownership_marker() {
    let temp = TempDir::new().unwrap();
    let staging = temp.path().join("owned-staging");
    fs::create_dir_all(staging.join("objects")).unwrap();
    fs::write(
        staging.join(DESCRIPTOR_STAGING_MARKER_FILE),
        DESCRIPTOR_STAGING_MARKER_BYTES,
    )
    .unwrap();
    fs::write(staging.join("objects/descriptor.o"), b"large output").unwrap();

    remove_completed_staging(&staging).unwrap();
    assert!(!staging.exists());

    let unowned = temp.path().join("unowned-staging");
    fs::create_dir_all(&unowned).unwrap();
    assert!(matches!(
        remove_completed_staging(&unowned),
        Err(DescriptorCacheError::UnownedRecoveryStaging(path)) if path == unowned
    ));
    assert!(unowned.exists());
}

#[test]
fn verified_object_copy_rejects_blob_and_sidecar_hash_tampering() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source.o");
    fs::write(&source, b"exact descriptor object").unwrap();
    let expected = descriptor_object_content_hash(&source).unwrap();
    let unit = DescriptorCargoUnitId::from_hex(&"66".repeat(32)).unwrap();

    let exact_destination = temp.path().join("cache/exact.o");
    atomic_copy_descriptor_object_verified(&source, &exact_destination, unit, &expected).unwrap();
    assert_eq!(
        fs::read(&exact_destination).unwrap(),
        fs::read(&source).unwrap()
    );

    let forged_hash_destination = temp.path().join("cache/forged-hash.o");
    let error = atomic_copy_descriptor_object_verified(
        &source,
        &forged_hash_destination,
        unit,
        &"00".repeat(32),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        DescriptorCacheError::DescriptorObjectHashMismatch { .. }
    ));
    assert!(!forged_hash_destination.exists());

    fs::write(&source, b"mutated descriptor object").unwrap();
    let mutated_destination = temp.path().join("cache/mutated.o");
    let error =
        atomic_copy_descriptor_object_verified(&source, &mutated_destination, unit, &expected)
            .unwrap_err();
    assert!(matches!(
        error,
        DescriptorCacheError::DescriptorObjectHashMismatch { .. }
    ));
    assert!(!mutated_destination.exists());
}
