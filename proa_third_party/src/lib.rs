// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Optional third-party integrations for Proa's HTML and Markdown renderers.
//!
//! Scripts use a small, CSP-aware browser adapter and do not require an RSJS
//! runtime or hydration. Nothing is emitted until a component is rendered.

mod script;
pub use script::{InvalidScriptUrl, Script, ScriptStrategy};

#[cfg(feature = "google-analytics")]
pub mod google;
#[cfg(feature = "youtube")]
pub mod youtube;

/// Shared by the script and provider bootstraps; installed once per document.
pub(crate) const LOADER: &str = include_str!("loader.js");

#[cfg(test)]
mod tests {
    #[test]
    fn browser_adapters_obey_loading_and_consent_contracts() {
        let output = std::process::Command::new("node")
            .arg("--test")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/runtime.test.mjs"
            ))
            .output()
            .expect("Node.js 22+ is required to test the browser adapters");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
