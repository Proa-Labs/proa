// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use const_json::{ConstJson, JsonBuffer};

const STATIC_NAME: &str = "counter";
const STATIC_MESSAGE: &str = "Line1\n\"quoted\"";
const STATIC_LABEL: &str = "static-collection";
const STATIC_TAGS: [&str; 3] = ["alpha", "beta", "gamma"];
const STATIC_GRID: [[&str; 2]; 2] = [["r1c1", "r1c2"], ["r2c1", "r2c2"]];

#[derive(ConstJson, Copy, Clone)]
#[const_json(capacity = 96)]
struct Numbers {
    name: &'static str,
    count: u32,
    offset: i32,
    flag: bool,
}

#[derive(ConstJson)]
#[const_json(capacity = 64)]
struct Escapes {
    message: &'static str,
}

#[derive(ConstJson)]
#[const_json(capacity = 192)]
struct Collections {
    label: &'static str,
    tags: [&'static str; 3],
    grid: [[&'static str; 2]; 2],
}

#[derive(ConstJson)]
#[const_json(capacity = 256)]
struct Report {
    header: Numbers,
    footer: Escapes,
    entries: [Numbers; 2],
}

const PRIMARY_NUMBERS: Numbers = Numbers {
    name: "primary",
    count: 7,
    offset: 1,
    flag: true,
};

const SECONDARY_NUMBERS: Numbers = Numbers {
    name: "secondary",
    count: 3,
    offset: -1,
    flag: false,
};

const ENTRY_LIST: [Numbers; 2] = [PRIMARY_NUMBERS, SECONDARY_NUMBERS];

const fn build_numbers() -> JsonBuffer<96> {
    Numbers {
        name: STATIC_NAME,
        count: 9000,
        offset: -12,
        flag: false,
    }
    .to_json()
}

const fn build_escapes() -> JsonBuffer<64> {
    Escapes {
        message: STATIC_MESSAGE,
    }
    .to_json()
}

const fn build_collections() -> JsonBuffer<192> {
    Collections {
        label: STATIC_LABEL,
        tags: STATIC_TAGS,
        grid: STATIC_GRID,
    }
    .to_json()
}

const fn build_report() -> JsonBuffer<256> {
    Report {
        header: PRIMARY_NUMBERS,
        footer: Escapes { message: "done" },
        entries: ENTRY_LIST,
    }
    .to_json()
}

#[test]
fn derives_numbers_struct() {
    const BUF: JsonBuffer<96> = build_numbers();
    assert_eq!(
        BUF.as_str(),
        r#"{"name":"counter","count":9000,"offset":-12,"flag":false}"#
    );
}

#[test]
fn derives_string_escaping() {
    const BUF: JsonBuffer<64> = build_escapes();
    assert_eq!(BUF.as_str(), r#"{"message":"Line1\n\"quoted\""}"#);
}

#[test]
fn derives_static_arrays() {
    const BUF: JsonBuffer<192> = build_collections();
    assert_eq!(
        BUF.as_str(),
        r#"{"label":"static-collection","tags":["alpha","beta","gamma"],"grid":[["r1c1","r1c2"],["r2c1","r2c2"]]}"#
    );
}

#[test]
fn derives_nested_structs() {
    const BUF: JsonBuffer<256> = build_report();
    assert_eq!(
        BUF.as_str(),
        r#"{"header":{"name":"primary","count":7,"offset":1,"flag":true},"footer":{"message":"done"},"entries":[{"name":"primary","count":7,"offset":1,"flag":true},{"name":"secondary","count":3,"offset":-1,"flag":false}]}"#
    );
}
