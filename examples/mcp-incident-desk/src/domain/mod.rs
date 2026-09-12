// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    error::{AppError, Result},
    security::{digest, now, random},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqliteConnection};
mod create;
pub use create::{create, CreateIssueInput, CreateIssueResult, Severity};
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Principal {
    pub user_id: String,
    pub name: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub role: String,
    pub scopes: String,
    pub client_id: String,
    pub credential_id: String,
    pub kind: String,
}
impl Principal {
    pub fn can_write(&self) -> bool {
        self.role == "maintainer" && self.scopes.split_whitespace().any(|s| s == "issues:write")
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, FromRow, JsonSchema)]
pub struct Issue {
    pub workspace_id: String,
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub status: String,
    pub severity: String,
    pub affected_users: i64,
    pub event_count: i64,
    pub release_id: String,
    pub runbook_id: String,
    pub owner_id: String,
    pub last_seen: String,
    pub version: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, FromRow, JsonSchema)]
pub struct Activity {
    pub id: String,
    pub user_name: String,
    pub client_id: String,
    pub note: String,
    pub created_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct IssueProjection {
    pub issue: Issue,
    pub project: String,
    pub owner: String,
    pub release_summary: String,
    pub deployed_at: String,
    pub runbook_title: String,
    pub runbook_steps: Vec<String>,
    pub events: Vec<String>,
    pub activity: Vec<Activity>,
    pub can_resolve: bool,
    pub url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResolveInput {
    pub issue_id: String,
    pub expected_version: i64,
    pub note: String,
    pub idempotency_key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResolveResult {
    pub issue_id: String,
    pub status: String,
    pub version: i64,
    pub audit_id: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SearchInput {
    pub query: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchResult {
    pub issues: Vec<Issue>,
    pub next_cursor: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Cursor {
    workspace: String,
    query: String,
    status: String,
    revision: i64,
    after: String,
}
pub async fn authorize(conn: &mut SqliteConnection, p: &Principal, write: bool) -> Result<()> {
    let role: Option<String> =
        sqlx::query_scalar("SELECT role FROM memberships WHERE user_id=? AND workspace_id=?")
            .bind(&p.user_id)
            .bind(&p.workspace_id)
            .fetch_optional(&mut *conn)
            .await?;
    if role.is_none() {
        return Err(AppError::Forbidden);
    }
    let active: bool = if p.kind == "mcp" {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM grants WHERE id=? AND user_id=? AND workspace_id=? AND revoked=0 AND expires>?)").bind(&p.credential_id).bind(&p.user_id).bind(&p.workspace_id).bind(now()).fetch_one(&mut *conn).await?
    } else {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE id_hash=? AND user_id=? AND workspace_id=? AND kind='app' AND expires>? AND last_seen>?)").bind(&p.credential_id).bind(&p.user_id).bind(&p.workspace_id).bind(now()).bind(now()-1800).fetch_one(&mut *conn).await?
    };
    if !active {
        return Err(AppError::Unauthorized);
    }
    if !p.scopes.split_whitespace().any(|s| s == "issues:read") {
        return Err(AppError::Forbidden);
    }
    if write && (role.as_deref() != Some("maintainer") || !p.can_write()) {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
pub async fn search(
    db: &sqlx::SqlitePool,
    p: &Principal,
    input: SearchInput,
) -> Result<SearchResult> {
    let mut tx = db.begin().await?;
    authorize(&mut tx, p, false).await?;
    let query = input.query.unwrap_or_default();
    let status = input.status.unwrap_or_default();
    if query.len() > 200 || !matches!(status.as_str(), "" | "open" | "resolved") {
        return Err(AppError::Invalid("invalid search"));
    }
    let revision: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(version),0) FROM issues WHERE workspace_id=?")
            .bind(&p.workspace_id)
            .fetch_one(&mut *tx)
            .await?;
    let after = if let Some(cursor) = input.cursor {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| AppError::Invalid("invalid cursor"))?;
        let c: Cursor =
            serde_json::from_slice(&bytes).map_err(|_| AppError::Invalid("invalid cursor"))?;
        if c.workspace != p.workspace_id
            || c.query != query
            || c.status != status
            || c.revision != revision
        {
            return Err(AppError::Conflict("search changed; restart pagination"));
        }
        c.after
    } else {
        String::new()
    };
    let limit = input.limit.unwrap_or(20).clamp(1, 50) as usize;
    let mut issues:Vec<Issue>=sqlx::query_as("SELECT * FROM issues WHERE workspace_id=? AND id>? AND (?='' OR status=?) AND (instr(lower(title),lower(?))>0 OR instr(lower(id),lower(?))>0) ORDER BY id LIMIT ?").bind(&p.workspace_id).bind(after).bind(&status).bind(&status).bind(&query).bind(&query).bind((limit+1) as i64).fetch_all(&mut *tx).await?;
    let next_cursor = if issues.len() > limit {
        issues.truncate(limit);
        use base64::Engine;
        Some(
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&Cursor {
                    workspace: p.workspace_id.clone(),
                    query,
                    status,
                    revision,
                    after: issues.last().expect("nonempty page").id.clone(),
                })
                .map_err(anyhow::Error::from)?,
            ),
        )
    } else {
        None
    };
    tx.commit().await?;
    Ok(SearchResult {
        issues,
        next_cursor,
    })
}
pub async fn resolve(
    db: &sqlx::SqlitePool,
    p: &Principal,
    input: ResolveInput,
) -> Result<ResolveResult> {
    if input.note.trim().is_empty()
        || input.note.len() > 2000
        || input.idempotency_key.len() < 8
        || input.idempotency_key.len() > 128
    {
        return Err(AppError::Invalid(
            "a note and 8–128 character idempotency key are required",
        ));
    }
    let hash = digest(&serde_json::to_string(&input).map_err(anyhow::Error::from)?);
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    authorize(&mut tx, p, true).await?;
    let prior:Option<(String,String)>=sqlx::query_as("SELECT input_hash,result FROM idempotency WHERE user_id=? AND client_id=? AND workspace_id=? AND operation='resolve' AND key=?").bind(&p.user_id).bind(&p.client_id).bind(&p.workspace_id).bind(&input.idempotency_key).fetch_optional(&mut *tx).await?;
    if let Some((old, result)) = prior {
        if old != hash {
            return Err(AppError::Conflict(
                "idempotency key already used with different input",
            ));
        }
        return Ok(serde_json::from_str(&result).map_err(anyhow::Error::from)?);
    }
    let issue: Issue = sqlx::query_as("SELECT * FROM issues WHERE workspace_id=? AND id=?")
        .bind(&p.workspace_id)
        .bind(&input.issue_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    if issue.version != input.expected_version {
        return Err(AppError::Conflict(
            "issue changed; retrieve its current version",
        ));
    }
    if issue.status == "resolved" {
        return Err(AppError::Conflict("issue already resolved"));
    }
    let audit_id = random();
    let result = ResolveResult {
        issue_id: input.issue_id.clone(),
        status: "resolved".into(),
        version: issue.version + 1,
        audit_id: audit_id.clone(),
    };
    sqlx::query("UPDATE issues SET status='resolved',version=version+1 WHERE workspace_id=? AND id=? AND version=?").bind(&p.workspace_id).bind(&input.issue_id).bind(input.expected_version).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO activity VALUES(?,?,?,?,?,?,?)")
        .bind(audit_id)
        .bind(&p.workspace_id)
        .bind(&input.issue_id)
        .bind(&p.user_id)
        .bind(&p.client_id)
        .bind(&input.note)
        .bind(now())
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO idempotency VALUES(?,?,?,'resolve',?,?,?)")
        .bind(&p.user_id)
        .bind(&p.client_id)
        .bind(&p.workspace_id)
        .bind(&input.idempotency_key)
        .bind(hash)
        .bind(serde_json::to_string(&result).map_err(anyhow::Error::from)?)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(result)
}
