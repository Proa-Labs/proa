// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Optional background exports of public documentation as a typed JSON bundle.
//! This workflow was added for Ora's async-job check. Ordinary documentation
//! reads remain synchronous; the worker does real serialization without an
//! artificial delay. Jobs are ephemeral and local to the serving instance.

use std::{
    collections::HashMap,
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    body::Bytes,
    extract::{
        rejection::{JsonRejection, PathRejection},
        Path, State,
    },
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use proa_docs::DocPage;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use utoipa::ToSchema;

use super::{ApiError, ApiState, Document, Mutex, Source};
use crate::data::routes;

const RETENTION: Duration = Duration::from_secs(15 * 60);
const MAX_JOBS: usize = 8;
const MAX_RESULT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportInput {
    /// Optional selection of public document paths. Omit to export all pages.
    #[schema(min_items = 1, max_items = 50)]
    paths: Option<Vec<String>>,
}

#[derive(Serialize, ToSchema)]
pub struct ExportBundle {
    documents: Vec<Document>,
    total: usize,
}

#[derive(Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Serialize, ToSchema)]
pub struct JobStatus {
    job_id: String,
    status: ExportStatus,
    /// Same-origin path; resolve against the origin that accepted the request.
    status_url: String,
    /// Same-origin result path, available when status is completed.
    #[schema(required)]
    result_url: Option<String>,
    /// Seconds until this instance discards the job.
    expires_in: u64,
    /// Failure code when status is failed; submit a smaller selection on export_too_large.
    #[schema(required)]
    error: Option<&'static str>,
}

enum StateValue {
    Queued,
    Running,
    Completed(Bytes),
    Failed(&'static str),
}

struct Entry {
    expires: Instant,
    state: StateValue,
}

pub struct Jobs {
    entries: Mutex<HashMap<String, Entry>>,
    workers: Arc<Semaphore>,
}

impl Default for Jobs {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            workers: Arc::new(Semaphore::new(2)),
        }
    }
}

fn unavailable() -> ApiError {
    ApiError { status: StatusCode::SERVICE_UNAVAILABLE, code: "export_capacity", detail: "The bounded export service is full or unavailable. Retry later; synchronous document reads remain available." }
}

fn missing() -> ApiError {
    ApiError { status: StatusCode::NOT_FOUND, code: "export_not_found", detail: "This export is unknown, expired, or belongs to another serving instance. Submit the same read-only export request again." }
}

fn selection(source: Source, input: ExportInput) -> Result<Vec<&'static DocPage>, ApiError> {
    let mut pages = match input.paths {
        None => source
            .pages()
            .iter()
            .filter(|page| !page.frontmatter.hidden)
            .collect(),
        Some(paths) => {
            if !(1..=50).contains(&paths.len()) {
                return Err(ApiError::invalid(
                    "paths must contain between 1 and 50 public document paths, or be omitted.",
                ));
            }
            paths
                .iter()
                .map(|path| super::page_from(source, path))
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    pages.sort_unstable_by_key(|page| page.route_path);
    pages.dedup_by_key(|page| page.route_path);
    Ok(pages)
}

fn identifier(pages: &[&DocPage]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"proa-docs-json-export-v1");
    for page in pages {
        for value in [
            page.route_path,
            page.markdown_path,
            page.title,
            page.description.unwrap_or(""),
            page.markdown,
        ] {
            hash.update((value.len() as u64).to_be_bytes());
            hash.update(value.as_bytes());
        }
    }
    let mut id = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in hash.finalize() {
        id.push(HEX[(byte >> 4) as usize] as char);
        id.push(HEX[(byte & 0xf) as usize] as char);
    }
    id
}

fn view(id: &str, source: Source, entry: &Entry, now: Instant) -> JobStatus {
    // Job state belongs to this serving instance. Relative paths keep local
    // clients on their own server even when SITE_URL defaults to production.
    let status_url = format!("{}{}/{id}", source.prefix(), routes::API_EXPORTS);
    let (status, result_url, error) = match &entry.state {
        StateValue::Queued => (ExportStatus::Queued, None, None),
        StateValue::Running => (ExportStatus::Running, None, None),
        StateValue::Completed(_) => (
            ExportStatus::Completed,
            Some(format!("{status_url}/result")),
            None,
        ),
        StateValue::Failed(code) => (ExportStatus::Failed, None, Some(*code)),
    };
    JobStatus {
        job_id: id.into(),
        status,
        status_url,
        result_url,
        expires_in: entry.expires.saturating_duration_since(now).as_secs(),
        error,
    }
}

impl Jobs {
    fn submit(
        self: &Arc<Self>,
        source: Source,
        pages: Vec<&'static DocPage>,
        now: Instant,
    ) -> Result<JobStatus, ApiError> {
        let id = identifier(&pages);
        let mut entries = self.entries.lock().map_err(|_| unavailable())?;
        entries.retain(|_, entry| entry.expires > now);
        if let Some(entry) = entries.get(&id) {
            return Ok(view(&id, source, entry, now));
        }
        if entries.len() >= MAX_JOBS {
            return Err(unavailable());
        }
        let entry = Entry {
            expires: now + RETENTION,
            state: StateValue::Queued,
        };
        let response = view(&id, source, &entry, now);
        entries.insert(id.clone(), entry);
        drop(entries);

        let jobs = Arc::clone(self);
        tokio::spawn(async move {
            let Ok(_permit) = Arc::clone(&jobs.workers).acquire_owned().await else {
                jobs.update(&id, StateValue::Failed("export_unavailable"));
                return;
            };
            jobs.update(&id, StateValue::Running);
            let result = tokio::task::spawn_blocking(move || bundle(pages)).await;
            let state = match result {
                Ok(Ok(body)) => StateValue::Completed(body),
                Ok(Err(code)) => StateValue::Failed(code),
                Err(_) => {
                    tracing::error!("documentation export worker failed");
                    StateValue::Failed("export_failed")
                }
            };
            jobs.update(&id, state);
        });
        Ok(response)
    }

    fn update(&self, id: &str, state: StateValue) {
        if let Ok(mut entries) = self.entries.lock() {
            if let Some(entry) = entries.get_mut(id) {
                entry.state = state;
            }
        }
    }

    fn read(
        &self,
        source: Source,
        id: &str,
        now: Instant,
    ) -> Result<(JobStatus, Option<Bytes>), ApiError> {
        if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ApiError::invalid("Invalid export job identifier."));
        }
        let mut entries = self.entries.lock().map_err(|_| unavailable())?;
        entries.retain(|_, entry| entry.expires > now);
        let entry = entries.get(id).ok_or_else(missing)?;
        let body = if let StateValue::Completed(body) = &entry.state {
            Some(body.clone())
        } else {
            None
        };
        Ok((view(id, source, entry, now), body))
    }
}

struct BoundedBuffer(Vec<u8>);
impl io::Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_RESULT_BYTES - self.0.len() {
            return Err(io::Error::other("export exceeds size limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn bundle(pages: Vec<&'static DocPage>) -> Result<Bytes, &'static str> {
    let total = pages.len();
    let documents = pages
        .into_iter()
        .map(|page| Document {
            summary: page.into(),
            markdown: page.markdown,
        })
        .collect();
    let mut output = BoundedBuffer(Vec::new());
    serde_json::to_writer(&mut output, &ExportBundle { documents, total })
        .map_err(|_| "export_too_large")?;
    Ok(Bytes::from(output.0))
}

fn accepted(job: JobStatus) -> Response {
    let location = HeaderValue::from_str(&job.status_url).expect("valid export status path");
    let mut response = (StatusCode::ACCEPTED, Json(job)).into_response();
    response.headers_mut().insert(header::LOCATION, location);
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    response
}

fn job_response(job: JobStatus) -> Response {
    match job.status {
        ExportStatus::Queued | ExportStatus::Running => accepted(job),
        ExportStatus::Completed | ExportStatus::Failed => Json(job).into_response(),
    }
}

fn path_id(id: Result<Path<String>, PathRejection>) -> Result<String, ApiError> {
    id.map(|Path(id)| id)
        .map_err(|_| ApiError::invalid("Invalid export job identifier."))
}

/// Start a background JSON export of public documents.
///
/// Returns 202 Accepted with job_id and a Location/status_url to poll. The
/// worker serializes real document data; small exports may finish immediately.
/// Matching selections are deduplicated while retained; a terminal reused job
/// returns 200 with its current status. Each instance retains
/// at most eight jobs for fifteen minutes, with two workers and a 4 MiB result
/// limit per job. State is not shared across replicas or restarts; on an expired
/// or unknown job, safely resubmit the same selection. No private data is read.
#[utoipa::path(post, path = routes::API_EXPORTS, operation_id = "exportDocs",
    request_body = ExportInput,
    responses(ApiError,
        (status = 200, description = "An identical retained export has already completed or failed.", body = JobStatus),
        (status = 202, description = "Export accepted. Poll status_url or Location; retrieve result_url when completed.", body = JobStatus,
            headers(("Location" = String, description = "Same-origin job status path."), ("Retry-After" = u32, description = "Suggested seconds before polling."))),
        (status = 413, description = "Request exceeds 16 KiB.", body = super::Problem, content_type = "application/problem+json"),
        (status = 415, description = "Expected application/json.", body = super::Problem, content_type = "application/problem+json"),
        (status = 503, description = "Export capacity exhausted.", body = super::Problem, content_type = "application/problem+json")
    ))]
pub async fn create(
    State(state): State<Arc<ApiState>>,
    input: Result<Json<ExportInput>, JsonRejection>,
) -> Result<Response, ApiError> {
    let pages = selection(state.source, super::json_input(input)?)?;
    state
        .exports
        .submit(state.source, pages, Instant::now())
        .map(job_response)
}

/// Poll the state of an accepted documentation export.
///
/// Queued/running jobs return 202 with Retry-After. Completed/failed jobs return
/// 200 with their terminal state. Follow result_url on completed jobs. An unknown
/// or expired job returns 404; resubmit the same safe export request.
#[utoipa::path(get, path = routes::API_EXPORT_JOB_SPEC, operation_id = "getDocsExport",
    params(("id" = String, Path, description = "job_id returned by exportDocs", min_length = 64, max_length = 64)),
    responses(ApiError,
        (status = 200, description = "Completed or failed job state.", body = JobStatus),
        (status = 202, description = "Work remains queued or running.", body = JobStatus,
            headers(("Location" = String, description = "Same-origin job status path."), ("Retry-After" = u32, description = "Seconds before polling.")))
    ))]
pub async fn status(
    State(state): State<Arc<ApiState>>,
    id: Result<Path<String>, PathRejection>,
) -> Result<Response, ApiError> {
    let id = path_id(id)?;
    let (job, _) = state.exports.read(state.source, &id, Instant::now())?;
    Ok(job_response(job))
}

/// Download a completed JSON documentation bundle.
#[utoipa::path(get, path = routes::API_EXPORT_RESULT_SPEC, operation_id = "getDocsExportResult",
    params(("id" = String, Path, description = "job_id returned by exportDocs", min_length = 64, max_length = 64)),
    responses(ApiError,
        (status = 200, description = "Exported document metadata and Markdown.", body = ExportBundle),
        (status = 202, description = "Export is still pending. Poll Location.", body = JobStatus,
            headers(("Location" = String, description = "Same-origin job status path."), ("Retry-After" = u32, description = "Seconds before polling."))),
        (status = 409, description = "Export failed; read its status for the failure code.", body = super::Problem, content_type = "application/problem+json")
    ))]
pub async fn result(
    State(state): State<Arc<ApiState>>,
    id: Result<Path<String>, PathRejection>,
) -> Result<Response, ApiError> {
    let id = path_id(id)?;
    let (job, body) = state.exports.read(state.source, &id, Instant::now())?;
    if let Some(body) = body {
        return Ok((
            [
                (header::CONTENT_TYPE, "application/json"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=proa-docs.json",
                ),
            ],
            body,
        )
            .into_response());
    }
    if matches!(job.status, ExportStatus::Failed) {
        return Err(ApiError { status: StatusCode::CONFLICT, code: "export_failed", detail: "The export failed. Inspect the job status; use a smaller selection for export_too_large." });
    }
    Ok(accepted(job))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_and_failed_results_preserve_the_polling_contract() {
        let jobs = Arc::new(Jobs::default());
        // Hold both worker permits to observe a real queued job deterministically.
        let _permits = jobs.workers.acquire_many(2).await.unwrap();
        let job = jobs
            .submit(
                Source::Sandbox,
                vec![&Source::Sandbox.pages()[0]],
                Instant::now(),
            )
            .unwrap();
        let state = Arc::new(ApiState {
            source: Source::Sandbox,
            replay: super::super::replay::SearchReplayCache::default(),
            exports: Arc::clone(&jobs),
        });
        let response = result(State(Arc::clone(&state)), Ok(Path(job.job_id.clone())))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(response.headers()[header::LOCATION], job.status_url);
        assert_eq!(response.headers()[header::RETRY_AFTER], "1");
        jobs.update(&job.job_id, StateValue::Failed("export_too_large"));
        let response = status(State(Arc::clone(&state)), Ok(Path(job.job_id.clone())))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["status"], "failed");
        assert_eq!(body["error"], "export_too_large");
        assert!(body["result_url"].is_null());
        let error = result(State(state), Ok(Path(job.job_id)))
            .await
            .err()
            .unwrap();
        assert_eq!(error.status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn jobs_deduplicate_bound_capacity_and_expire() {
        let jobs = Arc::new(Jobs::default());
        let now = Instant::now();
        let pages: Vec<_> = Source::Production
            .pages()
            .iter()
            .filter(|p| !p.frontmatter.hidden)
            .take(MAX_JOBS + 1)
            .collect();
        let first = jobs
            .submit(Source::Production, vec![pages[0]], now)
            .unwrap();
        let retry = jobs
            .submit(Source::Production, vec![pages[0]], now)
            .unwrap();
        assert_eq!(first.job_id, retry.job_id);
        for page in &pages[1..MAX_JOBS] {
            jobs.submit(Source::Production, vec![page], now).unwrap();
        }
        assert_eq!(
            jobs.submit(Source::Production, vec![pages[MAX_JOBS]], now)
                .err()
                .unwrap()
                .status,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert!(jobs.read(Source::Production, &first.job_id, now).is_ok());
        assert_eq!(
            jobs.read(Source::Production, &first.job_id, now + RETENTION)
                .err()
                .unwrap()
                .status,
            StatusCode::NOT_FOUND
        );
        assert!(jobs
            .submit(Source::Production, vec![pages[MAX_JOBS]], now + RETENTION)
            .is_ok());
    }

    #[test]
    fn writer_enforces_result_budget_before_growing() {
        use io::Write;
        let mut output = BoundedBuffer(vec![0; MAX_RESULT_BYTES - 1]);
        assert!(output.write_all(&[1]).is_ok());
        assert!(output.write_all(&[2]).is_err());
        assert_eq!(output.0.len(), MAX_RESULT_BYTES);
    }

    #[test]
    fn selection_is_public_and_order_independent() {
        let source = Source::Sandbox;
        let first = source.pages()[0].route_path.to_owned();
        let second = source.pages()[1].route_path.to_owned();
        let a = selection(
            source,
            ExportInput {
                paths: Some(vec![first.clone(), second.clone(), first.clone()]),
            },
        )
        .unwrap();
        let b = selection(
            source,
            ExportInput {
                paths: Some(vec![second, first]),
            },
        )
        .unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(identifier(&a), identifier(&b));
        assert!(selection(
            source,
            ExportInput {
                paths: Some(vec!["/etc/passwd".into()])
            }
        )
        .is_err());
        assert!(selection(
            source,
            ExportInput {
                paths: Some(Vec::new())
            }
        )
        .is_err());
    }
}
