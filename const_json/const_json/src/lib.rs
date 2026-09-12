// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![no_std]

//! Const-only JSON serialization and parsing runtime helpers.
//!
//! This crate pairs with the `ConstJson` derive macro to generate
//! fully `const fn` JSON serialization with zero heap allocation.
//!
//! It also provides compile-time JSON parsing via `const fn`.

pub use const_json_macros::ConstJson;

mod buffer;
pub mod parse;
pub mod ser;

pub use buffer::JsonBuffer;
pub use parse::{JsonObject, JsonValue, ParseError};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use crate as const_json;

    #[derive(ConstJson)]
    #[const_json(capacity = 64)]
    struct Example<'a> {
        title: &'a str,
        count: u32,
        enabled: bool,
    }

    const fn build_example() -> JsonBuffer<64> {
        Example {
            title: "example",
            count: 42,
            enabled: true,
        }
        .to_json()
    }

    #[test]
    fn generates_expected_json() {
        const BUF: JsonBuffer<64> = build_example();
        assert_eq!(
            BUF.as_str(),
            r#"{"title":"example","count":42,"enabled":true}"#
        );
    }
}
