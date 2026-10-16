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

/// Configure a linked core or query runtime for this deployment at startup.
/// Query/mutation request URLs and the browser `app_url` helper share the mount.
/// The source stays borrowed for origin-root deployments.
pub fn with_base_path<'a>(
    source: &'a str,
    base_path: &proa_core::BasePath,
) -> std::borrow::Cow<'a, str> {
    if base_path.as_str().is_empty() {
        return std::borrow::Cow::Borrowed(source);
    }
    let value = serde_json::to_string(base_path.as_str()).expect("serialize base path");
    std::borrow::Cow::Owned(source.replacen(
        "const PROA_BASE_PATH = \"\";",
        &format!("const PROA_BASE_PATH = {value};"),
        1,
    ))
}

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
