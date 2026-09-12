// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[path = "../../rsjs_conformance/rust/semantic_corpus.rs"]
mod semantic_corpus;

fn main() {
    let source = include_str!("../../rsjs_conformance/semantic-corpus.json");
    let corpus = semantic_corpus::parse_and_validate(source)
        .unwrap_or_else(|err| panic!("semantic corpus validation failed: {err}"));
    let output = semantic_corpus::evaluate(&corpus)
        .unwrap_or_else(|err| panic!("semantic corpus evaluation failed: {err}"));
    println!(
        "{}",
        serde_json::to_string(&output).expect("semantic corpus output is serializable")
    );
}
