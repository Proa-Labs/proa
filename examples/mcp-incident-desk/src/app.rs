// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    config::Config,
    error::{AppError, Result},
    security::Keys,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub config: Arc<Config>,
    pub keys: Arc<Keys>,
    pub http: reqwest::Client,
    pub limits: Arc<Mutex<HashMap<String, (i64, u32)>>>,
}
impl AppState {
    pub async fn open(config: Config) -> anyhow::Result<Self> {
        let db = crate::db::connect(&config.database_url).await?;
        let initialized: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key='signing_kid'")
                .fetch_optional(&db)
                .await?;
        if initialized.is_some() {
            anyhow::ensure!(
                config.private_dir.join("signing.pem").exists()
                    && config.private_dir.join("web-client.secret").exists(),
                "private setup material missing; restore it before starting"
            );
        }
        let keys = Arc::new(Keys::load_or_create(&config.private_dir)?);
        if let Some(kid) = initialized {
            anyhow::ensure!(kid == keys.kid, "signing key does not match database");
            let stored: String =
                sqlx::query_scalar("SELECT secret_hash FROM clients WHERE id='incident-web'")
                    .fetch_one(&db)
                    .await?;
            anyhow::ensure!(
                crate::security::equal(&stored, &crate::security::digest(&keys.client_secret)),
                "web client secret does not match database"
            );
        }
        let mut client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none());
        let ca = config.private_dir.join("ca.pem");
        if ca.exists() {
            client =
                client.add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(ca)?)?);
        }
        let state = Self {
            db,
            config: Arc::new(config),
            keys,
            http: client.build()?,
            limits: Default::default(),
        };
        crate::seed::seed(&state).await?;
        Ok(state)
    }
    pub fn rate_limit(&self, key: &str, max: u32) -> Result<()> {
        let now = crate::security::now();
        let mut entries = self.limits.lock().map_err(|_| AppError::Limited)?;
        entries.retain(|_, v| now - v.0 < 60);
        if entries.len() >= 1024 && !entries.contains_key(key) {
            return Err(AppError::Limited);
        }
        let entry = entries.entry(key.to_owned()).or_insert((now, 0));
        entry.1 += 1;
        if entry.1 > max {
            Err(AppError::Limited)
        } else {
            Ok(())
        }
    }
}
