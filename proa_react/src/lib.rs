// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod bootstrap;
mod builder;
mod children;
mod island;
mod island_def;
mod js;

pub use bootstrap::{
    react_bootstrap, render_import_map_entries, render_react_bootstrap, render_react_import_map,
    HydrationStrategy, ReactBootstrap, ReactBootstrapConfig, ReactModuleDescriptor,
};
pub use builder::{ErrorBoundaryConfig, Island, IslandStrategy};
pub use children::Children;
pub use island::{react_island, react_island_with_props, ReactIsland};
pub use island_def::IslandDef;

// These versions define the default CDN runtime used by this crate. Keep them
// explicit so a published `proa_react` archive is self-contained and does not
// depend on a JavaScript workspace manifest outside the crate.
const REACT_VERSION: &str = "19.0.0";
const REACT_DOM_VERSION: &str = "19.0.0";

// Helper to concatenate strings at compile time
const fn const_concat(parts: &[&str]) -> ([u8; 256], usize) {
    let mut buf = [0u8; 256];
    let mut pos = 0;

    let mut part_idx = 0;
    while part_idx < parts.len() {
        let bytes = parts[part_idx].as_bytes();
        let mut i = 0;
        while i < bytes.len() && pos < 256 {
            buf[pos] = bytes[i];
            pos += 1;
            i += 1;
        }
        part_idx += 1;
    }

    (buf, pos)
}

// Generate quarterly cache-bust timestamp at compile time
const fn get_build_quarter() -> &'static str {
    // Parse compile time from env!("BUILD_TIMESTAMP") or use a fixed quarter
    // For now, we'll use a const approach based on compile-time month
    // This will be set at build time and remain constant for the binary
    const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

    // Parse YYYY-MM format to extract year and month
    let bytes = BUILD_TIMESTAMP.as_bytes();

    // Extract month from the build timestamp's `YYYY-MM` prefix.
    let month_tens = (bytes[5] - b'0') as usize;
    let month_ones = (bytes[6] - b'0') as usize;
    let month = month_tens * 10 + month_ones;

    // Determine quarter based on month
    match month {
        1..=3 => "Q1",
        4..=6 => "Q2",
        7..=9 => "Q3",
        _ => "Q4",
    }
}

// Build the cache-bust parameter at compile time
const CACHE_BUST_QUARTER: &str = get_build_quarter();

// Build CDN URLs at compile time with quarterly cache-busting
const REACT_URL_PARTS: ([u8; 256], usize) = const_concat(&[
    "https://esm.sh/react@",
    REACT_VERSION,
    "?ts=",
    env!("BUILD_YEAR"),
    "-",
    CACHE_BUST_QUARTER,
]);
const REACT_DOM_URL_PARTS: ([u8; 256], usize) = const_concat(&[
    "https://esm.sh/react-dom@",
    REACT_DOM_VERSION,
    "/client?ts=",
    env!("BUILD_YEAR"),
    "-",
    CACHE_BUST_QUARTER,
]);

// Helper to convert const array to &str
const fn url_from_parts(parts: &([u8; 256], usize)) -> &str {
    let (buf, len) = parts;
    let ptr = buf.as_ptr();
    // SAFETY: We built this from valid UTF-8 strings
    unsafe {
        let slice = core::slice::from_raw_parts(ptr, *len);
        core::str::from_utf8_unchecked(slice)
    }
}

/// Default React CDN URL.
pub const DEFAULT_REACT_URL: &str = url_from_parts(&REACT_URL_PARTS);

/// Default ReactDOM CDN URL.
pub const DEFAULT_REACT_DOM_URL: &str = url_from_parts(&REACT_DOM_URL_PARTS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_urls_generated_correctly() {
        assert!(DEFAULT_REACT_URL.starts_with("https://esm.sh/react@"));
        assert!(DEFAULT_REACT_DOM_URL.starts_with("https://esm.sh/react-dom@"));

        // Should have actual versions, not fallback
        println!("React URL: {}", DEFAULT_REACT_URL);
        println!("ReactDOM URL: {}", DEFAULT_REACT_DOM_URL);

        // Verify versions match
        assert_eq!(REACT_VERSION, "19.0.0");
        assert_eq!(REACT_DOM_VERSION, "19.0.0");

        // Verify quarterly cache-bust parameter is present
        assert!(
            DEFAULT_REACT_URL.contains("?ts="),
            "React URL should contain quarterly cache-bust parameter"
        );
        assert!(
            DEFAULT_REACT_DOM_URL.contains("?ts="),
            "ReactDOM URL should contain quarterly cache-bust parameter"
        );

        // Verify quarter format (should be Q1, Q2, Q3, or Q4)
        assert!(
            DEFAULT_REACT_URL.contains("-Q1")
                || DEFAULT_REACT_URL.contains("-Q2")
                || DEFAULT_REACT_URL.contains("-Q3")
                || DEFAULT_REACT_URL.contains("-Q4"),
            "React URL should contain valid quarter suffix"
        );
    }
}
