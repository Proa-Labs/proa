// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_config::{
    ApplicationConfig, AssetConfig, BuildConfig, Config, DatabaseConfig, DatabaseDriver,
    DeployConfig, MarkdownRepresentationConfig, ProjectPaths, RegistriesConfig,
    RepresentationsConfig, Result, ScriptsConfig, StylingConfig, UiConfig,
};

pub async fn config() -> Result<Config> {
    Ok(Config {
        application: Some(ApplicationConfig {
            binary: "mcp-incident-desk",
            package: "mcp-incident-desk",
            starter: "blank",
            template: "app",
            ..Default::default()
        }),
        database: Some(DatabaseConfig {
            driver: DatabaseDriver::Sqlite,
            ..Default::default()
        }),
        ui: Some(UiConfig { provider: "proa" }),
        representations: Some(RepresentationsConfig {
            markdown: Some(MarkdownRepresentationConfig {
                enabled: true,
                ..Default::default()
            }),
            ..Default::default()
        }),
        styling: Some(StylingConfig {
            kind: "tailwind",
            input: Some("styles/input.css"),
            output: Some("public/styles.css"),
        }),
        deploy: Some(DeployConfig {
            provider: "none",
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
        build: BuildConfig {
            assets: Some(vec![AssetConfig {
                kind: "tailwind",
                input: "styles/input.css",
                output: "public/styles.css",
                ..Default::default()
            }]),
            ..Default::default()
        },
        ..Config::default()
    })
}
