// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Native database lifecycle commands for application-first Proa sites.

use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde::Serialize;
use sqlx::{
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row,
};

use crate::{config, util::resolve_config_root};

#[derive(Debug, Args)]
pub(crate) struct DbArgs {
    #[command(subcommand)]
    command: DbCommand,
}

#[derive(Debug, Subcommand)]
enum DbCommand {
    /// Create the configured SQLite database without running migrations.
    Create(ConnectionArgs),
    /// Apply every pending migration.
    Migrate(MigrateArgs),
    /// Revert reversible migrations.
    Rollback(RollbackArgs),
    /// Safely recreate the local database and apply migrations.
    Reset(DestructiveArgs),
    /// Safely remove the local database and its exact WAL/SHM companions.
    Drop(DropArgs),
    /// Show applied and pending migrations.
    Status(StatusArgs),
}

#[derive(Debug, Args)]
struct ConnectionArgs {
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    database_url: Option<String>,
}

#[derive(Debug, Args)]
struct MigrateArgs {
    #[command(flatten)]
    connection: ConnectionArgs,
    /// Stop after this migration version.
    #[arg(long)]
    target: Option<i64>,
}

#[derive(Debug, Args)]
struct RollbackArgs {
    #[command(flatten)]
    connection: ConnectionArgs,
    /// Number of applied migrations to revert.
    #[arg(long, default_value_t = 1, conflicts_with = "to")]
    steps: usize,
    /// Revert to this migration version.
    #[arg(long)]
    to: Option<i64>,
    /// Confirm the resolved rollback plan.
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Args)]
struct DestructiveArgs {
    #[command(flatten)]
    connection: ConnectionArgs,
    /// Confirm the exact local database path.
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Args)]
struct DropArgs {
    #[command(flatten)]
    destructive: DestructiveArgs,
    /// Allow removing an SQLite file that still has live companions.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[command(flatten)]
    connection: ConnectionArgs,
    #[arg(long)]
    json: bool,
}

#[derive(Debug)]
struct DatabaseContext {
    root: PathBuf,
    path: PathBuf,
    migrations: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MigrationStatus {
    status: &'static str,
    version: i64,
    name: String,
    direction: &'static str,
    checksum_state: &'static str,
    applied_at: Option<String>,
}

#[derive(Debug)]
struct AppliedMigration {
    description: String,
    checksum: Vec<u8>,
    installed_on: String,
}

pub(crate) fn run(args: DbArgs) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("construct database runtime")?;
    runtime.block_on(async move {
        match args.command {
            DbCommand::Create(args) => create(resolve(args)?).await,
            DbCommand::Migrate(args) => migrate(resolve(args.connection)?, args.target).await,
            DbCommand::Rollback(args) => rollback(args).await,
            DbCommand::Reset(args) => reset(args).await,
            DbCommand::Drop(args) => drop_database(args),
            DbCommand::Status(args) => status(args).await,
        }
    })
}

fn resolve(args: ConnectionArgs) -> Result<DatabaseContext> {
    let root = resolve_config_root(args.root)?;
    let project = config::load_project_config(&root)?;
    if project
        .database
        .as_ref()
        .and_then(|database| database.get("driver"))
        .and_then(serde_json::Value::as_str)
        != Some("sqlite")
    {
        bail!("No database capability is configured.\nAdd one with: proa add database sqlite");
    }
    let configured = args
        .database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .or_else(|| env_file_value(&root.join(".env"), "DATABASE_URL"))
        .unwrap_or_else(|| {
            format!(
                "sqlite://data/{}.db",
                project
                    .application
                    .as_ref()
                    .and_then(|application| application.get("package"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("app")
            )
        });
    let relative = configured
        .strip_prefix("sqlite://")
        .or_else(|| configured.strip_prefix("sqlite:"))
        .context("only file-backed SQLite DATABASE_URL values are supported")?;
    if relative.is_empty() || relative == ":memory:" {
        bail!("database commands require a file-backed SQLite URL");
    }
    let raw_path = PathBuf::from(relative);
    let path = if raw_path.is_absolute() {
        raw_path
    } else {
        root.join(raw_path)
    };
    let migrations = project
        .database
        .as_ref()
        .and_then(|database| database.get("migrations"))
        .and_then(serde_json::Value::as_str)
        .map(|value| root.join(value))
        .unwrap_or_else(|| root.join("migrations"));
    Ok(DatabaseContext {
        root,
        path,
        migrations,
    })
}

fn env_file_value(path: &Path, name: &str) -> Option<String> {
    let source = fs::read_to_string(path).ok()?;
    source
        .lines()
        .filter_map(|line| line.trim().strip_prefix(&format!("{name}=")))
        .map(str::trim)
        .map(|value| value.trim_matches(['\'', '"']))
        .find(|value| !value.is_empty())
        .map(ToString::to_string)
}

async fn pool(context: &DatabaseContext, create: bool) -> Result<sqlx::SqlitePool> {
    if create {
        if let Some(parent) = context.path.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    let options = SqliteConnectOptions::new()
        .filename(&context.path)
        .create_if_missing(create)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    Ok(SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?)
}

async fn create(context: DatabaseContext) -> Result<()> {
    pool(&context, true).await?.close().await;
    println!(
        "Database ready at {}\nNext: proa db migrate",
        display_path(&context)
    );
    Ok(())
}

async fn migrator(context: &DatabaseContext) -> Result<Migrator> {
    Migrator::new(context.migrations.as_path())
        .await
        .with_context(|| format!("load migrations from {}", context.migrations.display()))
}

async fn migrate(context: DatabaseContext, target: Option<i64>) -> Result<()> {
    let pool = pool(&context, true).await?;
    let mut migrator = migrator(&context).await?;
    if let Some(target) = target {
        migrator.migrations = Cow::Owned(
            migrator
                .migrations
                .iter()
                .filter(|migration| migration.version <= target)
                .cloned()
                .collect(),
        );
    }
    migrator.run(&pool).await?;
    pool.close().await;
    println!("Database is up to date · {}", display_path(&context));
    Ok(())
}

async fn applied_versions(pool: &sqlx::SqlitePool) -> Result<Vec<i64>> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await?;
    if exists == 0 {
        return Ok(Vec::new());
    }
    Ok(
        sqlx::query("SELECT version FROM _sqlx_migrations WHERE success = TRUE ORDER BY version")
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| row.get::<i64, _>(0))
            .collect(),
    )
}

async fn applied_migrations(
    pool: &sqlx::SqlitePool,
) -> Result<std::collections::BTreeMap<i64, AppliedMigration>> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await?;
    if exists == 0 {
        return Ok(Default::default());
    }
    Ok(sqlx::query(
        "SELECT version, description, checksum, CAST(installed_on AS TEXT) \
         FROM _sqlx_migrations WHERE success = TRUE ORDER BY version",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        (
            row.get::<i64, _>(0),
            AppliedMigration {
                description: row.get(1),
                checksum: row.get(2),
                installed_on: row.get(3),
            },
        )
    })
    .collect())
}

async fn rollback(args: RollbackArgs) -> Result<()> {
    let context = resolve(args.connection)?;
    let pool = pool(&context, false).await?;
    let applied = applied_versions(&pool).await?;
    let target = match args.to {
        Some(target) => target,
        None => {
            let retained = applied.len().saturating_sub(args.steps);
            retained
                .checked_sub(1)
                .and_then(|index| applied.get(index))
                .copied()
                .unwrap_or(0)
        }
    };
    if !args.yes {
        bail!(
            "rollback would move {} to version {target}; rerun with --yes",
            display_path(&context)
        );
    }
    migrator(&context).await?.undo(&pool, target).await?;
    pool.close().await;
    println!("Rolled back database to version {target}");
    Ok(())
}

async fn reset(args: DestructiveArgs) -> Result<()> {
    let context = resolve(args.connection)?;
    guard_destructive(&context, args.yes)?;
    remove_database_files(&context.path)?;
    migrate(context, None).await
}

fn drop_database(args: DropArgs) -> Result<()> {
    let context = resolve(args.destructive.connection)?;
    guard_destructive(&context, args.destructive.yes)?;
    if !args.force
        && (companion(&context.path, "-wal").exists() || companion(&context.path, "-shm").exists())
    {
        bail!("database has WAL/SHM companions and may be in use; close it or rerun with --force");
    }
    remove_database_files(&context.path)?;
    println!("Database removed: {}", display_path(&context));
    Ok(())
}

fn guard_destructive(context: &DatabaseContext, yes: bool) -> Result<()> {
    if ["PROA_ENV", "APP_ENV", "RUST_ENV"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .any(|value| value.eq_ignore_ascii_case("production"))
    {
        bail!("refusing a destructive database command in a production environment");
    }
    let root = context
        .root
        .canonicalize()
        .unwrap_or_else(|_| context.root.clone());
    let candidate = context
        .path
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .map(|parent| parent.join(context.path.file_name().unwrap_or_default()))
        .unwrap_or_else(|| context.path.clone());
    if !candidate.starts_with(&root) {
        bail!(
            "refusing database outside project root: {}",
            candidate.display()
        );
    }
    if !yes {
        bail!(
            "database path: {}\nRerun with --yes to confirm this exact local file",
            candidate.display()
        );
    }
    Ok(())
}

fn companion(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.display()))
}

fn remove_database_files(path: &Path) -> Result<()> {
    for candidate in [
        path.to_path_buf(),
        companion(path, "-wal"),
        companion(path, "-shm"),
    ] {
        match fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("remove {}", candidate.display()))
            }
        }
    }
    Ok(())
}

/// Prepare the isolated SQLite database owned by one `proa test` session.
/// This bypasses `.env` resolution deliberately: the caller supplies an exact
/// path under `target/proa-tests/`, and that path is the only database touched.
pub(crate) fn prepare_test_database(root: &Path, path: &Path) -> Result<()> {
    let project = config::load_project_config(root)?;
    if project
        .database
        .as_ref()
        .and_then(|database| database.get("driver"))
        .and_then(serde_json::Value::as_str)
        != Some("sqlite")
    {
        bail!("test database setup requires the SQLite capability");
    }
    let migrations = project
        .database
        .as_ref()
        .and_then(|database| database.get("migrations"))
        .and_then(serde_json::Value::as_str)
        .map(|value| root.join(value))
        .unwrap_or_else(|| root.join("migrations"));
    let context = DatabaseContext {
        root: root.to_path_buf(),
        path: path.to_path_buf(),
        migrations,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("construct test database runtime")?;
    runtime.block_on(async move {
        let pool = pool(&context, true).await?;
        migrator(&context).await?.run(&pool).await?;
        pool.close().await;
        Result::<()>::Ok(())
    })
}

pub(crate) fn remove_test_database(path: &Path) -> Result<()> {
    remove_database_files(path)
}

async fn status(args: StatusArgs) -> Result<()> {
    let context = resolve(args.connection)?;
    let migrator = migrator(&context).await?;
    let mut applied = if context.path.exists() {
        let pool = pool(&context, false).await?;
        let migrations = applied_migrations(&pool).await?;
        pool.close().await;
        migrations
    } else {
        Default::default()
    };
    let mut entries: Vec<_> = migrator
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .map(|migration| {
            let installed = applied.remove(&migration.version);
            let status = if installed.as_ref().is_some_and(|installed| {
                installed.checksum.as_slice() == migration.checksum.as_ref()
            }) {
                "up"
            } else if installed.is_some() {
                "edited"
            } else {
                "pending"
            };
            MigrationStatus {
                status,
                version: migration.version,
                name: migration.description.to_string(),
                direction: "up",
                checksum_state: match status {
                    "up" => "verified",
                    "edited" => "mismatch",
                    _ => "not-applied",
                },
                applied_at: installed.map(|installed| installed.installed_on),
            }
        })
        .collect();
    entries.extend(
        applied
            .into_iter()
            .map(|(version, installed)| MigrationStatus {
                status: "missing",
                version,
                name: installed.description,
                direction: "up",
                checksum_state: "source-missing",
                applied_at: Some(installed.installed_on),
            }),
    );
    entries.sort_by_key(|entry| entry.version);
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"databaseKind":"sqlite","location":display_path(&context),"migrations":entries})
            )?
        );
    } else {
        println!(
            "Database  {}\n\nSTATUS   VERSION  NAME",
            display_path(&context)
        );
        for entry in &entries {
            println!(
                "{:<8} {:04}     {}",
                entry.status, entry.version, entry.name
            );
        }
        let up = entries.iter().filter(|entry| entry.status == "up").count();
        let pending = entries
            .iter()
            .filter(|entry| entry.status == "pending")
            .count();
        let invalid = entries.len() - up - pending;
        println!("\n{up} applied · {pending} pending · {invalid} invalid");
    }
    Ok(())
}

fn display_path(context: &DatabaseContext) -> String {
    context
        .path
        .strip_prefix(&context.root)
        .unwrap_or(&context.path)
        .display()
        .to_string()
}
