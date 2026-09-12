// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! rsjs DSL conformance harness.
//!
//! This suite is intentionally shaped like a language contract:
//!
//! - `tests/ui/conformance/pass/*.rs` contains syntax that is inside the
//!   supported rsjs client IR surface and must keep compiling.
//! - `tests/ui/conformance/fail/*.rs` contains tempting Rust syntax that is
//!   outside the current client IR surface, plus rustc-resolved direct
//!   component ABI/render-trait violations. Both must fail with a clear,
//!   user-facing diagnostic.
//!
//! The broader `ui.rs` suite still snapshots historical diagnostics. This
//! harness is narrower: it should grow when the public rsjs DSL boundary moves.
//! A direct component's `ComponentAbi` and `WebRenderSync`/`WebRender`
//! obligations are emitted into this compilation, so their diagnostics remain
//! in this suite. Rust-resolved helper-definition failures live under
//! `tests/ui/linker/` because only the descriptor linker, not this proc-macro
//! trybuild pass, can distinguish a normal Rust function from
//! `#[rsjs::client]`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

mod support;

#[test]
fn feature_manifest_covers_trybuild_conformance_fixtures() {
    let workspace = workspace_root();
    let manifest_path = workspace.join("rsjs_conformance/features.json");
    let manifest = fs::read_to_string(&manifest_path).unwrap_or_else(|err| {
        panic!(
            "failed to read rsjs conformance manifest at {}: {err}",
            manifest_path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&manifest)
        .unwrap_or_else(|err| panic!("invalid rsjs conformance manifest JSON: {err}"));

    assert_eq!(
        json.get("schema_version").and_then(|value| value.as_u64()),
        Some(1),
        "rsjs conformance manifest must declare schema_version = 1"
    );

    let features = json
        .get("features")
        .and_then(|value| value.as_array())
        .expect("rsjs conformance manifest needs a features array");

    let allowed_statuses = BTreeSet::from([
        "supported",
        "partial",
        "expected_fail",
        "planned",
        "out_of_scope",
        "unknown",
    ]);
    let mut ids = BTreeSet::new();
    let mut manifest_fixtures = BTreeSet::new();
    let mut fixture_case_ids = BTreeMap::new();
    let mut snapshot_paths = BTreeMap::new();
    let mut runtime_script_paths = BTreeMap::new();
    let mut coverage_counts = BTreeMap::<&str, usize>::new();
    let mut pass_fixture_count = 0_usize;
    let mut fail_fixture_count = 0_usize;
    let mut compile_fail_stderr_count = 0_usize;
    let coverage_keys = BTreeSet::from([
        "compile_pass",
        "compile_fail",
        "hir_snapshot",
        "ir_snapshot",
        "js_snapshot",
        "descriptor_snapshot",
        "runtime",
    ]);
    let snapshot_keys = BTreeSet::from([
        "hir_snapshot",
        "ir_snapshot",
        "js_snapshot",
        "descriptor_snapshot",
    ]);

    for feature in features {
        let id = required_str(feature, "id");
        assert!(ids.insert(id.to_string()), "duplicate feature id `{id}`");

        let status = required_str(feature, "status");
        assert!(
            allowed_statuses.contains(status),
            "feature `{id}` has unknown status `{status}`"
        );
        if feature.get("category").and_then(|value| value.as_str()) == Some("semantics") {
            let semantic_contract = required_str(feature, "semantic_contract");
            assert!(
                matches!(
                    semantic_contract,
                    "parity" | "characterization" | "unsupported"
                ),
                "semantic feature `{id}` has unknown semantic_contract `{semantic_contract}`"
            );
            if status == "supported" {
                assert_eq!(
                    semantic_contract, "parity",
                    "supported semantic feature `{id}` must be a parity guarantee, not a characterization"
                );
            }
            if semantic_contract == "characterization" {
                assert_ne!(
                    status, "supported",
                    "characterization feature `{id}` must not claim supported parity"
                );
            }
        }
        if let Some(corpus_path) = feature
            .get("differential_corpus")
            .and_then(|value| value.as_str())
        {
            assert!(
                corpus_path.starts_with("rsjs_conformance/")
                    && corpus_path.ends_with(".json"),
                "feature `{id}` differential_corpus `{corpus_path}` must be an rsjs_conformance JSON file"
            );
            let corpus_source = fs::read_to_string(workspace.join(corpus_path))
                .unwrap_or_else(|err| panic!("failed to read differential corpus: {err}"));
            let corpus: serde_json::Value = serde_json::from_str(&corpus_source)
                .unwrap_or_else(|err| panic!("invalid differential corpus JSON: {err}"));
            assert_eq!(
                corpus
                    .get("schema_version")
                    .and_then(|value| value.as_u64()),
                Some(1),
                "feature `{id}` differential corpus must declare schema_version = 1"
            );
            assert!(
                corpus
                    .get("cases")
                    .and_then(|value| value.as_array())
                    .is_some_and(|cases| !cases.is_empty()),
                "feature `{id}` differential corpus needs cases"
            );
            assert!(
                corpus
                    .get("exclusions")
                    .and_then(|value| value.as_array())
                    .is_some_and(|exclusions| !exclusions.is_empty()),
                "feature `{id}` differential corpus needs explicit exclusions"
            );
        }

        let coverage = feature
            .get("coverage")
            .and_then(|value| value.as_object())
            .unwrap_or_else(|| panic!("feature `{id}` needs a coverage object"));
        for key in coverage.keys() {
            assert!(
                coverage_keys.contains(key.as_str()),
                "feature `{id}` has unknown coverage key `{key}`"
            );
        }
        for key in &coverage_keys {
            if coverage.get(*key).and_then(|value| value.as_bool()) == Some(true) {
                *coverage_counts.entry(*key).or_default() += 1;
            }
        }
        let has_compile_pass = coverage
            .get("compile_pass")
            .and_then(|value| value.as_bool())
            == Some(true);
        if has_compile_pass {
            for key in ["hir_snapshot", "ir_snapshot", "js_snapshot", "runtime"] {
                assert_eq!(
                    coverage.get(key).and_then(|value| value.as_bool()),
                    Some(true),
                    "feature `{id}` has coverage.compile_pass=true but no coverage.{key}=true"
                );
            }
        }
        if has_compile_pass && descriptor_snapshot_required(feature) {
            assert_eq!(
                coverage
                    .get("descriptor_snapshot")
                    .and_then(|value| value.as_bool()),
                Some(true),
                "feature `{id}` has descriptor-relevant source positions but no coverage.descriptor_snapshot=true"
            );
        }
        if coverage.get("runtime").and_then(|value| value.as_bool()) == Some(true) {
            assert_eq!(
                coverage
                    .get("compile_pass")
                    .and_then(|value| value.as_bool()),
                Some(true),
                "feature `{id}` has coverage.runtime=true but no coverage.compile_pass=true"
            );
            let runtime_script = feature
                .get("runtime_script")
                .and_then(|value| value.as_str())
                .unwrap_or_else(|| {
                    panic!("feature `{id}` has coverage.runtime=true but no runtime_script")
                });
            assert!(
                runtime_script.starts_with("rsjs_conformance/runtime/")
                    && runtime_script.ends_with(".mjs"),
                "feature `{id}` runtime_script `{runtime_script}` must point at an rsjs_conformance/runtime/*.mjs file"
            );
            assert!(
                workspace.join(runtime_script).exists(),
                "feature `{id}` references missing runtime script `{runtime_script}`"
            );
            if let Some(previous_id) =
                runtime_script_paths.insert(runtime_script.to_string(), id.to_string())
            {
                panic!(
                    "feature `{id}` references runtime script `{runtime_script}`, already referenced by `{previous_id}`"
                );
            }
        } else {
            assert!(
                feature.get("runtime_script").is_none(),
                "feature `{id}` has runtime_script but coverage.runtime is not true"
            );
        }
        let snapshots = feature.get("snapshots").and_then(|value| value.as_object());
        if let Some(snapshots) = snapshots {
            for key in snapshots.keys() {
                assert!(
                    snapshot_keys.contains(key.as_str()),
                    "feature `{id}` has unknown snapshot key `{key}`"
                );
            }
        }
        for key in &snapshot_keys {
            let covered = coverage
                .get(*key)
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let snapshot_path = snapshots
                .and_then(|snapshots| snapshots.get(*key))
                .and_then(|value| value.as_str());
            if let Some(path) = snapshot_path {
                if let Some(previous_id) = snapshot_paths.insert(path.to_string(), id.to_string()) {
                    panic!(
                        "feature `{id}` references snapshot `{path}`, already referenced by `{previous_id}`"
                    );
                }
            }
            if covered {
                let path = snapshot_path.unwrap_or_else(|| {
                    panic!("feature `{id}` marks coverage.{key}=true but has no snapshots.{key}")
                });
                assert!(
                    workspace.join(path).exists(),
                    "feature `{id}` references missing snapshot `{path}`"
                );
                validate_snapshot_path(&workspace, id, key, path);
            } else {
                assert!(
                    snapshot_path.is_none(),
                    "feature `{id}` has snapshots.{key} but coverage.{key} is not true"
                );
            }
        }
        if let Some(supporting_snapshots) = feature.get("supporting_snapshots") {
            let supporting_snapshots = supporting_snapshots
                .as_array()
                .unwrap_or_else(|| panic!("feature `{id}` supporting_snapshots must be an array"));
            for value in supporting_snapshots {
                let path = value.as_str().unwrap_or_else(|| {
                    panic!("feature `{id}` supporting_snapshots entries must be strings")
                });
                let kind = snapshot_kind_for_path(path).unwrap_or_else(|| {
                    panic!("feature `{id}` supporting snapshot `{path}` has an unknown file shape")
                });
                if let Some(previous_id) = snapshot_paths.insert(path.to_string(), id.to_string()) {
                    panic!(
                        "feature `{id}` references supporting snapshot `{path}`, already referenced by `{previous_id}`"
                    );
                }
                assert!(
                    workspace.join(path).exists(),
                    "feature `{id}` references missing supporting snapshot `{path}`"
                );
                validate_snapshot_path(&workspace, id, kind, path);
            }
        }
        let fixtures = feature
            .get("fixtures")
            .and_then(|value| value.as_array())
            .unwrap_or_else(|| panic!("feature `{id}` needs a fixtures array"));
        assert!(
            !fixtures.is_empty(),
            "feature `{id}` should reference at least one fixture"
        );

        for fixture in fixtures {
            let case_id = required_str(fixture, "case_id");
            let path = required_str(fixture, "path");
            let expect = required_str(fixture, "expect");
            assert!(
                is_valid_case_id(case_id),
                "feature `{id}` fixture `{path}` has invalid case_id `{case_id}`"
            );
            if let Some(previous_path) =
                fixture_case_ids.insert(case_id.to_string(), path.to_string())
            {
                panic!(
                    "feature `{id}` fixture `{path}` reuses case_id `{case_id}` already used by `{previous_path}`"
                );
            }
            assert!(
                matches!(expect, "pass" | "fail"),
                "feature `{id}` fixture `{path}` has invalid expect `{expect}`"
            );
            assert!(
                workspace.join(path).exists(),
                "feature `{id}` references missing fixture `{path}`"
            );
            match expect {
                "pass" => pass_fixture_count += 1,
                "fail" => {
                    fail_fixture_count += 1;
                    let stderr_path = workspace.join(path).with_extension("stderr");
                    assert!(
                        stderr_path.exists(),
                        "feature `{id}` compile-fail fixture `{path}` is missing stderr snapshot {}",
                        stderr_path.display()
                    );
                    compile_fail_stderr_count += 1;
                }
                _ => unreachable!("fixture expectation was validated above"),
            }
            let expected_coverage_key = if expect == "pass" {
                "compile_pass"
            } else {
                "compile_fail"
            };
            assert_eq!(
                coverage
                    .get(expected_coverage_key)
                    .and_then(|value| value.as_bool()),
                Some(true),
                "feature `{id}` fixture `{path}` expects `{expect}` but coverage.{expected_coverage_key} is not true"
            );
            manifest_fixtures.insert(path.to_string());
        }
    }

    let coverage_min = json
        .get("gates")
        .and_then(|gates| gates.get("coverage_min"))
        .and_then(|value| value.as_object())
        .expect("rsjs conformance manifest needs gates.coverage_min");
    for (key, min) in coverage_min {
        assert!(
            coverage_keys.contains(key.as_str()),
            "manifest has unknown coverage gate `{key}`"
        );
        let min = min.as_u64().unwrap_or_else(|| {
            panic!("manifest coverage gate `{key}` must be a non-negative integer")
        }) as usize;
        let actual = coverage_counts.get(key.as_str()).copied().unwrap_or(0);
        assert!(
            actual >= min,
            "coverage.{key} regressed below gate: {actual} < {min}"
        );
    }

    let fixture_min = json
        .get("gates")
        .and_then(|gates| gates.get("fixture_min"))
        .and_then(|value| value.as_object())
        .expect("rsjs conformance manifest needs gates.fixture_min");
    let fixture_gate_counts = BTreeMap::from([
        ("pass", pass_fixture_count),
        ("fail", fail_fixture_count),
        ("case_ids", fixture_case_ids.len()),
        ("compile_fail_stderr", compile_fail_stderr_count),
    ]);
    for (key, min) in fixture_min {
        let actual = fixture_gate_counts
            .get(key.as_str())
            .unwrap_or_else(|| panic!("manifest has unknown fixture gate `{key}`"));
        let min = min.as_u64().unwrap_or_else(|| {
            panic!("manifest fixture gate `{key}` must be a non-negative integer")
        }) as usize;
        assert!(
            *actual >= min,
            "fixtures.{key} regressed below gate: {actual} < {min}"
        );
    }

    let mut expected_fixtures = BTreeSet::new();
    collect_fixture_paths(
        &workspace,
        &workspace.join("rsjs_macros/tests/ui/conformance/pass"),
        &mut expected_fixtures,
    );
    collect_fixture_paths(
        &workspace,
        &workspace.join("rsjs_macros/tests/ui/conformance/fail"),
        &mut expected_fixtures,
    );

    assert_eq!(
        manifest_fixtures, expected_fixtures,
        "rsjs_conformance/features.json must reference every conformance trybuild fixture exactly once"
    );

    let mut expected_runtime_scripts = BTreeSet::new();
    collect_runtime_script_paths(
        &workspace,
        &workspace.join("rsjs_conformance/runtime"),
        &mut expected_runtime_scripts,
    );
    let manifest_runtime_scripts = runtime_script_paths
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        manifest_runtime_scripts, expected_runtime_scripts,
        "rsjs_conformance/features.json must reference every runtime conformance script exactly once"
    );

    let mut expected_snapshots = BTreeSet::new();
    collect_snapshot_paths(
        &workspace,
        &workspace.join("rsjs_conformance/snapshots"),
        &mut expected_snapshots,
    );
    let manifest_snapshots = snapshot_paths.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        manifest_snapshots, expected_snapshots,
        "rsjs_conformance/features.json must reference every conformance snapshot exactly once"
    );
}

#[test]
fn supported_rsjs_dsl_surface_compiles() {
    support::configure_trybuild_descriptor_phase();
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/conformance/pass/*.rs");
}

#[test]
fn unsupported_rsjs_dsl_surface_has_guided_diagnostics() {
    support::configure_trybuild_descriptor_phase();
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/conformance/fail/*.rs");
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rsjs_macros lives directly under the workspace root")
        .to_path_buf()
}

fn required_str<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or_else(|| panic!("manifest object is missing string field `{key}`"))
}

fn descriptor_snapshot_required(feature: &serde_json::Value) -> bool {
    feature
        .get("source_positions")
        .and_then(|value| value.as_array())
        .is_some_and(|positions| {
            positions
                .iter()
                .filter_map(|value| value.as_str())
                .any(|position| matches!(position, "component_prop" | "template_loop"))
        })
}

fn collect_fixture_paths(workspace: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("failed to read fixture dir {}: {err}", dir.display()))
    {
        let path = entry
            .unwrap_or_else(|err| panic!("failed to read fixture dir entry: {err}"))
            .path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let rel = path
                .strip_prefix(workspace)
                .expect("fixture should live under workspace root")
                .to_string_lossy()
                .replace('\\', "/");
            assert!(
                out.insert(rel),
                "duplicate conformance fixture path {}",
                path.display()
            );
        }
    }
}

fn collect_runtime_script_paths(workspace: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("failed to read runtime dir {}: {err}", dir.display()))
    {
        let path = entry
            .unwrap_or_else(|err| panic!("failed to read runtime dir entry: {err}"))
            .path();
        if path.extension().is_some_and(|ext| ext == "mjs") {
            let rel = path
                .strip_prefix(workspace)
                .expect("runtime script should live under workspace root")
                .to_string_lossy()
                .replace('\\', "/");
            if rel == "rsjs_conformance/runtime/harness.mjs" {
                continue;
            }
            assert!(
                out.insert(rel),
                "duplicate runtime conformance script path {}",
                path.display()
            );
        }
    }
}

fn collect_snapshot_paths(workspace: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("failed to read snapshot dir {}: {err}", dir.display()))
    {
        let path = entry
            .unwrap_or_else(|err| panic!("failed to read snapshot dir entry: {err}"))
            .path();
        if path.is_dir() {
            collect_snapshot_paths(workspace, &path, out);
            continue;
        }
        if path.is_file() {
            let rel = path
                .strip_prefix(workspace)
                .expect("snapshot should live under workspace root")
                .to_string_lossy()
                .replace('\\', "/");
            assert!(
                out.insert(rel),
                "duplicate conformance snapshot path {}",
                path.display()
            );
        }
    }
}

fn snapshot_kind_for_path(path: &str) -> Option<&'static str> {
    if path.ends_with(".descriptor.json") {
        Some("descriptor_snapshot")
    } else if path.ends_with(".body-ir.json") || path.ends_with(".module-ir.json") {
        Some("ir_snapshot")
    } else if path.ends_with(".hir.json") || path.ends_with(".body-hir.json") {
        Some("hir_snapshot")
    } else if path.ends_with(".js") {
        Some("js_snapshot")
    } else {
        None
    }
}

fn validate_snapshot_path(workspace: &Path, feature_id: &str, kind: &str, path: &str) {
    assert!(
        path.starts_with("rsjs_conformance/snapshots/"),
        "feature `{feature_id}` snapshot `{path}` must live under rsjs_conformance/snapshots/"
    );
    match kind {
        "hir_snapshot" => {
            assert!(
                path.ends_with(".hir.json") || path.ends_with(".body-hir.json"),
                "feature `{feature_id}` HIR snapshot `{path}` must end with .hir.json or .body-hir.json"
            );
            let text = fs::read_to_string(workspace.join(path))
                .unwrap_or_else(|err| panic!("failed to read HIR snapshot `{path}`: {err}"));
            let json: serde_json::Value = serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("HIR snapshot `{path}` is not valid JSON: {err}"));
            assert!(
                contains_source_span(&json),
                "feature `{feature_id}` HIR snapshot `{path}` must include at least one source span"
            );
            if path.ends_with(".body-hir.json") {
                assert!(
                    contains_expression_source_span(&json),
                    "feature `{feature_id}` helper HIR snapshot `{path}` must include at least one expression source span"
                );
            } else if path.ends_with(".hir.json") {
                assert!(
                    contains_handler_expression_source_span(&json),
                    "feature `{feature_id}` handler HIR snapshot `{path}` must include at least one expression source span"
                );
                if requires_effect_operand_span_granularity(feature_id, path) {
                    assert!(
                        contains_effect_operand_span_different_from_statement_span(&json),
                        "feature `{feature_id}` HIR snapshot `{path}` must preserve browser/node-ref effect operand spans instead of only statement spans"
                    );
                }
            }
        }
        "ir_snapshot" => {
            assert!(
                path.ends_with(".body-ir.json") || path.ends_with(".module-ir.json"),
                "feature `{feature_id}` Client IR snapshot `{path}` must end with .body-ir.json or .module-ir.json"
            );
            let text = fs::read_to_string(workspace.join(path)).unwrap_or_else(|err| {
                panic!("failed to read Client IR snapshot `{path}`: {err}")
            });
            let json: serde_json::Value = serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("Client IR snapshot `{path}` is not valid JSON: {err}"));
            if path.ends_with(".body-ir.json") {
                validate_client_block_ir_snapshot(feature_id, &json, path);
            } else if path.ends_with(".module-ir.json") {
                validate_client_module_ir_snapshot(feature_id, &json);
            }
        }
        "js_snapshot" => assert!(
            path.ends_with(".js"),
            "feature `{feature_id}` JS snapshot `{path}` must end with .js"
        ),
        "descriptor_snapshot" => assert!(
            path.starts_with("rsjs_conformance/snapshots/descriptor/")
                && path.ends_with(".descriptor.json"),
            "feature `{feature_id}` descriptor snapshot `{path}` must point at rsjs_conformance/snapshots/descriptor/*.descriptor.json"
        ),
        other => panic!("feature `{feature_id}` uses unknown snapshot kind `{other}`"),
    }
    if kind == "descriptor_snapshot" {
        let text = fs::read_to_string(workspace.join(path))
            .unwrap_or_else(|err| panic!("failed to read descriptor snapshot `{path}`: {err}"));
        let json: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|err| panic!("descriptor snapshot `{path}` is not valid JSON: {err}"));
        validate_descriptor_snapshot(feature_id, &json);
    }
}

fn contains_source_span(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => values.iter().any(contains_source_span),
        serde_json::Value::Object(map) => {
            map.get("span").is_some_and(is_source_span) || map.values().any(contains_source_span)
        }
        _ => false,
    }
}

fn is_source_span(value: &serde_json::Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    map.get("start").is_some_and(is_source_position)
        && map.get("end").is_some_and(is_source_position)
}

fn contains_expression_source_span(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => values.iter().any(contains_expression_source_span),
        serde_json::Value::Object(map) => {
            let current = map
                .get("kind")
                .and_then(|value| value.as_str())
                .is_some_and(is_helper_expression_kind)
                && map.get("span").is_some_and(is_source_span);
            current || map.values().any(contains_expression_source_span)
        }
        _ => false,
    }
}

fn is_helper_expression_kind(kind: &str) -> bool {
    matches!(
        kind,
        "Lit"
            | "Param"
            | "Local"
            | "BinaryOp"
            | "UnaryOp"
            | "Conditional"
            | "IsNonEmpty"
            | "Len"
            | "StringToLowercase"
            | "StringContains"
            | "ClientCall"
    )
}

fn contains_handler_expression_source_span(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => {
            values.iter().any(contains_handler_expression_source_span)
        }
        serde_json::Value::Object(map) => {
            let current = map
                .get("kind")
                .and_then(|value| value.as_str())
                .is_some_and(is_handler_expression_kind)
                && map.get("span").is_some_and(is_source_span);
            current || map.values().any(contains_handler_expression_source_span)
        }
        _ => false,
    }
}

fn is_handler_expression_kind(kind: &str) -> bool {
    matches!(
        kind,
        "literal"
            | "array_lit"
            | "signal_read"
            | "local"
            | "event_value"
            | "event_attribute"
            | "event_payload"
            | "node_ref_read"
            | "browser_read"
            | "encoding"
            | "binary"
            | "debug"
    )
}

fn requires_effect_operand_span_granularity(feature_id: &str, path: &str) -> bool {
    (feature_id == "browser.effects.p0" && path.ends_with("browser_effects_p0.hir.json"))
        || (feature_id == "node.refs.p0" && path.ends_with("node_refs_p0.hir.json"))
}

fn contains_effect_operand_span_different_from_statement_span(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .any(contains_effect_operand_span_different_from_statement_span),
        serde_json::Value::Object(map) => {
            let current = matches!(
                map.get("kind").and_then(|value| value.as_str()),
                Some("browser_effect" | "node_ref_effect")
            ) && map
                .get("span")
                .filter(|span| is_source_span(span))
                .is_some_and(|statement_span| {
                    map.get("effect").is_some_and(|effect| {
                        contains_expression_span_different_from(effect, statement_span)
                    })
                });
            current
                || map
                    .values()
                    .any(contains_effect_operand_span_different_from_statement_span)
        }
        _ => false,
    }
}

fn contains_expression_span_different_from(
    value: &serde_json::Value,
    statement_span: &serde_json::Value,
) -> bool {
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| contains_expression_span_different_from(value, statement_span)),
        serde_json::Value::Object(map) => {
            let current = map
                .get("kind")
                .and_then(|value| value.as_str())
                .is_some_and(is_handler_expression_kind)
                && map
                    .get("span")
                    .filter(|span| is_source_span(span))
                    .is_some_and(|span| span != statement_span);
            current
                || map
                    .values()
                    .any(|value| contains_expression_span_different_from(value, statement_span))
        }
        _ => false,
    }
}

fn is_source_position(value: &serde_json::Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    let Some(line) = map.get("line").and_then(|value| value.as_u64()) else {
        return false;
    };
    let Some(column) = map.get("column").and_then(|value| value.as_u64()) else {
        return false;
    };
    line >= 1 && column <= u64::from(u32::MAX)
}

fn validate_client_block_ir_snapshot(feature_id: &str, value: &serde_json::Value, label: &str) {
    rsjs_ir::parse_client_block_json(value).unwrap_or_else(|err| {
        panic!("feature `{feature_id}` Client IR block `{label}` must parse as Client IR v3: {err}")
    });
}

fn validate_client_module_ir_snapshot(feature_id: &str, value: &serde_json::Value) {
    let Some(map) = value.as_object() else {
        panic!("feature `{feature_id}` module IR snapshot must be a JSON object");
    };
    assert_eq!(
        map.get("format").and_then(|value| value.as_str()),
        Some("rsjs.client_module_ir"),
        "feature `{feature_id}` module IR snapshot must declare format = rsjs.client_module_ir"
    );
    assert_eq!(
        map.get("client_ir_version")
            .and_then(|value| value.as_u64()),
        Some(3),
        "feature `{feature_id}` module IR snapshot must declare client_ir_version = 3"
    );

    let functions = map
        .get("functions")
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| {
            panic!("feature `{feature_id}` module IR snapshot must include functions array")
        });
    let exports = map
        .get("exports")
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| {
            panic!("feature `{feature_id}` module IR snapshot must include exports array")
        });

    let mut functions_by_id = std::collections::BTreeMap::new();
    for function in functions {
        let Some(function_map) = function.as_object() else {
            panic!("feature `{feature_id}` module IR functions must be objects");
        };
        let Some(function_id) = function_map.get("id").and_then(|value| value.as_u64()) else {
            panic!("feature `{feature_id}` module IR function id must be a non-negative integer");
        };
        assert!(
            functions_by_id.insert(function_id, function).is_none(),
            "feature `{feature_id}` module IR function id {function_id} is duplicated"
        );
    }

    let mut stack = Vec::new();
    for export in exports {
        let Some(export_map) = export.as_object() else {
            panic!("feature `{feature_id}` module IR exports must be objects");
        };
        let Some(function_id) = export_map.get("function").and_then(|value| value.as_u64()) else {
            panic!(
                "feature `{feature_id}` module IR export function must be a non-negative integer"
            );
        };
        assert!(
            functions_by_id.contains_key(&function_id),
            "feature `{feature_id}` module IR export references unknown function {function_id}"
        );
        stack.push(function_id);
    }

    let mut reachable = std::collections::BTreeSet::new();
    while let Some(function_id) = stack.pop() {
        if !reachable.insert(function_id) {
            continue;
        }
        let function = functions_by_id
            .get(&function_id)
            .expect("function roots checked above");
        let body = function
            .get("body")
            .unwrap_or_else(|| panic!("feature `{feature_id}` module IR function must have body"));
        validate_client_block_ir_snapshot(
            feature_id,
            body,
            &format!("module IR function {function_id} body"),
        );
        let mut local_refs = Vec::new();
        collect_local_function_refs(body, &mut local_refs);
        for target in local_refs {
            assert!(
                functions_by_id.contains_key(&target),
                "feature `{feature_id}` module IR function {function_id} calls unknown local function {target}"
            );
            stack.push(target);
        }
    }

    for function_id in functions_by_id.keys() {
        assert!(
            reachable.contains(function_id),
            "feature `{feature_id}` module IR function {function_id} is unreachable from exports"
        );
    }
}

fn collect_local_function_refs(value: &serde_json::Value, out: &mut Vec<u64>) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                collect_local_function_refs(value, out);
            }
        }
        serde_json::Value::Object(map) => {
            if matches!(
                map.get("kind").and_then(|value| value.as_str()),
                Some("call")
            ) {
                if let Some(function_id) = map
                    .get("function")
                    .and_then(|value| value.as_object())
                    .filter(|function| {
                        matches!(
                            function.get("kind").and_then(|value| value.as_str()),
                            Some("local")
                        )
                    })
                    .and_then(|function| function.get("function"))
                    .and_then(|value| value.as_u64())
                {
                    out.push(function_id);
                }
            }
            for value in map.values() {
                collect_local_function_refs(value, out);
            }
        }
        _ => {}
    }
}

fn validate_descriptor_snapshot(feature_id: &str, value: &serde_json::Value) {
    let Some(map) = value.as_object() else {
        panic!("feature `{feature_id}` descriptor snapshot must be a JSON object");
    };
    assert_eq!(
        map.get("schema_version").and_then(|value| value.as_u64()),
        Some(9),
        "feature `{feature_id}` descriptor snapshot must declare schema_version = 9"
    );
    assert!(
        matches!(
            map.get("client_origin_permission")
                .and_then(|value| value.as_str()),
            Some("participant_only" | "origin_capable")
        ),
        "feature `{feature_id}` descriptor snapshot must declare a valid \
         client_origin_permission"
    );
    assert_eq!(
        map.get("format").and_then(|value| value.as_str()),
        Some("rsjs.component_descriptor"),
        "feature `{feature_id}` descriptor snapshot must declare format = rsjs.component_descriptor"
    );
    assert_eq!(
        map.get("client_ir_version")
            .and_then(|value| value.as_u64()),
        Some(3),
        "feature `{feature_id}` descriptor snapshot must declare client_ir_version = 3"
    );
    assert_eq!(
        map.get("asset_id").and_then(|value| value.as_str()),
        Some("<asset_id>"),
        "feature `{feature_id}` descriptor snapshot must normalize asset_id"
    );
    assert_eq!(
        map.get("js_hash").and_then(|value| value.as_str()),
        Some("sha256:<hash>"),
        "feature `{feature_id}` descriptor snapshot must normalize js_hash"
    );
    assert_eq!(
        map.get("descriptor_hash").and_then(|value| value.as_str()),
        Some("sha256:<hash>"),
        "feature `{feature_id}` descriptor snapshot must normalize descriptor_hash"
    );

    let handlers = map
        .get("handlers")
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| {
            panic!("feature `{feature_id}` descriptor snapshot must include handlers array")
        });
    validate_descriptor_handlers(feature_id, handlers);

    let callback_slots = map
        .get("callback_slots")
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| {
            panic!("feature `{feature_id}` descriptor snapshot must include callback_slots array")
        });
    validate_descriptor_callback_slots(feature_id, callback_slots);

    let fields = map
        .get("fields")
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| {
            panic!("feature `{feature_id}` descriptor snapshot must include fields array")
        });
    validate_descriptor_fields(feature_id, fields, callback_slots);

    let bindings = map
        .get("bindings")
        .and_then(|value| value.as_object())
        .unwrap_or_else(|| {
            panic!("feature `{feature_id}` descriptor snapshot must include bindings object")
        });
    validate_descriptor_bindings(feature_id, bindings, handlers);

    let view_tree = map
        .get("view_tree")
        .and_then(|value| value.as_object())
        .unwrap_or_else(|| {
            panic!("feature `{feature_id}` descriptor snapshot must include view_tree object")
        });
    let counts = map
        .get("counts")
        .and_then(|value| value.as_object())
        .unwrap_or_else(|| {
            panic!("feature `{feature_id}` descriptor snapshot must include counts object")
        });
    validate_descriptor_counts(feature_id, counts, map, view_tree);
}

fn validate_descriptor_handlers(feature_id: &str, handlers: &[serde_json::Value]) {
    let mut handler_ids = BTreeSet::new();
    for handler in handlers {
        let Some(handler_map) = handler.as_object() else {
            panic!("feature `{feature_id}` descriptor handlers must be objects");
        };
        let Some(handler_id) = handler_map.get("id").and_then(|value| value.as_u64()) else {
            panic!("feature `{feature_id}` descriptor handler id must be a non-negative integer");
        };
        assert!(
            handler_ids.insert(handler_id),
            "feature `{feature_id}` descriptor handler id {handler_id} is duplicated"
        );
        assert!(
            !handler_map.contains_key("statements"),
            "feature `{feature_id}` descriptor handler {handler_id} must not use legacy statements"
        );
        assert_eq!(
            handler_map
                .get("body_ir_version")
                .and_then(|value| value.as_u64()),
            Some(3),
            "feature `{feature_id}` descriptor handler {handler_id} must declare body_ir_version = 3"
        );
        let body_ir = handler_map
            .get("body_ir")
            .and_then(|value| value.as_object())
            .unwrap_or_else(|| {
                panic!(
                    "feature `{feature_id}` descriptor handler {handler_id} must include body_ir object"
                )
            });
        validate_client_block_ir_snapshot(
            feature_id,
            &serde_json::Value::Object(body_ir.clone()),
            &format!("descriptor handler {handler_id} body_ir"),
        );
    }
}

fn validate_descriptor_callback_slots(feature_id: &str, callback_slots: &[serde_json::Value]) {
    let mut slot_ids = BTreeSet::new();
    for slot in callback_slots {
        let Some(slot_map) = slot.as_object() else {
            panic!("feature `{feature_id}` descriptor callback_slots entries must be objects");
        };
        let Some(slot_id) = slot_map.get("id").and_then(|value| value.as_u64()) else {
            panic!(
                "feature `{feature_id}` descriptor callback slot id must be a non-negative integer"
            );
        };
        assert!(
            slot_ids.insert(slot_id),
            "feature `{feature_id}` descriptor callback slot id {slot_id} is duplicated"
        );
        assert!(
            slot_map
                .get("name")
                .and_then(|value| value.as_str())
                .is_some_and(|name| !name.is_empty()),
            "feature `{feature_id}` descriptor callback slot {slot_id} must include a non-empty name"
        );
    }
}

fn validate_descriptor_fields(
    feature_id: &str,
    fields: &[serde_json::Value],
    callback_slots: &[serde_json::Value],
) {
    let callback_slot_ids = callback_slots
        .iter()
        .filter_map(|slot| slot.get("id").and_then(|value| value.as_u64()))
        .collect::<BTreeSet<_>>();
    for field in fields {
        let Some(field_map) = field.as_object() else {
            panic!("feature `{feature_id}` descriptor fields entries must be objects");
        };
        let field_name = field_map
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("<unknown>");
        assert!(
            field_name != "<unknown>" && !field_name.is_empty(),
            "feature `{feature_id}` descriptor field must include a non-empty name"
        );
        if matches!(
            field_map.get("kind").and_then(|value| value.as_str()),
            Some("handler")
        ) {
            let slot_id = field_map
                .get("callback_slot")
                .and_then(|value| value.as_u64())
                .unwrap_or_else(|| {
                    panic!(
                        "feature `{feature_id}` descriptor handler field {field_name} must include callback_slot"
                    )
                });
            assert!(
                callback_slot_ids.contains(&slot_id),
                "feature `{feature_id}` descriptor handler field {field_name} references unknown callback slot {slot_id}"
            );
        }
    }
}

fn validate_descriptor_bindings(
    feature_id: &str,
    bindings: &serde_json::Map<String, serde_json::Value>,
    handlers: &[serde_json::Value],
) {
    let handler_ids = handlers
        .iter()
        .filter_map(|handler| handler.get("id").and_then(|value| value.as_u64()))
        .collect::<BTreeSet<_>>();
    let events = bindings
        .get("events")
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| {
            panic!("feature `{feature_id}` descriptor bindings must include events array")
        });
    for event_binding in events {
        let Some(event_map) = event_binding.as_object() else {
            panic!("feature `{feature_id}` descriptor event bindings must be objects");
        };
        let Some(handler_id) = event_map.get("handler").and_then(|value| value.as_u64()) else {
            panic!(
                "feature `{feature_id}` descriptor event binding must reference a non-negative handler id"
            );
        };
        assert!(
            handler_ids.contains(&handler_id),
            "feature `{feature_id}` descriptor event binding references unknown handler {handler_id}"
        );
    }
}

fn validate_descriptor_counts(
    feature_id: &str,
    counts: &serde_json::Map<String, serde_json::Value>,
    descriptor: &serde_json::Map<String, serde_json::Value>,
    view_tree: &serde_json::Map<String, serde_json::Value>,
) {
    for key in [
        "handlers",
        "callback_slots",
        "signals",
        "node_refs",
        "exprs",
    ] {
        let Some(values) = descriptor.get(key).and_then(|value| value.as_array()) else {
            continue;
        };
        assert_eq!(
            counts.get(key).and_then(|value| value.as_u64()),
            Some(values.len() as u64),
            "feature `{feature_id}` descriptor counts.{key} must match {key} array length"
        );
    }
    if let Some(nodes) = view_tree.get("nodes").and_then(|value| value.as_array()) {
        assert_eq!(
            counts.get("nodes").and_then(|value| value.as_u64()),
            Some(nodes.len() as u64),
            "feature `{feature_id}` descriptor counts.nodes must match view_tree.nodes length"
        );
    }
}

fn is_valid_case_id(value: &str) -> bool {
    let mut previous_was_separator = true;
    for byte in value.bytes() {
        match byte {
            b'a'..=b'z' | b'0'..=b'9' => previous_was_separator = false,
            b'.' | b'_' | b'-' if !previous_was_separator => {
                previous_was_separator = true;
            }
            _ => return false,
        }
    }
    !previous_was_separator
}
