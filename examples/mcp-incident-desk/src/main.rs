// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use mcp_incident_desk::{app::AppState, config::Config, routes};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::from_env()?;
    let cert = config.private_dir.join("tls.pem");
    let key = config.private_dir.join("tls.key");
    anyhow::ensure!(cert.exists()&&key.exists(),"Run ./scripts/setup.sh to create local TLS certificates, then trust .local/ca.pem in your browser.");
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
    let state = AppState::open(config.clone()).await?;
    if std::env::args().nth(1).as_deref() == Some("reset-password") {
        let id = std::env::args()
            .nth(2)
            .ok_or_else(|| anyhow::anyhow!("usage: reset-password alice|ben|casey"))?;
        let password = mcp_incident_desk::security::random();
        let hash = mcp_incident_desk::security::password_hash(&password)?;
        let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
        anyhow::ensure!(
            sqlx::query("UPDATE users SET password_hash=? WHERE id=?")
                .bind(hash)
                .bind(&id)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                == 1,
            "unknown account"
        );
        sqlx::query("DELETE FROM sessions WHERE user_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE grants SET revoked=1 WHERE user_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        println!("New local demo password for {id}: {password}");
        return Ok(());
    }
    let cleanup = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            if let Err(e) = cleanup_expired(&cleanup).await {
                tracing::warn!(error=%e,"credential cleanup failed");
            }
        }
    });
    tracing::info!(app=%config.app_origin,issuer=%config.issuer,"Incident Desk listening");
    let app = axum_server::bind_rustls(
        std::net::SocketAddr::from(([127, 0, 0, 1], config.app_port)),
        tls.clone(),
    )
    .serve(routes::router(state.clone()).into_make_service());
    let issuer = axum_server::bind_rustls(
        std::net::SocketAddr::from(([127, 0, 0, 1], config.issuer_port)),
        tls,
    )
    .serve(routes::issuer(state).into_make_service());
    tokio::try_join!(app, issuer)?;
    Ok(())
}
async fn cleanup_expired(s: &AppState) -> anyhow::Result<()> {
    let now = mcp_incident_desk::security::now();
    let mut tx = s.db.begin().await?;
    for table in ["pending", "sessions", "codes"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} WHERE expires<? LIMIT 500)")).bind(now).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM refresh_tokens WHERE hash IN (SELECT r.hash FROM refresh_tokens r JOIN grants g ON g.id=r.grant_id WHERE g.expires<? LIMIT 500)").bind(now).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
