// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_config::{BuildConfig, Config, DevConfig, ProjectPaths, Result, ScriptsConfig};

pub async fn config() -> Result<Config> {
    Ok(Config {
        application: Some(
            proa_config::json::json!({ "binary": "mcp-incident-desk", "kind": "site", "package": "mcp-incident-desk", "starter": "blank", "template": "app" }),
        ),
        database: Some(
            proa_config::json::json!({ "driver": "sqlite", "migrations": "migrations", "runMigrationsOnStartup": true }),
        ),
        ui: Some(proa_config::json::json!({ "provider": "proa" })),
        representations: Some(
            proa_config::json::json!({ "html": { "enabled": true }, "markdown": { "enabled": true, "explicitSuffix": ".md", "negotiateOnCanonicalPath": true, "rootPath": "/index.md" } }),
        ),
        styling: Some(
            proa_config::json::json!({ "input": "styles/input.css", "kind": "tailwind", "output": "public/styles.css" }),
        ),
        deploy: Some(proa_config::json::json!({ "provider": "none" })),
        test: Some(proa_config::json::json!({ "before": [] })),
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
        dev: DevConfig {
            host: "127.0.0.1".into(),
            port: 5570,
            before: vec![],
            watch: vec![
                "src".into(),
                "styles".into(),
                "migrations".into(),
                "Cargo.toml".into(),
                "build.rs".into(),
                "proa.config.rs".into(),
                "proa.lock.json".into(),
                "public/incident.css".into(),
            ],
        },
        build: BuildConfig {
            before: vec![],
            assets: Some(vec![proa_config::AssetConfig {
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
