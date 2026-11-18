// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_config::{
    ApplicationConfig, AssetConfig, BuildConfig, Config, DatabaseConfig, DatabaseDriver,
    DeployConfig, HtmlRepresentationConfig, MarkdownRepresentationConfig, ProjectPaths,
    RepresentationsConfig, Result, ScriptsConfig, StylingConfig, UiConfig,
};

pub async fn config() -> Result<Config> {
    Ok(Config {
        application: Some(ApplicationConfig {
            binary: "mcp-incident-desk".into(),
            package: "mcp-incident-desk".into(),
            kind: "site".into(),
            starter: "blank".into(),
            template: "app".into(),
        }),
        database: Some(DatabaseConfig {
            driver: DatabaseDriver::Sqlite,
            migrations: Some("migrations".into()),
            run_migrations_on_startup: Some(true),
            ..Default::default()
        }),
        ui: Some(UiConfig {
            provider: "proa".into(),
        }),
        representations: Some(RepresentationsConfig {
            html: Some(HtmlRepresentationConfig { enabled: true }),
            markdown: Some(MarkdownRepresentationConfig::site()),
        }),
        styling: Some(StylingConfig {
            kind: "tailwind".into(),
            input: Some("styles/input.css".into()),
            output: Some("public/styles.css".into()),
        }),
        deploy: Some(DeployConfig {
            provider: "none".into(),
            ..Default::default()
        }),
        registries: std::collections::BTreeMap::from([(
            "@proa".into(),
            "https://ui.proa.so/r/{name}.json".into(),
        )]),
        paths: ProjectPaths {
            components: "src/components".into(),
            assets: "public/proa".into(),
            pages: Some("src/pages".into()),
            endpoints: Some("src/endpoints".into()),
            layouts: Some("src/layouts".into()),
            models: Some("src/models".into()),
            public: Some("public".into()),
            runtime_data: Some("data".into()),
        },
        scripts: ScriptsConfig {
            dev: "bash scripts/dev.sh",
            build: "bash scripts/build.sh",
            ..Default::default()
        },
        build: BuildConfig {
            assets: Some(vec![AssetConfig {
                kind: "tailwind".into(),
                command: vec![],
                input: "styles/input.css".into(),
                output: "public/styles.css".into(),
                config: None,
            }]),
            ..Default::default()
        },
        ..Config::default()
    })
}
