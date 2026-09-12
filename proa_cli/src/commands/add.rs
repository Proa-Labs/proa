// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Incremental, idempotent capability installation for existing applications.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use crate::{config, util::resolve_config_root};

#[derive(Debug, Args)]
pub(crate) struct AddArgs {
    #[command(subcommand)]
    capability: AddCapability,
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[arg(long, global = true)]
    dry_run: bool,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    yes: bool,
    #[arg(long, global = true)]
    offline: bool,
}

#[derive(Debug, Subcommand)]
enum AddCapability {
    /// Add SQLite and SQLx project conventions.
    Database {
        #[arg(value_enum)]
        driver: Database,
    },
    /// Add a page representation policy.
    Representation {
        #[arg(value_enum)]
        representation: Representation,
    },
    /// Add the source-owned Proa UI convention.
    Ui,
    /// Add a Proa Docs content root.
    Docs,
    /// Add a blog content root.
    Blog,
    /// Add portable deployment files and optional provider metadata.
    Deploy {
        #[arg(value_enum)]
        provider: Deployment,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Database {
    Sqlite,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Representation {
    Markdown,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Deployment {
    Docker,
    Railway,
    Coolify,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AddReport {
    operation_id: &'static str,
    command_version: u16,
    project_root: PathBuf,
    planned_filesystem_operations: Vec<Operation>,
    configuration_changes: Vec<String>,
    warnings: Vec<String>,
    result: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Operation {
    operation: &'static str,
    path: PathBuf,
    bytes: usize,
}

pub(crate) fn run(args: AddArgs) -> Result<()> {
    let root = resolve_config_root(args.root)?;
    let _accept_defaults = args.yes;
    let _offline = args.offline;
    let config_path = root.join(config::CONFIG_FILE);
    let lock_path = root.join(config::LOCK_FILE);
    let mut config_value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&config_path)?)?;
    let mut lock_value: serde_json::Value = if lock_path.exists() {
        serde_json::from_str(&fs::read_to_string(&lock_path)?)?
    } else {
        serde_json::json!({"schemaVersion":1,"scaffold":{"capabilities":[]},"components":{}})
    };
    let package = config_value
        .pointer("/application/package")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("app")
        .to_string();
    let has_database = config_value.pointer("/database/driver").is_some();
    let mut files = Vec::<(PathBuf, String)>::new();
    let mut warnings = Vec::new();
    let (id, changes, partial) = match args.capability {
        AddCapability::Database {
            driver: Database::Sqlite,
        } => {
            set_pointer(
                &mut config_value,
                "database",
                serde_json::json!({"driver":"sqlite","migrations":"migrations","runMigrationsOnStartup":true,"integration":"manual"}),
            )?;
            add_sqlx_dependency(&root, &mut files)?;
            files.push((PathBuf::from("data/.gitkeep"), String::new()));
            files.push((PathBuf::from("migrations/.gitkeep"), String::new()));
            files.push((PathBuf::from("src/db.rs"), database_module()));
            warnings.push("Database files were added without rewriting arbitrary router/state code; add `mod db;`, call `db::connect`, and place the pool in application state.".to_string());
            (
                "database:sqlite",
                vec!["database.driver=sqlite".to_string()],
                true,
            )
        }
        AddCapability::Representation {
            representation: Representation::Markdown,
        } => {
            let representations = config_value
                .as_object_mut()
                .context("project config must be an object")?
                .entry("representations")
                .or_insert_with(|| serde_json::json!({}));
            representations.as_object_mut().context("representations must be an object")?.insert("markdown".to_string(), serde_json::json!({"enabled":true,"negotiateOnCanonicalPath":true,"explicitSuffix":".md","rootPath":"/index.md"}));
            (
                "representation:markdown",
                vec!["representations.markdown.enabled=true".to_string()],
                false,
            )
        }
        AddCapability::Ui => {
            set_pointer(
                &mut config_value,
                "ui",
                serde_json::json!({"provider":"proa"}),
            )?;
            config_value
                .as_object_mut()
                .unwrap()
                .entry("registries")
                .or_insert_with(|| serde_json::json!({"@proa":"https://ui.proa.so/r/{name}.json"}));
            files.push((PathBuf::from("src/components/mod.rs"), String::new()));
            ("ui:proa", vec!["ui.provider=proa".to_string()], false)
        }
        AddCapability::Docs => {
            set_content_capability(&mut config_value, "docs")?;
            files.push((
                PathBuf::from("src/docs/index.md"),
                "# Documentation\n\nStart writing here.\n".to_string(),
            ));
            (
                "content:docs",
                vec!["content.docs.enabled=true".to_string()],
                false,
            )
        }
        AddCapability::Blog => {
            set_content_capability(&mut config_value, "blog")?;
            files.push((
                PathBuf::from("src/blog/first-post.md"),
                "# First post\n\nWrite your first post here.\n".to_string(),
            ));
            (
                "content:blog",
                vec!["content.blog.enabled=true".to_string()],
                false,
            )
        }
        AddCapability::Deploy { provider } => {
            let provider_name = match provider {
                Deployment::Docker => "docker",
                Deployment::Railway => "railway",
                Deployment::Coolify => "coolify",
            };
            set_pointer(
                &mut config_value,
                "deploy",
                serde_json::json!({"provider":provider_name}),
            )?;
            files.extend(deployment_files(provider, &package, has_database));
            (
                match provider {
                    Deployment::Docker => "deploy:docker",
                    Deployment::Railway => "deploy:railway",
                    Deployment::Coolify => "deploy:coolify",
                },
                vec![format!("deploy.provider={provider_name}")],
                false,
            )
        }
    };
    add_capability_lock(&mut lock_value, id, partial)?;
    let config_source = format!("{}\n", serde_json::to_string_pretty(&config_value)?);
    let lock_source = format!("{}\n", serde_json::to_string_pretty(&lock_value)?);
    let mut mutations = vec![
        (PathBuf::from(config::CONFIG_FILE), config_source),
        (PathBuf::from(config::LOCK_FILE), lock_source),
    ];
    for (relative, source) in files {
        if root.join(&relative).exists() && relative != Path::new("Cargo.toml") {
            continue;
        }
        mutations.push((relative, source));
    }
    let report = AddReport {
        operation_id: "capability.add",
        command_version: 1,
        project_root: root.clone(),
        planned_filesystem_operations: mutations
            .iter()
            .map(|(path, source)| Operation {
                operation: if root.join(path).exists() {
                    "update"
                } else {
                    "create"
                },
                path: path.clone(),
                bytes: source.len(),
            })
            .collect(),
        configuration_changes: changes,
        warnings,
        result: if args.dry_run {
            "planned"
        } else if partial {
            "partial"
        } else {
            "applied"
        },
    };
    if !args.dry_run {
        commit(&root, &mutations)?;
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{} capability {id}",
            if args.dry_run { "Would add" } else { "Added" }
        );
        for operation in &report.planned_filesystem_operations {
            println!("  {:<8} {}", operation.operation, operation.path.display());
        }
        for warning in &report.warnings {
            eprintln!("warning: {warning}");
        }
    }
    Ok(())
}

fn set_pointer(config: &mut serde_json::Value, key: &str, value: serde_json::Value) -> Result<()> {
    config
        .as_object_mut()
        .context("project config must be an object")?
        .insert(key.to_string(), value);
    Ok(())
}

fn set_content_capability(config: &mut serde_json::Value, name: &str) -> Result<()> {
    let content = config
        .as_object_mut()
        .context("project config must be an object")?
        .entry("content")
        .or_insert_with(|| serde_json::json!({}));
    content
        .as_object_mut()
        .context("content config must be an object")?
        .insert(name.to_string(), serde_json::json!({ "enabled": true }));
    Ok(())
}

fn add_capability_lock(lock: &mut serde_json::Value, id: &str, partial: bool) -> Result<()> {
    let object = lock
        .as_object_mut()
        .context("lock file must be an object")?;
    let scaffold = object
        .entry("scaffold")
        .or_insert_with(|| serde_json::json!({"capabilities":[]}));
    let capabilities = scaffold
        .as_object_mut()
        .context("lock scaffold must be an object")?
        .entry("capabilities")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .context("lock capabilities must be an array")?;
    if !capabilities
        .iter()
        .any(|value| value.get("id").and_then(serde_json::Value::as_str) == Some(id))
    {
        capabilities.push(serde_json::json!({
            "id": id,
            "planVersion": 1,
            "status": if partial { "partial" } else { "integrated" }
        }));
    }
    Ok(())
}

fn add_sqlx_dependency(root: &Path, files: &mut Vec<(PathBuf, String)>) -> Result<()> {
    use toml_edit::{Array, InlineTable, Item, Table, Value};

    let manifest_path = root.join("Cargo.toml");
    let source = fs::read_to_string(&manifest_path)?;
    let mut manifest = source.parse::<toml_edit::DocumentMut>()?;
    if !manifest["dependencies"].is_table() {
        manifest["dependencies"] = Item::Table(Table::new());
    }
    let dependencies = manifest["dependencies"]
        .as_table_mut()
        .context("Cargo.toml `[dependencies]` must be a table")?;
    let required = ["runtime-tokio", "sqlite", "migrate", "macros"];
    if let Some(item) = dependencies.get_mut("sqlx") {
        let table = match item {
            Item::Value(Value::String(version)) => {
                let version = version.value().to_string();
                let mut table = InlineTable::new();
                table.insert("version", Value::from(version));
                *item = Item::Value(Value::InlineTable(table));
                item.as_inline_table_mut()
                    .expect("just converted to inline table")
            }
            Item::Value(Value::InlineTable(table)) => table,
            _ => bail!("Cargo dependency `sqlx` must be a version string or inline table"),
        };
        if table.get("features").is_none() {
            table.insert("features", Value::Array(Array::new()));
        }
        let features = table
            .get_mut("features")
            .and_then(Value::as_array_mut)
            .context("Cargo dependency `sqlx` features must be an array")?;
        for feature in required {
            if !features.iter().any(|value| value.as_str() == Some(feature)) {
                features.push(feature);
            }
        }
        if table.get("default-features").is_none() {
            table.insert("default-features", Value::from(false));
        }
    } else {
        let mut table = InlineTable::new();
        table.insert("version", Value::from("0.8"));
        let mut features = Array::new();
        for feature in required {
            features.push(feature);
        }
        table.insert("features", Value::Array(features));
        table.insert("default-features", Value::from(false));
        dependencies.insert("sqlx", Item::Value(Value::InlineTable(table)));
    }
    files.push((PathBuf::from("Cargo.toml"), manifest.to_string()));
    Ok(())
}

fn database_module() -> String {
    r#"use std::{str::FromStr, time::Duration};

use sqlx::{
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};

static MIGRATOR: Migrator = sqlx::migrate!();

pub async fn connect(database_url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}
"#
    .to_string()
}

fn deployment_files(
    provider: Deployment,
    package: &str,
    has_database: bool,
) -> Vec<(PathBuf, String)> {
    let migration_copy = if has_database {
        "COPY --from=build /app/migrations ./migrations\n"
    } else {
        ""
    };
    let mut files = vec![
        (PathBuf::from("Dockerfile"), format!("FROM rust:1.93-bookworm AS build\nWORKDIR /app\nCOPY . .\nRUN --mount=type=secret,id=proa_token cargo build --release --locked\nFROM debian:bookworm-slim\nWORKDIR /app\nCOPY --from=build /app/target/release/{package} /usr/local/bin/app\nCOPY --from=build /app/public ./public\n{migration_copy}ENV PROA_HOST=0.0.0.0 PORT=3000\nEXPOSE 3000\nCMD [\"/usr/local/bin/app\"]\n")),
        (PathBuf::from(".dockerignore"), "target\n.git\ndata/*.db*\n.env\n".to_string()),
    ];
    if matches!(provider, Deployment::Railway) {
        files.push((PathBuf::from("railway.json"), "{\n  \"$schema\": \"https://railway.com/railway.schema.json\",\n  \"build\": { \"builder\": \"DOCKERFILE\" },\n  \"deploy\": { \"healthcheckPath\": \"/readyz\", \"healthcheckTimeout\": 120, \"restartPolicyType\": \"ON_FAILURE\" }\n}\n".to_string()));
        let persistence = if has_database {
            " For SQLite, mount a persistent volume at `/app/data`, use one replica, and enable tested backups."
        } else {
            ""
        };
        files.push((
            PathBuf::from("RAILWAY.md"),
            format!("# Railway\n\nRun `proa deploy railway`.{persistence}\n"),
        ));
    }
    if matches!(provider, Deployment::Coolify) {
        files.push((
            PathBuf::from("coolify.json"),
            "{\n  \"healthcheck\": \"/readyz\"\n}\n".to_string(),
        ));
        files.push((
            PathBuf::from("coolify.env.example"),
            format!(
                "PROA_HOST=0.0.0.0\nPORT=3000\n{}",
                if has_database {
                    format!("DATABASE_URL=sqlite://data/{package}.db\n")
                } else {
                    String::new()
                }
            ),
        ));
        let persistence = if has_database {
            " Mount persistent storage at `/app/data`, use one replica, and configure tested backups."
        } else {
            ""
        };
        files.push((
            PathBuf::from("COOLIFY.md"),
            format!("# Coolify\n\nBuild the Dockerfile and check `/readyz`.{persistence}\n"),
        ));
    }
    files
}

fn commit(root: &Path, mutations: &[(PathBuf, String)]) -> Result<()> {
    let mut staged = Vec::new();
    for (relative, source) in mutations {
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let file_name = target
            .file_name()
            .and_then(|value| value.to_str())
            .context("mutation filename must be UTF-8")?;
        let temporary =
            target.with_file_name(format!(".{file_name}.proa-{}.tmp", std::process::id()));
        fs::write(&temporary, source).with_context(|| format!("stage {}", target.display()))?;
        staged.push((temporary, target));
    }
    for (temporary, target) in staged {
        fs::rename(&temporary, &target).with_context(|| format!("commit {}", target.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_lock_is_idempotent() {
        let mut lock = serde_json::json!({});
        add_capability_lock(&mut lock, "ui:proa", false).unwrap();
        add_capability_lock(&mut lock, "ui:proa", false).unwrap();
        assert_eq!(
            lock.pointer("/scaffold/capabilities")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
