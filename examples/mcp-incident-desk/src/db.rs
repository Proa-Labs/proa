// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{str::FromStr, time::Duration};

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
    let filename = options.get_filename().to_owned();
    if filename.to_string_lossy() != ":memory:" {
        if let Some(parent) = filename.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    if filename.exists() {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&filename, std::fs::Permissions::from_mode(0o600))?;
    }
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}
