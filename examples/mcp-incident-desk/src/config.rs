// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub app_origin: String,
    pub issuer: String,
    pub private_dir: PathBuf,
    pub app_port: u16,
    pub issuer_port: u16,
}
impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let app_port = std::env::var("PORT")
            .unwrap_or_else(|_| "5570".into())
            .parse()?;
        let issuer_port = std::env::var("ISSUER_PORT")
            .unwrap_or_else(|_| "5571".into())
            .parse()?;
        let config = Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/mcp-incident-desk.db".into()),
            app_origin: std::env::var("APP_ORIGIN")
                .unwrap_or_else(|_| format!("https://localhost:{app_port}")),
            issuer: std::env::var("ISSUER_ORIGIN")
                .unwrap_or_else(|_| format!("https://127.0.0.1:{issuer_port}")),
            private_dir: std::env::var("PRIVATE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(".local")),
            app_port,
            issuer_port,
        };
        for origin in [&config.app_origin, &config.issuer] {
            let u = url::Url::parse(origin)?;
            anyhow::ensure!(
                u.scheme() == "https"
                    && u.path() == "/"
                    && u.query().is_none()
                    && u.fragment().is_none()
                    && u.username().is_empty()
                    && u.password().is_none(),
                "origins must be HTTPS origins without paths or credentials"
            );
            anyhow::ensure!(!origin.ends_with('/'), "origins must omit trailing slash");
        }
        anyhow::ensure!(
            url::Url::parse(&config.app_origin)?.host_str()
                != url::Url::parse(&config.issuer)?.host_str(),
            "app and issuer require distinct cookie hosts"
        );
        Ok(config)
    }
    pub fn resource(&self) -> String {
        format!("{}/mcp", self.app_origin)
    }
    pub fn callback(&self) -> String {
        format!("{}/auth/callback", self.app_origin)
    }
}
