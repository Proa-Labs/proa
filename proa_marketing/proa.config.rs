// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa::config::{BuildConfig, Config, Result};

pub async fn config() -> Result<Config> {
    Ok(Config {
        build: BuildConfig {
            // The marketing watch and release scripts already build the
            // stylesheet with their pinned Tailwind and explicit config.
            assets: Some(Vec::new()),
            ..BuildConfig::default()
        },
        ..Config::default()
    })
}
