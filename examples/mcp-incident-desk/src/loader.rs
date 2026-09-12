// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    domain::{self, Activity, Issue, IssueProjection, Principal},
    error::{AppError, Result},
};
use proa_core::{DataLoader, LoadError, LoadKey, LoadRequest, LoadValue};
use proa_stream::DataResolver;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Mutex;
pub struct ScopedLoader {
    pub principal: Principal,
    pub reads: AtomicUsize,
    transaction: Mutex<Option<sqlx::Transaction<'static, sqlx::Sqlite>>>,
}
impl DataLoader for ScopedLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl std::future::Future<Output = std::result::Result<LoadValue, LoadError>> + Send {
        let key = request.key().to_string();
        async move {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let result: Result<LoadValue> = async {
                let mut guard = self.transaction.lock().await;
                let tx = guard
                    .as_mut()
                    .ok_or(AppError::Invalid("read snapshot closed"))?;
                let prefix = format!("{}:", self.principal.workspace_id);
                let key = key.strip_prefix(&prefix).ok_or(AppError::Forbidden)?;
                if let Some(id) = key.strip_prefix("issue/") {
                    let issue: Issue =
                        sqlx::query_as("SELECT * FROM issues WHERE workspace_id=? AND id=?")
                            .bind(&self.principal.workspace_id)
                            .bind(id)
                            .fetch_optional(&mut **tx)
                            .await?
                            .ok_or(AppError::NotFound)?;
                    Ok(LoadValue::typed(issue))
                } else if let Some(id) = key.strip_prefix("project/") {
                    let name: String = sqlx::query_scalar(
                        "SELECT name FROM projects WHERE workspace_id=? AND id=?",
                    )
                    .bind(&self.principal.workspace_id)
                    .bind(id)
                    .fetch_optional(&mut **tx)
                    .await?
                    .ok_or(AppError::NotFound)?;
                    Ok(LoadValue::typed(name))
                } else {
                    Err(AppError::NotFound)
                }
            }
            .await;
            result.map_err(|e| LoadError::custom(e.to_string()))
        }
    }
}
pub struct RequestScope {
    pub principal: Principal,
    pub loaders: Arc<DataResolver<ScopedLoader>>,
    pub inner: Arc<ScopedLoader>,
    pub request_id: String,
    pub locale: String,
}
impl RequestScope {
    pub async fn new(db: &sqlx::SqlitePool, p: Principal) -> Result<Self> {
        let mut tx = db.begin().await?;
        domain::authorize(&mut tx, &p, false).await?;
        let inner = Arc::new(ScopedLoader {
            principal: p.clone(),
            reads: AtomicUsize::new(0),
            transaction: Mutex::new(Some(tx)),
        });
        Ok(Self {
            principal: p,
            loaders: DataResolver::request_scope(inner.clone()),
            inner,
            request_id: crate::security::random(),
            locale: "en".into(),
        })
    }
    pub async fn report(&self, id: &str, origin: &str) -> Result<IssueProjection> {
        let key = LoadKey::named(&self.principal.workspace_id, format!("issue/{id}"));
        let issue = self
            .loaders
            .load_key(&key)
            .await
            .map_err(|_| AppError::NotFound)?
            .downcast::<Issue>()
            .map_err(|_| AppError::NotFound)?;
        let project_key = LoadKey::named(
            &self.principal.workspace_id,
            format!("project/{}", issue.project_id),
        );
        // Independent sections request the same typed project; the resolver performs one SQL read.
        let (a, b) = tokio::join!(
            self.loaders.load_key(&project_key),
            self.loaders.load_key(&project_key)
        );
        let project = a
            .map_err(|_| AppError::NotFound)?
            .downcast::<String>()
            .map_err(|_| AppError::NotFound)?;
        b.map_err(|_| AppError::NotFound)?;
        let mut guard = self.inner.transaction.lock().await;
        let tx = guard.as_mut().ok_or(AppError::Invalid("snapshot closed"))?;
        let (release_summary, deployed_at): (String, String) = sqlx::query_as(
            "SELECT summary,deployed_at FROM releases WHERE workspace_id=? AND id=?",
        )
        .bind(&issue.workspace_id)
        .bind(&issue.release_id)
        .fetch_one(&mut **tx)
        .await?;
        let (runbook_title, steps): (String, String) =
            sqlx::query_as("SELECT title,steps_json FROM runbooks WHERE workspace_id=? AND id=?")
                .bind(&issue.workspace_id)
                .bind(&issue.runbook_id)
                .fetch_one(&mut **tx)
                .await?;
        let owner: String = sqlx::query_scalar("SELECT name FROM users WHERE id=?")
            .bind(&issue.owner_id)
            .fetch_one(&mut **tx)
            .await?;
        let events=sqlx::query_scalar("SELECT detail FROM event_samples WHERE workspace_id=? AND issue_id=? ORDER BY id LIMIT 10").bind(&issue.workspace_id).bind(id).fetch_all(&mut **tx).await?;
        let activity:Vec<Activity>=sqlx::query_as("SELECT a.id,u.name user_name,a.client_id,a.note,a.created_at FROM activity a JOIN users u ON u.id=a.user_id WHERE a.workspace_id=? AND a.issue_id=? ORDER BY a.created_at DESC LIMIT 20").bind(&issue.workspace_id).bind(id).fetch_all(&mut **tx).await?;
        let result = IssueProjection {
            issue: (*issue).clone(),
            project: (*project).clone(),
            owner,
            release_summary,
            deployed_at,
            runbook_title,
            runbook_steps: serde_json::from_str(&steps).map_err(anyhow::Error::from)?,
            events,
            activity,
            can_resolve: self.principal.can_write() && issue.status == "open",
            url: format!("{origin}/issues/{id}"),
        };
        if let Some(tx) = guard.take() {
            tx.commit().await?;
        }
        Ok(result)
    }
}

pub struct DatabaseStatus {
    pub driver: &'static str,
    pub version: String,
}
pub fn database_status_key() -> LoadKey {
    LoadKey::named("system", "database-status")
}
