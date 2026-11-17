// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, fs, sync::OnceLock};

use proa_cli::{
    capabilities::CapabilityOverrides,
    config::{
        generated_project_config, load_project_config_with_unknown_keys, AssetConfig, BuildConfig,
        DevConfig, ProjectConfig, ProjectPaths, CONFIG_FILE, CONFIG_SCHEMA_URL,
    },
    site_scaffold::{new_site, NewSiteOptions},
};
use serde_json::{json, Value};

fn schema() -> Value {
    serde_json::from_str(include_str!("../schemas/proa.config.json")).unwrap()
}

fn validator() -> &'static jsonschema::Validator {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| jsonschema::validator_for(&schema()).unwrap())
}

fn assert_supported(value: &Value) -> ProjectConfig {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join(CONFIG_FILE),
        serde_json::to_vec(value).unwrap(),
    )
    .unwrap();
    let (config, unknown) = load_project_config_with_unknown_keys(temp.path()).unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    let errors: Vec<_> = validator()
        .iter_errors(value)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{value}: {errors:#?}");
    config
}

// Explicit struct literals make additions to the Rust model require a schema
// parity decision, including fields that serialization would normally omit.
fn populated_config() -> ProjectConfig {
    ProjectConfig {
        schema: Some(CONFIG_SCHEMA_URL.into()),
        application: Some(json!({ "binary": "site" })),
        database: Some(json!({ "driver": "sqlite" })),
        ui: Some(json!({ "provider": "proa" })),
        representations: Some(json!({ "markdown": { "enabled": true } })),
        styling: Some(json!({ "kind": "tailwind" })),
        deploy: Some(json!({ "provider": "docker" })),
        content: Some(json!({ "directory": "content" })),
        tasks: [("prepare".into(), json!({ "command": ["prepare"] }))].into(),
        test: Some(json!({ "before": ["prepare"] })),
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
        registries: [
            ("@proa".into(), "https://ui.proa.so/r/{name}.json".into()),
            ("@company".into(), "registry/{name}.json".into()),
        ]
        .into(),
        dev: DevConfig {
            command: vec!["cargo".into(), "run".into()],
            host: "127.0.0.1".into(),
            port: 3000,
            before: vec!["prepare".into()],
            watch: vec!["src".into()],
        },
        build: BuildConfig {
            command: vec!["cargo".into(), "build".into()],
            before: vec!["prepare".into()],
            assets: Some(vec![AssetConfig {
                kind: "command".into(),
                command: vec!["build-assets".into()],
                input: "assets".into(),
                output: "public/assets".into(),
                config: Some("assets.config.json".into()),
            }]),
        },
    }
}

#[test]
fn schema_covers_every_serialized_project_config_field() {
    let config = serde_json::to_value(populated_config()).unwrap();
    assert_supported(&config);

    let schema = schema();
    for (instance, definition) in [
        (&config, &schema),
        (&config["paths"], &schema["properties"]["paths"]),
        (&config["dev"], &schema["properties"]["dev"]),
        (&config["build"], &schema["properties"]["build"]),
        (&config["build"]["assets"][0], &schema["$defs"]["asset"]),
    ] {
        let fields: BTreeSet<_> = instance.as_object().unwrap().keys().collect();
        let properties: BTreeSet<_> = definition["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(fields, properties);
    }
}

#[test]
fn schema_accepts_defaulted_and_nullable_fields() {
    for value in [
        json!({}),
        json!({ "paths": {}, "registries": {}, "dev": {}, "build": {} }),
        json!({ "build": { "assets": [{}] } }),
        json!({
            "$schema": null,
            "paths": {
                "pages": null, "endpoints": null, "layouts": null,
                "models": null, "public": null, "runtimeData": null
            },
            "build": { "assets": null }
        }),
        json!({ "build": { "assets": [{ "config": null }] } }),
    ] {
        assert_supported(&value);
    }

    let defaults = assert_supported(&json!({}));
    assert_eq!(defaults.dev.port, DevConfig::default().port);
    assert_eq!(
        defaults.paths.components,
        ProjectPaths::default().components
    );
    assert!(defaults.build.assets.is_none());
    let explicit_empty = assert_supported(&json!({ "build": { "assets": [] } }));
    assert!(explicit_empty.build.assets.unwrap().is_empty());
}

#[test]
fn schema_preserves_opaque_values_and_named_maps() {
    for value in [
        Value::Null,
        json!(false),
        json!(42),
        json!("preset"),
        json!(["extension"]),
        json!({ "extension": { "nested": true } }),
    ] {
        for field in [
            "application",
            "database",
            "ui",
            "representations",
            "styling",
            "deploy",
            "content",
            "test",
        ] {
            assert_supported(&json!({ (field): value.clone() }));
        }
        assert_supported(&json!({ "tasks": { "custom": value } }));
    }
    assert_supported(&json!({
        "registries": {
            "@proa": "https://ui.proa.so/r/{name}.json",
            "@private": "file:///registry/{name}.json"
        }
    }));
}

#[test]
fn schema_keeps_typed_objects_closed() {
    for value in [
        json!({ "applicaton": {} }),
        json!({ "paths": { "runtime_data": "data" } }),
        json!({ "dev": { "befroe": [] } }),
        json!({ "build": { "befroe": [] } }),
        json!({ "build": { "assets": [{ "ouput": "public" }] } }),
    ] {
        assert!(!validator().is_valid(&value), "{value}");
        let mut unknown = Vec::new();
        let source = serde_json::to_string(&value).unwrap();
        let mut deserializer = serde_json::Deserializer::from_str(&source);
        let _: ProjectConfig = serde_ignored::deserialize(&mut deserializer, |path| {
            unknown.push(path.to_string());
        })
        .unwrap();
        assert!(!unknown.is_empty(), "{value}");
    }
}

#[test]
fn schema_rejects_incorrect_types_and_retains_existing_constraints() {
    for value in [
        json!({ "tasks": [] }),
        json!({ "tasks": null }),
        json!({ "paths": { "pages": false } }),
        json!({ "registries": { "@private": 123 } }),
        json!({ "dev": { "before": [false] } }),
        json!({ "build": { "before": null } }),
        json!({ "build": { "assets": {} } }),
        json!({ "dev": { "port": 65536 } }),
    ] {
        assert!(serde_json::from_value::<ProjectConfig>(value.clone()).is_err());
        assert!(!validator().is_valid(&value), "{value}");
    }
    for value in [
        json!({ "paths": { "assets": "../outside" } }),
        json!({ "paths": { "components": "/outside" } }),
        json!({ "registries": { "@private": "https://example.com/component.json" } }),
        json!({ "build": { "assets": [{ "kind": "unknown" }] } }),
    ] {
        assert!(!validator().is_valid(&value), "{value}");
    }
}

#[test]
fn schema_accepts_generated_configs_and_every_site_preset() {
    for tailwind in [false, true] {
        assert_supported(
            &serde_json::from_str(&generated_project_config(tailwind).unwrap()).unwrap(),
        );
    }

    let temp = tempfile::tempdir().unwrap();
    for template in ["minimal", "app", "marketing", "docs", "origin"] {
        let root = temp.path().join(template);
        new_site(NewSiteOptions {
            path: root.clone(),
            destination: None,
            title: None,
            template: template.into(),
            overrides: CapabilityOverrides::default(),
            vscode: false,
            agents_md: false,
            no_git: true,
            verify: false,
            start: false,
            open: false,
            offline: true,
            dry_run: None,
            json: false,
        })
        .unwrap();
        let value = serde_json::from_slice(&fs::read(root.join(CONFIG_FILE)).unwrap()).unwrap();
        assert_supported(&value);
    }
}
