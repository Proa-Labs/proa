// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[path = "../../rsjs_conformance/rust/semantic_corpus.rs"]
mod semantic_corpus;

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[test]
fn rust_side_semantic_corpus_is_complete_and_evaluable() {
    let corpus = semantic_corpus::parse_and_validate(include_str!(
        "../../rsjs_conformance/semantic-corpus.json"
    ))
    .expect("semantic corpus should validate");
    let output = semantic_corpus::evaluate(&corpus).expect("Rust corpus should evaluate");
    let cases = corpus["cases"]
        .as_array()
        .expect("validated corpus has cases");
    let results = output["results"]
        .as_array()
        .expect("Rust evaluator emits results");

    assert_eq!(
        results.len(),
        cases.len(),
        "every differential case needs one Rust-side result"
    );

    let result_ids = results
        .iter()
        .map(|result| result["id"].as_str().expect("result id should be a string"))
        .collect::<BTreeSet<_>>();
    let case_ids = cases
        .iter()
        .map(|case| case["id"].as_str().expect("case id should be a string"))
        .collect::<BTreeSet<_>>();
    assert_eq!(result_ids, case_ids);

    let parity_operations = cases
        .iter()
        .filter(|case| case["expectation"] == "parity")
        .map(|case| {
            case["operation"]
                .as_str()
                .expect("operation should be a string")
        })
        .collect::<BTreeSet<_>>();
    for required in [
        "string_utf8_len",
        "string_trim",
        "i64_increment",
        "i64_div_rem",
        "f64_display",
        "option_i32_wire",
        "option_string_wire",
        "fieldless_enum_wire",
        "nested_account_wire",
    ] {
        assert!(
            parity_operations.contains(required),
            "semantic corpus needs a parity case for `{required}`"
        );
    }

    assert!(
        cases
            .iter()
            .filter(|case| case["expectation"] == "known_divergence")
            .all(|case| !case["browser_characterization"].is_null()),
        "known divergences need an exact browser characterization"
    );
    assert!(
        cases
            .iter()
            .any(|case| case["expectation"] == "known_divergence"),
        "known divergences must stay visibly separate from parity cases"
    );
}

#[test]
fn unsupported_wire_shapes_are_explicit_and_backed_by_compile_fail_gates() {
    let corpus = semantic_corpus::parse_and_validate(include_str!(
        "../../rsjs_conformance/semantic-corpus.json"
    ))
    .expect("semantic corpus should validate");
    let exclusions = corpus["exclusions"]
        .as_array()
        .expect("validated corpus has exclusions");
    let by_id = exclusions
        .iter()
        .map(|entry| {
            (
                entry["id"]
                    .as_str()
                    .expect("exclusion id should be a string"),
                entry,
            )
        })
        .collect::<BTreeMap<_, _>>();

    for id in [
        "wire.client_props.data_enum",
        "wire.client_props.tuple_struct",
        "wire.client_props.nested_option",
    ] {
        assert!(by_id.contains_key(id), "missing explicit exclusion `{id}`");
    }

    let workspace = workspace_root();
    for exclusion in exclusions {
        if exclusion["kind"] != "compile_fail" {
            continue;
        }
        for key in ["fixture", "stderr"] {
            let relative = exclusion[key]
                .as_str()
                .unwrap_or_else(|| panic!("compile-fail exclusion needs `{key}`"));
            assert!(
                workspace.join(relative).is_file(),
                "compile-fail exclusion references missing {key} `{relative}`"
            );
        }
    }
}

#[test]
fn supported_wire_cases_round_trip_byte_identically_in_rust() {
    let corpus = semantic_corpus::parse_and_validate(include_str!(
        "../../rsjs_conformance/semantic-corpus.json"
    ))
    .expect("semantic corpus should validate");
    let output = semantic_corpus::evaluate(&corpus).expect("Rust corpus should evaluate");
    let results = output["results"]
        .as_array()
        .expect("Rust evaluator emits results");

    for result in results {
        let value = &result["value"];
        let Some(wire) = value.get("wire").and_then(Value::as_str) else {
            continue;
        };
        assert_eq!(
            value.get("roundtrip_wire").and_then(Value::as_str),
            Some(wire),
            "wire case `{}` was not byte-stable across Rust decode/encode",
            result["id"].as_str().unwrap_or("<missing>")
        );
        let parsed: Value =
            serde_json::from_str(wire).expect("wire corpus result should be valid JSON");
        assert_eq!(
            value.get("semantic"),
            Some(&parsed),
            "wire semantic value should match the emitted JSON"
        );
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rsjs lives directly under the workspace root")
        .to_path_buf()
}
