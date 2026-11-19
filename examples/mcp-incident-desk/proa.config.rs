// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa::config::{
    ApplicationConfig, Config, DatabaseConfig, DatabaseDriver, ProjectPaths, RegistriesConfig,
    Result, ScriptsConfig,
};

pub async fn config() -> Result<Config> {
    Ok(Config {
        application: Some(ApplicationConfig {
            binary: "mcp-incident-desk",
        }),
        database: Some(DatabaseConfig {
            driver: DatabaseDriver::Sqlite,
            ..Default::default()
        }),
        registries: RegistriesConfig::public_only(),
        paths: ProjectPaths {
            public: Some("public"),
            ..Default::default()
        },
        scripts: ScriptsConfig {
            dev: "bash scripts/dev.sh",
            build: "bash scripts/build.sh",
            ..Default::default()
        },
        ..Config::default()
    })
}
