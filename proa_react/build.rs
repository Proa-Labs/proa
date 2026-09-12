// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

fn main() {
    // Get current time at build
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards");

    // Convert to year and month
    let secs = duration.as_secs();

    // This approximation is sufficient for quarterly granularity.
    let days = secs / 86400;
    let years_since_1970 = days / 365;
    let year = 1970 + years_since_1970;

    // Calculate month (approximate)
    let days_this_year = days % 365;
    let month = (days_this_year / 30) + 1;
    let month = month.min(12); // Cap at 12

    // Set environment variables for const evaluation
    println!("cargo:rustc-env=BUILD_YEAR={}", year);
    println!("cargo:rustc-env=BUILD_TIMESTAMP={:04}-{:02}", year, month);

    // Rerun if build.rs changes
    println!("cargo:rerun-if-changed=build.rs");
}
