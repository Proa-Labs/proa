// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    app::AppState,
    security::{digest, now, password_hash, random},
};
pub async fn seed(s: &AppState) -> anyhow::Result<()> {
    let mut tx = s.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("INSERT OR IGNORE INTO settings VALUES('signing_kid',?)")
        .bind(&s.keys.kid)
        .execute(&mut *tx)
        .await?;
    let redirects = serde_json::to_string(&vec![s.config.callback()])?;
    sqlx::query(
        "INSERT OR IGNORE INTO clients VALUES('incident-web','Incident Desk website',?,?,?)",
    )
    .bind(redirects)
    .bind(digest(&s.keys.client_secret))
    .bind(now())
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT OR IGNORE INTO clients VALUES('incident-demo-client','Incident Desk reference client','[\"http://127.0.0.1:5572/callback\"]',NULL,?)").bind(now()).execute(&mut *tx).await?;
    if sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key='seed'")
        .fetch_optional(&mut *tx)
        .await?
        .is_some()
    {
        tx.commit().await?;
        return Ok(());
    }
    let mut credentials = Vec::new();
    for (id, name) in [("alice", "Alice"), ("ben", "Ben"), ("casey", "Casey")] {
        let password = random();
        let hash = password_hash(&password)?;
        sqlx::query("INSERT INTO users VALUES(?,?,?)")
            .bind(id)
            .bind(name)
            .bind(hash)
            .execute(&mut *tx)
            .await?;
        credentials.push((id, password));
    }
    for (id, name, owner) in [
        ("aster", "Aster Commerce", "alice"),
        ("harbor", "Harbor Analytics", "casey"),
    ] {
        sqlx::query("INSERT INTO workspaces VALUES(?,?)")
            .bind(id)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO memberships VALUES(?,?,'maintainer')")
            .bind(owner)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO projects VALUES(?,'checkout','Checkout')")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO releases VALUES(?,'checkout-2026.09.08.3','Connection handling changes deployed before the error increase. Correlation requires investigation.','2026-09-08 14:32 UTC')").bind(id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO runbooks VALUES(?,'pool','Connection pool recovery',?)")
            .bind(id)
            .bind(serde_json::to_string(&[
                "Check connection-pool utilization.",
                "Compare latency before and after the deployment.",
                "Verify recovery before resolving.",
            ])?)
            .execute(&mut *tx)
            .await?;
        for i in 0..6 {
            let issue_id = if id == "aster" {
                format!("AST-{}", 1042 + i)
            } else {
                format!("HBR-{}", 201 + i)
            };
            let title = if i == 0 {
                "Checkout timeout"
            } else {
                [
                    "Payment retry spike",
                    "Session refresh failed",
                    "Inventory lookup slow",
                    "Cart update failed",
                    "Invoice export timeout",
                ][(i - 1) as usize]
            };
            sqlx::query("INSERT INTO issues VALUES(?,?,'checkout',?,'open',?,148,2431,'checkout-2026.09.08.3','pool',?,'2026-09-08 14:45 UTC',7)").bind(id).bind(&issue_id).bind(title).bind(if i==0{"high"}else{"medium"}).bind(owner).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO event_samples VALUES(?,?,1,?)").bind(id).bind(&issue_id).bind(if id=="harbor"{"HARBOR-PRIVATE-ISOLATION-MARKER"}else{"TimeoutError: checkout database connection wait exceeded 3000ms. at checkout::submit_order (checkout.rs:142)"}).execute(&mut *tx).await?;
        }
    }
    sqlx::query("INSERT INTO memberships VALUES('ben','aster','viewer')")
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO settings VALUES('seed','1')")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    // Explicit one-time development credential display; never sent to application logs or HTTP.
    eprintln!("Incident Desk demo accounts (shown once; retain locally):");
    for (id, password) in credentials {
        eprintln!("  {id}: {password}");
    }
    Ok(())
}
