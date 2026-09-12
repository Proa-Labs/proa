// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Framework-owned RSJS runtime sources.
//!
//! Custom adapters can serve these bytes directly or fingerprint and rewrite
//! them into an application asset pipeline. Keeping the sources behind the
//! `rsjs` crate API avoids checkout-relative `include_str!` paths in consumers.

/// Core signal, hydration, DOM-binding, and lifecycle runtime source.
pub const CORE_JS: &str = include_str!("../runtime/rsjs-runtime.js");

/// Optional query and mutation runtime source.
pub const QUERY_JS: &str = include_str!("../runtime/rsjs-runtime-query.js");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_runtime_sources_expose_the_expected_modules() {
        assert!(CORE_JS.contains("export"));
        assert!(QUERY_JS.contains("export"));
        assert!(QUERY_JS.contains("Query"));
    }
}
