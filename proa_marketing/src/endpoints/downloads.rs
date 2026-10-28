// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A read-through bridge in front of the proa-cli GitHub Releases.
//!
//! The archives themselves are published by `.github/workflows/release-cli.yml`
//! and never move. `Proa-Labs/proa` is private today, so those asset URLs 404
//! for everyone without repository access and the installers cannot fetch them.
//! This module serves the same bytes from proa.so with server-side credentials.
//!
//! The repository is going public, and GitHub Releases stays the permanent
//! home. That transition is a configuration change, not a rewrite: drop
//! `PROA_RELEASE_TOKEN` and the bridge switches from streaming bytes to a 302
//! straight at github.com, with the same routes and the same URLs.
//!
//! Both modes resolve the release through the GitHub API first, because that is
//! what makes the filename check a whitelist against real data rather than a
//! guess about what a filename may look like. Redirect mode therefore still
//! spends API calls, unauthenticated ones at 60/hour per IP, which is why the
//! metadata cache below is not optional.
//!
//! Naming below is the installer contract from the release workflow, not a
//! preference:
//!
//!   tag       proa-cli-v<version>
//!   archive   proa-cli-<version>-x86_64-unknown-linux-gnu.tar.gz
//!   checksums proa-cli-<version>-SHA256SUMS
//!
//! Stable consumers use `/downloads/cli`; consumers that intentionally track
//! the framework's default branch use `/downloads/cli/main`. Main releases are
//! commit-qualified prereleases, so changing the channel pointer never mutates
//! bytes already served from a versioned URL.

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

/// Repository that publishes the releases. Overridable so a staging deployment
/// can point at a fork without a rebuild.
const DEFAULT_REPO: &str = crate::data::site::GITHUB_REPOSITORY;

/// Every proa-cli release tag starts with this. The monorepo tags other crates
/// too, which is why `/releases/latest` is unusable here: it answers with the
/// newest release of anything.
const TAG_PREFIX: &str = "proa-cli-v";

/// A release is installable only when the supported Ubuntu archive and shared
/// checksum file exist. GitHub exposes a release while assets are still
/// uploading, so treating the tag alone as "latest" creates a race where CI
/// resolves a version that cannot be installed on its runner yet.
const REQUIRED_RELEASE_ASSET_SUFFIXES: [&str; 2] =
    ["x86_64-unknown-linux-gnu.tar.gz", "SHA256SUMS"];

const GITHUB_API: &str = "https://api.github.com";
const GITHUB_WEB: &str = crate::data::site::GITHUB_ORIGIN;

/// GitHub rejects API requests without a User-Agent.
const USER_AGENT: &str = "proa.so-download-bridge";

/// Pinned so a future default change at GitHub cannot reshape the JSON we parse.
const GITHUB_API_VERSION: &str = "2022-11-28";

const JSON_CONTENT_TYPE: &str = "application/json; charset=utf-8";
const METADATA_CACHE_CONTROL: &str = "public, max-age=300";
const MAIN_METADATA_CACHE_CONTROL: &str = "no-store";

/// The version is part of the URL, so those bytes can never change.
const ARCHIVE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// Short: a 404 turns into a 200 the moment the release publishes.
const NOT_FOUND_CACHE_CONTROL: &str = "public, max-age=60";

/// A transient GitHub outage must not be cached at all, let alone for a year.
const UNAVAILABLE_CACHE_CONTROL: &str = "no-store";

/// Static bodies, so no runtime value (least of all the token) can reach a
/// client through an error path.
const NOT_FOUND_BODY: &str =
    r#"{"error":"not_found","message":"no proa-cli release or asset with that name"}"#;
const UNAVAILABLE_BODY: &str =
    r#"{"error":"unavailable","message":"the release host is unavailable, retry shortly"}"#;

const RELEASE_TTL: Duration = Duration::from_secs(300);

/// Main is a moving channel. Refresh the release list frequently enough that a
/// completed merge becomes installable promptly without turning every download
/// into a GitHub API call.
const RELEASE_LIST_TTL: Duration = Duration::from_secs(60);

/// Shorter than `RELEASE_TTL`: a missing release is the state right before a
/// release publishes, and nobody should wait five minutes for it to appear.
const MISSING_RELEASE_TTL: Duration = Duration::from_secs(60);

/// The working set is a handful of tags older than the cached release list. The
/// cap exists so probing made-up versions cannot grow the map without bound.
const MAX_CACHE_ENTRIES: usize = 128;

/// The API maximum, and the page size we ask for. A response with exactly this
/// many releases means older ones exist that the list does not show.
const RELEASES_PAGE_SIZE: usize = 100;

/// Release metadata is small (a page of 100 releases is well under a megabyte).
/// The cap is what stops a compromised or proxied upstream from turning a
/// metadata fetch into an out-of-memory kill.
const MAX_METADATA_BYTES: usize = 4 * 1024 * 1024;

/// Longer than any real version, short enough that a probed key cannot make the
/// cache or an outbound URL large.
const MAX_VERSION_LEN: usize = 64;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Applies between reads, not to the whole transfer, so a slow client on an
/// 11MB archive is not cut off.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

const METADATA_TIMEOUT: Duration = Duration::from_secs(15);

/// How the bytes reach the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadMode {
    /// Today: stream through proa.so with server-side credentials.
    Proxy,
    /// After the repository opens: 302 to github.com and hold no token at all.
    Redirect,
}

/// Why the resolved mode is what it is. Kept separate from the mode so the
/// resolution stays a pure function and the logging happens at the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModeReason {
    /// Unset, and a token is available, so bytes can be streamed.
    DefaultProxy,
    /// Unset and no token, which is the post-open-source configuration.
    DefaultRedirect,
    /// Set to a value we understand and can honour.
    Configured,
    /// Asked for proxy without a token. Serving 404s for every asset would be
    /// worse than quietly doing the thing that still works.
    ProxyWithoutToken,
    /// Set to something unrecognised.
    Unrecognized,
}

/// Everything the bridge reads from the environment, already validated.
#[derive(Debug, Clone)]
struct BridgeConfig {
    repo: String,
    token: Option<String>,
    mode: DownloadMode,
    mode_reason: ModeReason,
    /// A malformed `PROA_RELEASE_REPO` was ignored in favour of the default.
    repo_override_rejected: bool,
}

impl BridgeConfig {
    /// Pure resolution of the three environment variables. No I/O, no logging,
    /// so the whole configuration matrix is testable.
    fn resolve(repo: Option<String>, token: Option<String>, mode: Option<String>) -> Self {
        let requested_repo = repo
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let parsed_repo = requested_repo.and_then(parse_repo);
        let repo_override_rejected = requested_repo.is_some() && parsed_repo.is_none();

        let token = token
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let (mode, mode_reason) = resolve_mode(mode.as_deref(), token.is_some());

        Self {
            repo: parsed_repo.unwrap_or_else(|| DEFAULT_REPO.to_string()),
            token,
            mode,
            mode_reason,
            repo_override_rejected,
        }
    }

    /// One line at boot describing exactly what this process will do, because
    /// "downloads are 404ing" is otherwise a very quiet failure.
    fn log_startup(&self) {
        if self.repo_override_rejected {
            tracing::warn!(
                repo = %self.repo,
                "PROA_RELEASE_REPO is not an owner/name pair; using the default repository"
            );
        }
        match self.mode_reason {
            ModeReason::ProxyWithoutToken => tracing::warn!(
                repo = %self.repo,
                "PROA_DOWNLOAD_MODE=proxy needs PROA_RELEASE_TOKEN; serving redirects instead"
            ),
            ModeReason::Unrecognized => tracing::warn!(
                repo = %self.repo,
                mode = ?self.mode,
                "PROA_DOWNLOAD_MODE is not proxy or redirect; using the default for this token state"
            ),
            _ => tracing::info!(
                repo = %self.repo,
                mode = ?self.mode,
                authenticated = self.token.is_some(),
                "proa-cli download bridge ready"
            ),
        }
    }
}

/// `None` when the value cannot name a GitHub repository. Rejecting here keeps
/// operator typos out of the URLs we build.
fn parse_repo(value: &str) -> Option<String> {
    let (owner, name) = value.split_once('/')?;
    let segment_ok = |segment: &str| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    if segment_ok(owner) && segment_ok(name) {
        Some(format!("{owner}/{name}"))
    } else {
        None
    }
}

fn resolve_mode(requested: Option<&str>, has_token: bool) -> (DownloadMode, ModeReason) {
    let default = if has_token {
        (DownloadMode::Proxy, ModeReason::DefaultProxy)
    } else {
        (DownloadMode::Redirect, ModeReason::DefaultRedirect)
    };

    let Some(requested) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        return default;
    };

    match requested.to_ascii_lowercase().as_str() {
        "redirect" => (DownloadMode::Redirect, ModeReason::Configured),
        "proxy" if has_token => (DownloadMode::Proxy, ModeReason::Configured),
        "proxy" => (DownloadMode::Redirect, ModeReason::ProxyWithoutToken),
        _ => (default.0, ModeReason::Unrecognized),
    }
}

/// The subset of the GitHub release payload this bridge depends on. Unknown
/// fields are ignored, so GitHub can keep adding them.
#[derive(Debug, Clone, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReleaseAsset {
    id: u64,
    name: String,
}

impl Release {
    /// The whitelist. Comparing against the names GitHub actually published is
    /// strictly stronger than pattern matching a filename: a traversal segment
    /// is not rejected, it simply is not a release asset.
    fn asset(&self, name: &str) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

/// Body of `GET /downloads/cli/latest.json`.
///
/// No checksums on purpose. The installer already downloads and verifies
/// `proa-cli-<version>-SHA256SUMS`; repeating the hashes here would create a
/// second source of truth that can disagree with the first.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct LatestRelease<'a> {
    version: &'a str,
    tag: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cli_version: Option<String>,
    assets: Vec<&'a str>,
}

fn tag_for_version(version: &str) -> String {
    format!("{TAG_PREFIX}{version}")
}

fn version_from_tag(tag: &str) -> Option<&str> {
    tag.strip_prefix(TAG_PREFIX).filter(|rest| !rest.is_empty())
}

#[cfg(test)]
fn required_release_assets(version: &str) -> Vec<String> {
    REQUIRED_RELEASE_ASSET_SUFFIXES
        .iter()
        .map(|suffix| format!("proa-cli-{version}-{suffix}"))
        .collect()
}

/// `^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$`, hand-rolled to avoid pulling a
/// regex engine into the site binary for one pattern. Everything downstream
/// interpolates the version into a URL, so this is the fail-closed gate.
fn is_valid_version(version: &str) -> bool {
    // Bounded first: everything below is linear in the length, and the value
    // reaches a cache key and an outbound URL.
    if version.is_empty() || version.len() > MAX_VERSION_LEN {
        return false;
    }

    let (core, prerelease) = match version.split_once('-') {
        Some((core, rest)) => (core, Some(rest)),
        None => (version, None),
    };

    let mut parts = core.split('.');
    let numeric = matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(major), Some(minor), Some(patch), None)
            if [major, minor, patch].iter().all(|segment| {
                !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit())
            })
    );
    if !numeric {
        return false;
    }

    match prerelease {
        None => true,
        Some(tail) => {
            !tail.is_empty()
                && tail
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
        }
    }
}

/// `None` for anything not comparable, which drops it out of the running for
/// "latest" rather than letting it win by accident.
fn version_ordering(version: &str) -> Option<(u64, u64, u64)> {
    if !is_valid_version(version) {
        return None;
    }
    let core = version.split_once('-').map_or(version, |(core, _)| core);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

#[derive(Debug, PartialEq, Eq)]
struct MainSnapshotIdentity<'a> {
    cli_core_version: &'a str,
    commit: &'a str,
    cli_prerelease: Option<&'a str>,
}

impl MainSnapshotIdentity<'_> {
    fn cli_version(&self) -> String {
        match self.cli_prerelease {
            Some(prerelease) => format!("{}-{prerelease}", self.cli_core_version),
            None => self.cli_core_version.to_string(),
        }
    }
}

/// Parse the immutable identity used by automatic post-main-CI snapshots:
/// `<version-core>-main.<full git sha>[.<original prerelease>]`.
fn main_snapshot_identity(version: &str) -> Option<MainSnapshotIdentity<'_>> {
    let (cli_core_version, suffix) = version.split_once("-main.")?;
    let (commit, cli_prerelease) = suffix
        .split_once('.')
        .map_or((suffix, None), |(commit, prerelease)| {
            (commit, Some(prerelease))
        });
    if version_ordering(cli_core_version).is_none()
        || commit.len() != 40
        || !commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        || cli_prerelease.is_some_and(|prerelease| {
            prerelease.is_empty()
                || !prerelease
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
        })
    {
        return None;
    }
    Some(MainSnapshotIdentity {
        cli_core_version,
        commit,
        cli_prerelease,
    })
}

fn release_is_complete(release: &Release) -> bool {
    let Some(version) =
        version_from_tag(&release.tag_name).filter(|version| is_valid_version(version))
    else {
        return false;
    };

    REQUIRED_RELEASE_ASSET_SUFFIXES.iter().all(|required| {
        release.assets.iter().any(|asset| {
            asset
                .name
                .strip_prefix("proa-cli-")
                .and_then(|name| name.strip_prefix(version))
                .and_then(|name| name.strip_prefix('-'))
                == Some(*required)
        })
    })
}

/// Highest published proa-cli version, not the most recently created release.
/// Those differ the moment a patch is backported onto an older line, and the
/// installer must never be told that 0.1.4 is newer than 0.2.0.
///
/// Drafts have no publicly downloadable assets, and prereleases are not what an
/// unqualified `install.sh` should land on.
fn select_latest_release(releases: &[Release]) -> Option<&Release> {
    releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter(|release| release_is_complete(release))
        .filter_map(|release| {
            let version = version_from_tag(&release.tag_name)?;
            Some((version_ordering(version)?, release))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, release)| release)
}

/// Main snapshots are immutable and a stale workflow is refused at publication
/// time. Select by GitHub's publication timestamp instead of depending on the
/// order of the release-list response.
fn select_latest_main_release(releases: &[Release]) -> Option<&Release> {
    releases
        .iter()
        .filter(|release| {
            !release.draft
                && release.prerelease
                && release_is_complete(release)
                && version_from_tag(&release.tag_name)
                    .and_then(main_snapshot_identity)
                    .is_some()
        })
        .filter_map(|release| Some((release.published_at.as_deref()?, release)))
        .max_by(|left, right| left.0.cmp(right.0))
        .map(|(_, release)| release)
}

fn latest_payload(release: &Release) -> Option<LatestRelease<'_>> {
    if !release_is_complete(release) {
        return None;
    }
    let version = version_from_tag(&release.tag_name)?;
    Some(LatestRelease {
        version,
        tag: &release.tag_name,
        channel: None,
        commit: None,
        cli_version: None,
        assets: release
            .assets
            .iter()
            .map(|asset| asset.name.as_str())
            .collect(),
    })
}

fn main_latest_payload(release: &Release) -> Option<LatestRelease<'_>> {
    if release.draft || !release.prerelease || !release_is_complete(release) {
        return None;
    }
    let version = version_from_tag(&release.tag_name)?;
    let identity = main_snapshot_identity(version)?;
    Some(LatestRelease {
        version,
        tag: &release.tag_name,
        channel: Some("main"),
        commit: Some(identity.commit),
        cli_version: Some(identity.cli_version()),
        assets: release
            .assets
            .iter()
            .map(|asset| asset.name.as_str())
            .collect(),
    })
}

/// Where the bridge talks to. Constants in production; a local mock in tests, so
/// the whole request path can be exercised without reaching api.github.com.
/// Never derived from a request, so no client input can retarget it.
#[derive(Debug, Clone)]
struct Endpoints {
    api: String,
    web: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            api: GITHUB_API.to_string(),
            web: GITHUB_WEB.to_string(),
        }
    }
}

fn releases_url(api: &str, repo: &str) -> String {
    // 100 is the API maximum. The monorepo tags several crates, so a smaller
    // page could push every proa-cli release off the end of the list.
    format!("{api}/repos/{repo}/releases?per_page={RELEASES_PAGE_SIZE}")
}

fn release_by_tag_url(api: &str, repo: &str, tag: &str) -> String {
    // The tag is built from an already-validated version, so every character
    // here is URL safe and no escaping is needed.
    format!("{api}/repos/{repo}/releases/tags/{tag}")
}

fn asset_api_url(api: &str, repo: &str, asset_id: u64) -> String {
    format!("{api}/repos/{repo}/releases/assets/{asset_id}")
}

fn browser_download_url(web: &str, repo: &str, tag: &str, filename: &str) -> String {
    format!("{web}/{repo}/releases/download/{tag}/{filename}")
}

/// Two outcomes, both boring. No variant carries a message, which is what makes
/// it impossible for a credential to reach a client through an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
enum BridgeError {
    #[error("no matching proa-cli release or asset")]
    NotFound,
    #[error("the upstream release host is unavailable")]
    Unavailable,
}

/// What a GitHub response means to us, before any of it is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpstreamStatus {
    Ok,
    /// The release or asset genuinely does not exist.
    Missing,
    /// Quota exhausted. Worth its own log line: it is an operational signal,
    /// not a user error.
    RateLimited,
    /// Everything else, including a rejected token. Never distinguished to the
    /// client, which would otherwise learn about our credentials.
    Failed,
}

fn rate_limit_exhausted(headers: &HeaderMap) -> bool {
    headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim() == "0")
}

fn classify_status(status: StatusCode, headers: &HeaderMap) -> UpstreamStatus {
    if status.is_success() {
        return UpstreamStatus::Ok;
    }
    if status == StatusCode::NOT_FOUND {
        return UpstreamStatus::Missing;
    }
    if matches!(
        status,
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    ) && rate_limit_exhausted(headers)
    {
        return UpstreamStatus::RateLimited;
    }
    UpstreamStatus::Failed
}

/// Only ever a `&'static str` reaches a log line, so no request detail and no
/// credential can be carried into one by a transport error.
fn transport_reason(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_decode() {
        "decode"
    } else if error.is_body() {
        "body"
    } else {
        "transport"
    }
}

#[derive(Debug, Clone)]
struct CacheEntry {
    /// `None` records "GitHub says this does not exist", which is worth caching
    /// too: today there is no proa-cli release at all and every installer run
    /// would otherwise cost an API call.
    release: Option<Arc<Release>>,
    stored_at: Instant,
}

#[derive(Debug, Clone)]
enum CacheLookup {
    Miss,
    Present(Arc<Release>),
    Absent,
}

/// One page of the release list, cached whole. Both routes resolve out of this,
/// which is what keeps a made-up version from costing a GitHub API call.
#[derive(Debug, Clone)]
struct ListEntry {
    releases: Arc<Vec<Release>>,
    /// The page came back full, so releases older than it exist and a per-tag
    /// lookup for a version missing from the list is still meaningful.
    truncated: bool,
    stored_at: Instant,
}

fn ttl_for(present: bool) -> Duration {
    if present {
        RELEASE_TTL
    } else {
        MISSING_RELEASE_TTL
    }
}

fn entry_is_fresh(entry: &CacheEntry, now: Instant) -> bool {
    let is_published = entry
        .release
        .as_deref()
        .is_some_and(|release| !release.draft);
    now.duration_since(entry.stored_at) < ttl_for(is_published)
}

fn list_is_fresh(entry: &ListEntry, now: Instant, complete_ttl: Duration) -> bool {
    // An empty or incomplete page is the state while a release is publishing,
    // so it expires on the short TTL. A complete stable list can live for five
    // minutes; callers resolving the moving main channel request one minute.
    let has_cli_release = entry
        .releases
        .iter()
        .any(|release| !release.draft && release_is_complete(release));
    let ttl = if has_cli_release {
        complete_ttl
    } else {
        MISSING_RELEASE_TTL
    };
    now.duration_since(entry.stored_at) < ttl
}

/// Without this, every download costs a GitHub API call and the 5000/hour
/// authenticated limit is reachable from a single busy CI fleet, or from one
/// attacker walking the version space.
#[derive(Debug, Default)]
struct ReleaseCache {
    /// The release list, which answers almost every request on its own.
    list: RwLock<Option<ListEntry>>,
    /// Per-tag results, only ever populated for versions older than the list.
    entries: RwLock<HashMap<String, CacheEntry>>,
}

impl ReleaseCache {
    async fn list_at(
        &self,
        now: Instant,
        complete_ttl: Duration,
    ) -> Option<(Arc<Vec<Release>>, bool)> {
        let list = self.list.read().await;
        list.as_ref()
            .filter(|entry| list_is_fresh(entry, now, complete_ttl))
            .map(|entry| (Arc::clone(&entry.releases), entry.truncated))
    }

    async fn store_list_at(&self, releases: Arc<Vec<Release>>, truncated: bool, now: Instant) {
        *self.list.write().await = Some(ListEntry {
            releases,
            truncated,
            stored_at: now,
        });
    }

    async fn get_at(&self, key: &str, now: Instant) -> CacheLookup {
        let entries = self.entries.read().await;
        match entries.get(key) {
            Some(entry) if entry_is_fresh(entry, now) => match &entry.release {
                Some(release) => CacheLookup::Present(Arc::clone(release)),
                None => CacheLookup::Absent,
            },
            _ => CacheLookup::Miss,
        }
    }

    /// False when the map is full of entries that are all still fresh, which
    /// only happens while the tag space is being probed. Refusing the lookup
    /// bounds per-tag API calls to `MAX_CACHE_ENTRIES` per TTL window instead
    /// of letting an attacker spend the whole GitHub quota.
    async fn admits_at(&self, key: &str, now: Instant) -> bool {
        let mut entries = self.entries.write().await;
        if entries.contains_key(key) || entries.len() < MAX_CACHE_ENTRIES {
            return true;
        }
        entries.retain(|_, entry| entry_is_fresh(entry, now));
        entries.len() < MAX_CACHE_ENTRIES
    }

    async fn store_at(&self, key: String, release: Option<Arc<Release>>, now: Instant) {
        let mut entries = self.entries.write().await;
        if entries.len() >= MAX_CACHE_ENTRIES && !entries.contains_key(&key) {
            entries.retain(|_, entry| entry_is_fresh(entry, now));
            if entries.len() >= MAX_CACHE_ENTRIES {
                // Admission control above should have refused the lookup that
                // produced this. Dropping the value keeps the map bounded.
                return;
            }
        }
        entries.insert(
            key,
            CacheEntry {
                release,
                stored_at: now,
            },
        );
    }
}

/// Shared state for both routes: configuration, one pooled HTTP client, and the
/// release metadata cache.
struct DownloadBridge {
    config: BridgeConfig,
    /// `None` when the HTTP client could not be constructed. That is a boot
    /// problem, and answering 503 beats panicking the whole site.
    client: Option<reqwest::Client>,
    endpoints: Endpoints,
    cache: ReleaseCache,
    /// Coalesce simultaneous cache misses into one GitHub metadata request.
    list_refresh: Mutex<()>,
}

impl DownloadBridge {
    fn from_env() -> Self {
        let config = BridgeConfig::resolve(
            env::var("PROA_RELEASE_REPO").ok(),
            env::var("PROA_RELEASE_TOKEN").ok(),
            env::var("PROA_DOWNLOAD_MODE").ok(),
        );
        config.log_startup();
        Self::new(config, build_client())
    }

    fn new(config: BridgeConfig, client: Option<reqwest::Client>) -> Self {
        Self {
            config,
            client,
            endpoints: Endpoints::default(),
            cache: ReleaseCache::default(),
            list_refresh: Mutex::new(()),
        }
    }

    #[cfg(test)]
    fn with_endpoints(mut self, endpoints: Endpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    fn client(&self) -> Result<&reqwest::Client, BridgeError> {
        self.client.as_ref().ok_or(BridgeError::Unavailable)
    }

    /// The client's own Authorization header is never consulted, so a client
    /// cannot borrow our GitHub access by presenting one.
    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.config.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    /// `Ok(None)` means GitHub answered 404, which is a fact about the release,
    /// not a failure.
    async fn fetch_metadata<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<Option<T>, BridgeError> {
        let request = self
            .client()?
            .get(url)
            .header(header::ACCEPT, "application/vnd.github+json")
            .header("x-github-api-version", GITHUB_API_VERSION)
            .timeout(METADATA_TIMEOUT);

        let response = match self.authorize(request).send().await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(
                    reason = transport_reason(&error),
                    url,
                    "github release metadata request failed"
                );
                return Err(BridgeError::Unavailable);
            }
        };

        match classify_status(response.status(), response.headers()) {
            UpstreamStatus::Ok => {}
            UpstreamStatus::Missing => return Ok(None),
            UpstreamStatus::RateLimited => {
                tracing::warn!(url, "github api rate limit exhausted");
                return Err(BridgeError::Unavailable);
            }
            UpstreamStatus::Failed => {
                tracing::warn!(
                    status = response.status().as_u16(),
                    url,
                    "github release metadata request rejected"
                );
                return Err(BridgeError::Unavailable);
            }
        }

        // Bounded, unlike `response.json()`: metadata is small, and an upstream
        // that answers with gigabytes must cost us a 503 rather than the
        // process. The declared length is only a hint, so the stream is capped
        // as it arrives too.
        let body = match read_capped(response, MAX_METADATA_BYTES).await {
            Ok(body) => body,
            Err(reason) => {
                tracing::warn!(reason, url, "github release metadata could not be read");
                return Err(BridgeError::Unavailable);
            }
        };

        match serde_json::from_slice::<T>(&body) {
            Ok(payload) => Ok(Some(payload)),
            Err(_) => {
                // The serde error names a byte offset and the surrounding JSON,
                // so only the fact of the failure is logged.
                tracing::warn!(url, "github release metadata could not be decoded");
                Err(BridgeError::Unavailable)
            }
        }
    }

    /// One page of releases, cached whole. Every route resolves out of this, so
    /// a request for a version that was never published costs no API call.
    async fn release_list(
        &self,
        complete_ttl: Duration,
    ) -> Result<(Arc<Vec<Release>>, bool), BridgeError> {
        let now = Instant::now();
        if let Some(hit) = self.cache.list_at(now, complete_ttl).await {
            return Ok(hit);
        }

        // An expired list is a shared event. Serialize only the refresh, then
        // check again because another request may have filled it while waiting.
        let _refresh = self.list_refresh.lock().await;
        if let Some(hit) = self.cache.list_at(Instant::now(), complete_ttl).await {
            return Ok(hit);
        }

        let url = releases_url(&self.endpoints.api, &self.config.repo);
        let releases: Vec<Release> = match self.fetch_metadata(&url).await? {
            Some(releases) => releases,
            None => {
                // A 404 on the release list means the repository is invisible to
                // this process, which is a deployment problem worth naming.
                tracing::warn!(repo = %self.config.repo, "release list is not visible");
                Vec::new()
            }
        };

        let truncated = releases.len() >= RELEASES_PAGE_SIZE;
        let releases = Arc::new(releases);
        self.cache
            .store_list_at(Arc::clone(&releases), truncated, Instant::now())
            .await;
        Ok((releases, truncated))
    }

    /// Only for versions older than the cached list page. Guarded by admission
    /// control because the tag here is derived from client input.
    async fn release_by_tag(&self, tag: &str) -> Result<Arc<Release>, BridgeError> {
        let now = Instant::now();
        match self.cache.get_at(tag, now).await {
            CacheLookup::Present(release) => return Ok(release),
            CacheLookup::Absent => return Err(BridgeError::NotFound),
            CacheLookup::Miss => {}
        }

        if !self.cache.admits_at(tag, now).await {
            // 503 rather than 404: a genuine older version must be retried, not
            // remembered as missing by every cache between here and the client.
            tracing::warn!("release tag cache is saturated; refusing an upstream lookup");
            return Err(BridgeError::Unavailable);
        }

        let url = release_by_tag_url(&self.endpoints.api, &self.config.repo, tag);
        let release = self.fetch_metadata::<Release>(&url).await?.map(Arc::new);
        self.cache
            .store_at(tag.to_string(), release.clone(), Instant::now())
            .await;
        release.ok_or(BridgeError::NotFound)
    }

    async fn asset(
        &self,
        version: &str,
        filename: &str,
        list_ttl: Duration,
    ) -> Result<Response, BridgeError> {
        // Fail closed before anything leaves the process.
        if !is_valid_version(version) {
            return Err(BridgeError::NotFound);
        }

        let tag = tag_for_version(version);
        let (releases, truncated) = self.release_list(list_ttl).await?;

        // The cached list is the whitelist. Only a version old enough to have
        // fallen off the page is worth an extra API call, which is what stops a
        // walk of the version space from spending the GitHub quota.
        let older_than_the_list;
        let release = match releases.iter().find(|release| release.tag_name == tag) {
            Some(release) => release,
            None if truncated => {
                older_than_the_list = self.release_by_tag(&tag).await?;
                &*older_than_the_list
            }
            None => return Err(BridgeError::NotFound),
        };

        // Publishing is a two-phase transaction: the workflow uploads and
        // verifies every asset on a draft, then makes the release visible.
        // Authenticated API responses include drafts, so pinned URLs must
        // enforce this boundary themselves.
        if release.draft {
            return Err(BridgeError::NotFound);
        }

        let asset = release.asset(filename).ok_or(BridgeError::NotFound)?;

        // From here on the published name is used, never the client's string.
        match self.config.mode {
            DownloadMode::Redirect => Ok(redirect_response(&browser_download_url(
                &self.endpoints.web,
                &self.config.repo,
                &tag,
                &asset.name,
            ))),
            DownloadMode::Proxy => self.proxy_asset(asset.id).await,
        }
    }

    async fn proxy_asset(&self, asset_id: u64) -> Result<Response, BridgeError> {
        let url = asset_api_url(&self.endpoints.api, &self.config.repo, asset_id);
        let request = self
            .client()?
            .get(&url)
            .header(header::ACCEPT, "application/octet-stream");

        let upstream = match self.authorize(request).send().await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(
                    reason = transport_reason(&error),
                    url,
                    "github release asset request failed"
                );
                return Err(BridgeError::Unavailable);
            }
        };

        match classify_status(upstream.status(), upstream.headers()) {
            UpstreamStatus::Ok => {}
            UpstreamStatus::Missing => return Err(BridgeError::NotFound),
            UpstreamStatus::RateLimited => {
                tracing::warn!(url, "github api rate limit exhausted");
                return Err(BridgeError::Unavailable);
            }
            UpstreamStatus::Failed => {
                tracing::warn!(
                    status = upstream.status().as_u16(),
                    url,
                    "github release asset request rejected"
                );
                return Err(BridgeError::Unavailable);
            }
        }

        // Only the length is taken from upstream. Every other response header is
        // constructed here, so nothing GitHub sends is forwarded blind.
        let length = upstream.content_length();
        let mut builder = Response::builder().status(StatusCode::OK);
        for (name, value) in archive_headers() {
            builder = builder.header(name, value);
        }
        if let Some(length) = length {
            builder = builder.header(header::CONTENT_LENGTH, length);
        }

        // Streamed, not buffered: an 11MB archive must not sit in memory once
        // per concurrent download.
        builder
            .body(Body::from_stream(upstream.bytes_stream()))
            .map_err(|error| {
                tracing::error!(%error, "could not build the asset response");
                BridgeError::Unavailable
            })
    }
}

/// Collect a response body, refusing to buffer more than `limit` bytes. The
/// error is a `&'static str` so that, like `transport_reason`, nothing carrying
/// a URL or a credential can reach a log line.
async fn read_capped(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, &'static str> {
    if response
        .content_length()
        .is_some_and(|len| len > limit as u64)
    {
        return Err("oversize");
    }

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| transport_reason(&error))?;
        if body.len() + chunk.len() > limit {
            return Err("oversize");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn build_client() -> Option<reqwest::Client> {
    // No total-request timeout: it would apply to the whole asset transfer and
    // cut off slow clients mid-archive.
    match reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        // GitHub answers the asset API with a redirect to storage. reqwest drops
        // the Authorization header when a redirect crosses hosts, so the token
        // never reaches the storage provider.
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
    {
        Ok(client) => Some(client),
        Err(error) => {
            tracing::error!(%error, "could not build the release download client");
            None
        }
    }
}

fn archive_headers() -> [(HeaderName, &'static str); 3] {
    [
        (header::CONTENT_TYPE, "application/octet-stream"),
        (header::CACHE_CONTROL, ARCHIVE_CACHE_CONTROL),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
    ]
}

/// 302 rather than 307 or 308: the installers follow it with a plain GET, and
/// this is the mode the bridge lands in permanently once the repository opens.
fn redirect_response(location: &str) -> Response {
    let Ok(location) = HeaderValue::from_str(location) else {
        tracing::error!("release download URL is not a valid header value");
        return error_response(BridgeError::NotFound);
    };

    (
        StatusCode::FOUND,
        [
            (header::LOCATION, location),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(ARCHIVE_CACHE_CONTROL),
            ),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ),
        ],
        (),
    )
        .into_response()
}

fn latest_response(release: &Release) -> Response {
    let Some(payload) = latest_payload(release) else {
        return error_response(BridgeError::NotFound);
    };
    let Ok(body) = serde_json::to_vec(&payload) else {
        tracing::error!("latest release payload could not be serialized");
        return error_response(BridgeError::Unavailable);
    };

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, JSON_CONTENT_TYPE),
            (header::CACHE_CONTROL, METADATA_CACHE_CONTROL),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        body,
    )
        .into_response()
}

fn main_latest_response(release: &Release) -> Response {
    let Some(payload) = main_latest_payload(release) else {
        return error_response(BridgeError::NotFound);
    };
    let Ok(body) = serde_json::to_vec(&payload) else {
        tracing::error!("main latest release payload could not be serialized");
        return error_response(BridgeError::Unavailable);
    };

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, JSON_CONTENT_TYPE),
            (header::CACHE_CONTROL, MAIN_METADATA_CACHE_CONTROL),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        body,
    )
        .into_response()
}

fn error_response(error: BridgeError) -> Response {
    match error {
        BridgeError::NotFound => (
            StatusCode::NOT_FOUND,
            [
                (header::CONTENT_TYPE, JSON_CONTENT_TYPE),
                (header::CACHE_CONTROL, NOT_FOUND_CACHE_CONTROL),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            NOT_FOUND_BODY,
        )
            .into_response(),
        BridgeError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            [
                (header::CONTENT_TYPE, JSON_CONTENT_TYPE),
                (header::CACHE_CONTROL, UNAVAILABLE_CACHE_CONTROL),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::RETRY_AFTER, "30"),
            ],
            UNAVAILABLE_BODY,
        )
            .into_response(),
    }
}

async fn latest_handler(State(bridge): State<Arc<DownloadBridge>>) -> Response {
    match bridge.release_list(RELEASE_TTL).await {
        // Rendered straight from the cached list, so serving latest.json costs
        // neither an API call nor a copy of the release.
        Ok((releases, _)) => match select_latest_release(&releases) {
            Some(release) => latest_response(release),
            None => error_response(BridgeError::NotFound),
        },
        Err(error) => error_response(error),
    }
}

async fn main_latest_handler(State(bridge): State<Arc<DownloadBridge>>) -> Response {
    match bridge.release_list(RELEASE_LIST_TTL).await {
        Ok((releases, _)) => match select_latest_main_release(&releases) {
            Some(release) => main_latest_response(release),
            None => error_response(BridgeError::NotFound),
        },
        Err(error) => error_response(error),
    }
}

async fn asset_handler(
    State(bridge): State<Arc<DownloadBridge>>,
    Path((version, filename)): Path<(String, String)>,
) -> Response {
    if main_snapshot_identity(&version).is_some() {
        return error_response(BridgeError::NotFound);
    }
    match bridge.asset(&version, &filename, RELEASE_TTL).await {
        Ok(response) => response,
        Err(error) => error_response(error),
    }
}

async fn main_asset_handler(
    State(bridge): State<Arc<DownloadBridge>>,
    Path((version, filename)): Path<(String, String)>,
) -> Response {
    if main_snapshot_identity(&version).is_none() {
        return error_response(BridgeError::NotFound);
    }
    match bridge.asset(&version, &filename, RELEASE_LIST_TTL).await {
        Ok(response) => response,
        Err(error) => error_response(error),
    }
}

pub fn router() -> Router {
    router_with(Arc::new(DownloadBridge::from_env()))
}

fn router_with(bridge: Arc<DownloadBridge>) -> Router {
    Router::new()
        .route("/downloads/cli/latest.json", get(latest_handler))
        .route("/downloads/cli/main/latest.json", get(main_latest_handler))
        .route(
            "/downloads/cli/main/:version/:filename",
            get(main_asset_handler),
        )
        .route("/downloads/cli/:version/:filename", get(asset_handler))
        .with_state(bridge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower_test::ServiceExt;

    const TOKEN: &str = "ghp_secret_release_token_value";
    const MAIN_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn release(tag: &str, assets: &[&str]) -> Release {
        Release {
            tag_name: tag.to_string(),
            draft: false,
            prerelease: false,
            published_at: Some("2026-01-01T00:00:00Z".to_string()),
            assets: assets
                .iter()
                .enumerate()
                .map(|(index, name)| ReleaseAsset {
                    id: 100 + index as u64,
                    name: (*name).to_string(),
                })
                .collect(),
        }
    }

    /// The exact asset set `.github/workflows/release-cli.yml` publishes.
    fn published_assets(version: &str) -> Vec<String> {
        required_release_assets(version)
    }

    fn released(version: &str) -> Release {
        let names = published_assets(version);
        let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
        release(&tag_for_version(version), &borrowed)
    }

    fn main_released(cli_version: &str, commit: &str) -> Release {
        let (core, prerelease) = cli_version
            .split_once('-')
            .map_or((cli_version, None), |(core, prerelease)| {
                (core, Some(prerelease))
            });
        let mut version = format!("{core}-main.{commit}");
        if let Some(prerelease) = prerelease {
            version.push('.');
            version.push_str(prerelease);
        }
        let mut release = released(&version);
        release.prerelease = true;
        release
    }

    fn config_with_token() -> BridgeConfig {
        BridgeConfig::resolve(None, Some(TOKEN.to_string()), None)
    }

    async fn body_of(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn headers_as_text(response: &Response) -> String {
        response
            .headers()
            .iter()
            .map(|(name, value)| format!("{name}: {}", value.to_str().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // -- version grammar ---------------------------------------------------

    #[test]
    fn version_grammar_accepts_what_the_release_workflow_publishes() {
        for version in [
            "0.1.0",
            "1.0.0",
            "10.20.30",
            "0.1.0-rc.1",
            "0.1.0-main.0123456789abcdef0123456789abcdef01234567",
            "1.2.3-alpha1",
            "1.2.3-0",
            "01.2.3",
        ] {
            assert!(is_valid_version(version), "{version} should be accepted");
        }
    }

    #[test]
    fn version_grammar_rejects_everything_else() {
        for version in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.3-",
            "-1.2.3",
            "1.2.3+build",
            "1.2.3-rc-1",
            "1.2.3 ",
            " 1.2.3",
            "1.2.3/../..",
            "../../../etc/passwd",
            "..",
            ".",
            "1.2.3%2f",
            "1.2.x",
            "latest",
            "1.2.3-rc 1",
            "１.２.３",
        ] {
            assert!(!is_valid_version(version), "{version} should be rejected");
        }
    }

    #[test]
    fn version_grammar_is_length_bounded() {
        // All digits, so only the length can reject it. Without the bound this
        // becomes a cache key and an outbound URL of the client's choosing.
        let long = format!("{}.1.0", "9".repeat(MAX_VERSION_LEN));
        assert!(!is_valid_version(&long));
        assert!(!is_valid_version(&format!(
            "1.2.3-{}",
            "a".repeat(MAX_VERSION_LEN)
        )));
        assert!(is_valid_version(&format!(
            "1.2.3-{}",
            "a".repeat(MAX_VERSION_LEN - 6)
        )));
    }

    #[test]
    fn tag_matches_the_release_workflow_contract() {
        assert_eq!(tag_for_version("0.1.0"), "proa-cli-v0.1.0");
        assert_eq!(version_from_tag("proa-cli-v0.1.0"), Some("0.1.0"));
        assert_eq!(version_from_tag("proa-cli-v0.1.0-rc.1"), Some("0.1.0-rc.1"));
        assert_eq!(version_from_tag("proa-cli-v"), None);
        assert_eq!(version_from_tag("proa_ui-v1.0.0"), None);
        assert_eq!(version_from_tag("v0.1.0"), None);
    }

    #[test]
    fn version_ordering_ignores_uncomparable_versions() {
        assert_eq!(version_ordering("1.2.3"), Some((1, 2, 3)));
        assert_eq!(version_ordering("0.10.0"), Some((0, 10, 0)));
        assert_eq!(version_ordering("1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(version_ordering("nonsense"), None);
        assert_eq!(
            version_ordering("99999999999999999999.0.0"),
            None,
            "an unparsable number must not win by accident"
        );
    }

    #[test]
    fn main_snapshot_identity_requires_a_full_commit() {
        assert_eq!(
            main_snapshot_identity(&format!("0.1.1-main.{MAIN_SHA}")),
            Some(MainSnapshotIdentity {
                cli_core_version: "0.1.1",
                commit: MAIN_SHA,
                cli_prerelease: None,
            })
        );
        let prerelease_version = format!("0.2.0-main.{MAIN_SHA}.rc.1");
        let prerelease =
            main_snapshot_identity(&prerelease_version).expect("a prerelease main identity");
        assert_eq!(prerelease.cli_version(), "0.2.0-rc.1");
        assert_eq!(
            prerelease.commit, MAIN_SHA,
            "the suffix must not become part of the commit"
        );
        for version in [
            "0.1.1",
            "0.1.1-rc.1",
            "0.1.1-main.deadbeef",
            "0.1.1-main.0123456789abcdef0123456789abcdef01234567.",
            "0.1.1-main.0123456789abcdef0123456789abcdef0123456g",
            "0.1.1-main.0123456789ABCDEF0123456789ABCDEF01234567",
            "not-a-version-main.0123456789abcdef0123456789abcdef01234567",
        ] {
            assert!(main_snapshot_identity(version).is_none(), "{version}");
        }
    }

    // -- latest resolution -------------------------------------------------

    #[test]
    fn latest_selection_ignores_other_crates_tags() {
        let releases = vec![
            release("proa_ui-v9.9.9", &["proa_ui.tar.gz"]),
            release("proa-v2.0.0", &["proa.tar.gz"]),
            released("0.1.0"),
        ];
        let latest = select_latest_release(&releases).expect("a proa-cli release");
        assert_eq!(latest.tag_name, "proa-cli-v0.1.0");
    }

    #[test]
    fn latest_selection_prefers_the_highest_version_not_the_newest_tag() {
        // A backport published after 0.2.0 sits first in the API's
        // newest-first ordering, and must not be served as latest.
        let releases = vec![released("0.1.4"), released("0.2.0"), released("0.1.3")];
        let latest = select_latest_release(&releases).expect("a proa-cli release");
        assert_eq!(latest.tag_name, "proa-cli-v0.2.0");
    }

    #[test]
    fn latest_selection_skips_drafts_and_prereleases() {
        let mut draft = released("0.3.0");
        draft.draft = true;
        let mut prerelease = released("0.2.0-rc.1");
        prerelease.prerelease = true;
        let releases = vec![draft, prerelease, released("0.1.0")];
        let latest = select_latest_release(&releases).expect("a stable release");
        assert_eq!(latest.tag_name, "proa-cli-v0.1.0");
    }

    #[test]
    fn latest_selection_skips_a_newer_incomplete_release() {
        let incomplete = release(
            "proa-cli-v0.2.0",
            &[
                // Old-platform assets do not make the current Ubuntu
                // distribution contract complete.
                "proa-cli-0.2.0-aarch64-apple-darwin.tar.gz",
                "proa-cli-0.2.0-x86_64-apple-darwin.tar.gz",
                "proa-cli-0.2.0-SHA256SUMS",
            ],
        );
        let releases = vec![incomplete, released("0.1.0")];
        let latest = select_latest_release(&releases).expect("a complete stable release");
        assert_eq!(latest.tag_name, "proa-cli-v0.1.0");
    }

    #[test]
    fn latest_selection_is_empty_when_no_cli_release_exists() {
        assert!(select_latest_release(&[]).is_none());
        assert!(select_latest_release(&[release("proa_ui-v1.0.0", &["x.tar.gz"])]).is_none());
    }

    #[test]
    fn main_selection_uses_the_newest_complete_main_snapshot_only() {
        let older_sha = "1111111111111111111111111111111111111111";
        let mut newest = main_released("0.1.1", MAIN_SHA);
        newest.published_at = Some("2026-08-25T20:00:00Z".to_string());
        let mut older = main_released("0.1.1", older_sha);
        older.published_at = Some("2026-08-25T19:00:00Z".to_string());
        let mut release_candidate = released("0.1.2-rc.1");
        release_candidate.prerelease = true;
        let releases = vec![older, release_candidate, newest, released("9.9.9")];

        let latest = select_latest_main_release(&releases).expect("a main snapshot");
        assert_eq!(latest.tag_name, format!("proa-cli-v0.1.1-main.{MAIN_SHA}"));
    }

    #[test]
    fn main_selection_skips_drafts_and_incomplete_snapshots() {
        let mut draft = main_released("0.1.1", MAIN_SHA);
        draft.draft = true;
        let version = format!("0.1.1-main.{MAIN_SHA}");
        let mut incomplete = release(
            &tag_for_version(&version),
            &["proa-cli-0.1.1-main.0123456789abcdef0123456789abcdef01234567-SHA256SUMS"],
        );
        incomplete.prerelease = true;
        let mut unpublished = main_released("0.1.1", "2222222222222222222222222222222222222222");
        unpublished.published_at = None;

        assert!(select_latest_main_release(&[draft, incomplete, unpublished]).is_none());
    }

    #[test]
    fn releases_parse_from_a_github_shaped_payload() {
        let payload = r#"[
          {
            "url": "https://api.github.com/repos/Proa-Labs/proa/releases/1",
            "id": 1,
            "tag_name": "proa-cli-v0.1.0",
            "name": "proa-cli v0.1.0",
            "draft": false,
            "prerelease": false,
            "created_at": "2026-08-13T00:00:00Z",
            "published_at": "2026-08-13T00:01:00Z",
            "assets": [
              {
                "id": 4242,
                "name": "proa-cli-0.1.0-aarch64-apple-darwin.tar.gz",
                "size": 11000000,
                "content_type": "application/gzip",
                "browser_download_url": "https://github.com/Proa-Labs/proa/releases/download/proa-cli-v0.1.0/proa-cli-0.1.0-aarch64-apple-darwin.tar.gz"
              },
              { "id": 4243, "name": "proa-cli-0.1.0-SHA256SUMS", "size": 512 }
            ]
          }
        ]"#;
        let releases: Vec<Release> = serde_json::from_str(payload).unwrap();
        assert_eq!(releases[0].assets.len(), 2);
        assert_eq!(
            releases[0].published_at.as_deref(),
            Some("2026-08-13T00:01:00Z")
        );
        assert_eq!(
            releases[0].asset("proa-cli-0.1.0-SHA256SUMS").unwrap().id,
            4243
        );
        assert!(
            select_latest_release(&releases).is_none(),
            "a partially uploaded release must not become latest"
        );
    }

    #[test]
    fn release_missing_optional_fields_still_parses() {
        // A release with no assets yet, mid-upload.
        let releases: Vec<Release> =
            serde_json::from_str(r#"[{"tag_name": "proa-cli-v0.1.0"}]"#).unwrap();
        assert_eq!(releases[0].assets.len(), 0);
        assert!(!releases[0].draft);
    }

    // -- latest.json shape -------------------------------------------------

    #[test]
    fn latest_payload_carries_version_tag_and_asset_names_only() {
        let release = released("0.1.0");
        let payload = latest_payload(&release).expect("a payload");
        let json = serde_json::to_value(&payload).unwrap();
        let object = json.as_object().unwrap();

        assert_eq!(object.len(), 3, "latest.json stays deliberately small");
        assert_eq!(object["version"], "0.1.0");
        assert_eq!(object["tag"], "proa-cli-v0.1.0");

        let assets: Vec<&str> = object["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(assets, published_assets("0.1.0"));
        assert!(assets.contains(&"proa-cli-0.1.0-x86_64-unknown-linux-gnu.tar.gz"));
        assert!(assets.contains(&"proa-cli-0.1.0-SHA256SUMS"));
    }

    #[test]
    fn main_payload_names_the_channel_commit_and_binary_version() {
        let release = main_released("0.1.1", MAIN_SHA);
        let payload = main_latest_payload(&release).expect("a main payload");
        let json = serde_json::to_value(payload).unwrap();

        assert_eq!(json["channel"], "main");
        assert_eq!(json["commit"], MAIN_SHA);
        assert_eq!(json["cli_version"], "0.1.1");
        assert_eq!(json["version"], format!("0.1.1-main.{MAIN_SHA}"));
        assert_eq!(json["tag"], format!("proa-cli-v0.1.1-main.{MAIN_SHA}"));

        let prerelease = main_released("0.2.0-rc.1", MAIN_SHA);
        let json = serde_json::to_value(main_latest_payload(&prerelease).unwrap()).unwrap();
        assert_eq!(json["cli_version"], "0.2.0-rc.1");
        assert_eq!(json["version"], format!("0.2.0-main.{MAIN_SHA}.rc.1"));
    }

    #[test]
    fn latest_payload_never_duplicates_the_checksums() {
        let body = serde_json::to_string(&latest_payload(&released("0.1.0")).unwrap()).unwrap();
        assert!(
            !body.contains("sha256") && !body.contains("checksum"),
            "SHA256SUMS is the single source of truth for hashes: {body}"
        );
    }

    #[test]
    fn latest_payload_rejects_a_foreign_tag() {
        assert!(latest_payload(&release("proa_ui-v1.0.0", &[])).is_none());
    }

    #[test]
    fn latest_payload_rejects_an_incomplete_release() {
        let incomplete = release(
            "proa-cli-v0.1.0",
            &["proa-cli-0.1.0-aarch64-apple-darwin.tar.gz"],
        );
        assert!(latest_payload(&incomplete).is_none());
    }

    #[test]
    fn latest_response_is_cacheable_json() {
        let response = latest_response(&released("0.1.0"));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            JSON_CONTENT_TYPE
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=300"
        );
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap(),
            "nosniff"
        );
    }

    #[tokio::test]
    async fn latest_response_falls_back_to_404_for_an_unusable_release() {
        for release in [
            release("proa_ui-v1.0.0", &[]),
            release(
                "proa-cli-v0.1.0",
                &["proa-cli-0.1.0-aarch64-apple-darwin.tar.gz"],
            ),
        ] {
            let response = latest_response(&release);
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            let body: serde_json::Value = serde_json::from_str(&body_of(response).await).unwrap();
            assert_eq!(body["error"], "not_found");
        }
    }

    // -- asset whitelist ---------------------------------------------------

    #[test]
    fn asset_lookup_is_an_exact_whitelist_match() {
        let release = released("0.1.0");
        assert!(release
            .asset("proa-cli-0.1.0-x86_64-unknown-linux-gnu.tar.gz")
            .is_some());
        assert!(release.asset("proa-cli-0.1.0-SHA256SUMS").is_some());

        for filename in [
            "",
            "..",
            "../../etc/passwd",
            "/etc/passwd",
            "proa-cli-0.1.0-x86_64-unknown-linux-gnu.tar.gz/../../secret",
            "PROA-CLI-0.1.0-X86_64-UNKNOWN-LINUX-GNU.TAR.GZ",
            "proa-cli-0.1.0-x86_64-unknown-linux-gnu.tar",
            "proa-cli-0.1.0-x86_64-unknown-linux-gnu.tar.gz ",
            "proa-cli-0.2.0-x86_64-unknown-linux-gnu.tar.gz",
            "proa-cli-0.1.0-aarch64-apple-darwin.tar.gz",
            "proa-cli-0.1.0-SHA256SUMS.sig",
        ] {
            assert!(
                release.asset(filename).is_none(),
                "{filename:?} is not a published asset"
            );
        }
    }

    #[test]
    fn asset_ids_come_from_the_release_not_the_request() {
        let release = released("0.1.0");
        let asset = release.asset("proa-cli-0.1.0-SHA256SUMS").unwrap();
        assert_eq!(asset.name, "proa-cli-0.1.0-SHA256SUMS");
        assert_eq!(
            asset_api_url(GITHUB_API, "Proa-Labs/proa", asset.id),
            format!(
                "https://api.github.com/repos/Proa-Labs/proa/releases/assets/{}",
                asset.id
            )
        );
    }

    // -- URL construction --------------------------------------------------

    #[test]
    fn api_urls_target_the_configured_repository() {
        assert_eq!(
            releases_url(GITHUB_API, "Proa-Labs/proa"),
            "https://api.github.com/repos/Proa-Labs/proa/releases?per_page=100"
        );
        assert_eq!(
            release_by_tag_url(GITHUB_API, "Proa-Labs/proa", "proa-cli-v0.1.0"),
            "https://api.github.com/repos/Proa-Labs/proa/releases/tags/proa-cli-v0.1.0"
        );
        assert_eq!(
            asset_api_url(GITHUB_API, "forks/proa", 4242),
            "https://api.github.com/repos/forks/proa/releases/assets/4242"
        );
    }

    #[test]
    fn endpoints_default_to_github() {
        let endpoints = Endpoints::default();
        assert_eq!(endpoints.api, "https://api.github.com");
        assert_eq!(endpoints.web, "https://github.com");
        // Nothing reads an endpoint from the environment or from a request, so
        // no configuration and no client can retarget the bridge.
        let bridge = DownloadBridge::new(config_with_token(), None);
        assert_eq!(bridge.endpoints.api, GITHUB_API);
        assert_eq!(bridge.endpoints.web, GITHUB_WEB);
    }

    #[test]
    fn redirect_target_matches_the_published_release_url() {
        assert_eq!(
            browser_download_url(
                GITHUB_WEB,
                "Proa-Labs/proa",
                "proa-cli-v0.1.0",
                "proa-cli-0.1.0-x86_64-unknown-linux-gnu.tar.gz"
            ),
            "https://github.com/Proa-Labs/proa/releases/download/proa-cli-v0.1.0/\
             proa-cli-0.1.0-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    // -- configuration -----------------------------------------------------

    #[test]
    fn mode_defaults_to_proxy_only_when_a_token_is_present() {
        assert_eq!(
            resolve_mode(None, true),
            (DownloadMode::Proxy, ModeReason::DefaultProxy)
        );
        assert_eq!(
            resolve_mode(None, false),
            (DownloadMode::Redirect, ModeReason::DefaultRedirect)
        );
    }

    #[test]
    fn mode_honours_an_explicit_setting() {
        assert_eq!(
            resolve_mode(Some("proxy"), true),
            (DownloadMode::Proxy, ModeReason::Configured)
        );
        assert_eq!(
            resolve_mode(Some("redirect"), true),
            (DownloadMode::Redirect, ModeReason::Configured)
        );
        assert_eq!(
            resolve_mode(Some("  Redirect \n"), false),
            (DownloadMode::Redirect, ModeReason::Configured)
        );
        assert_eq!(
            resolve_mode(Some("PROXY"), true),
            (DownloadMode::Proxy, ModeReason::Configured)
        );
    }

    #[test]
    fn proxy_without_a_token_falls_back_to_redirect_rather_than_404ing() {
        assert_eq!(
            resolve_mode(Some("proxy"), false),
            (DownloadMode::Redirect, ModeReason::ProxyWithoutToken)
        );
    }

    #[test]
    fn an_unrecognized_mode_falls_back_to_the_default_for_the_token_state() {
        assert_eq!(
            resolve_mode(Some("stream"), true),
            (DownloadMode::Proxy, ModeReason::Unrecognized)
        );
        assert_eq!(
            resolve_mode(Some("stream"), false),
            (DownloadMode::Redirect, ModeReason::Unrecognized)
        );
        assert_eq!(
            resolve_mode(Some(""), true),
            (DownloadMode::Proxy, ModeReason::DefaultProxy),
            "an empty variable reads as unset"
        );
    }

    #[test]
    fn repository_override_must_be_an_owner_name_pair() {
        assert_eq!(
            parse_repo("Proa-Labs/proa").as_deref(),
            Some("Proa-Labs/proa")
        );
        assert_eq!(
            parse_repo("owner_1/repo.name-2").as_deref(),
            Some("owner_1/repo.name-2")
        );
        for value in [
            "",
            "proa",
            "/proa",
            "Proa-Labs/",
            "Proa-Labs/proa/extra",
            "../../etc",
            "Proa-Labs/../proa",
            "Proa-Labs/proa?x=1",
            "Proa Labs/proa",
            "Proa-Labs/proa#fragment",
            "..%2f..%2fetc/passwd",
        ] {
            assert!(parse_repo(value).is_none(), "{value:?} is not a repository");
        }
    }

    #[test]
    fn config_falls_back_to_the_default_repository() {
        let config = BridgeConfig::resolve(None, None, None);
        assert_eq!(config.repo, DEFAULT_REPO);
        assert!(!config.repo_override_rejected);

        let config = BridgeConfig::resolve(Some("   ".to_string()), None, None);
        assert_eq!(config.repo, DEFAULT_REPO);
        assert!(
            !config.repo_override_rejected,
            "a blank variable reads as unset, not as a typo"
        );

        let config = BridgeConfig::resolve(Some("not a repo".to_string()), None, None);
        assert_eq!(config.repo, DEFAULT_REPO);
        assert!(config.repo_override_rejected);
    }

    #[test]
    fn config_treats_a_blank_token_as_absent() {
        let config = BridgeConfig::resolve(None, Some("  \n".to_string()), None);
        assert!(config.token.is_none());
        assert_eq!(config.mode, DownloadMode::Redirect);

        let config = BridgeConfig::resolve(None, Some(format!("  {TOKEN}  ")), None);
        assert_eq!(config.token.as_deref(), Some(TOKEN));
        assert_eq!(config.mode, DownloadMode::Proxy);
    }

    // -- upstream classification ------------------------------------------

    fn headers_with(name: &'static str, value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_static(value));
        headers
    }

    #[test]
    fn rate_limit_exhaustion_is_detected_from_the_header() {
        assert!(rate_limit_exhausted(&headers_with(
            "x-ratelimit-remaining",
            "0"
        )));
        assert!(!rate_limit_exhausted(&headers_with(
            "x-ratelimit-remaining",
            "1"
        )));
        assert!(!rate_limit_exhausted(&HeaderMap::new()));
    }

    #[test]
    fn github_statuses_map_to_boring_outcomes() {
        let empty = HeaderMap::new();
        assert_eq!(classify_status(StatusCode::OK, &empty), UpstreamStatus::Ok);
        assert_eq!(
            classify_status(StatusCode::NOT_FOUND, &empty),
            UpstreamStatus::Missing
        );
        assert_eq!(
            classify_status(
                StatusCode::FORBIDDEN,
                &headers_with("x-ratelimit-remaining", "0")
            ),
            UpstreamStatus::RateLimited
        );
        assert_eq!(
            classify_status(
                StatusCode::TOO_MANY_REQUESTS,
                &headers_with("x-ratelimit-remaining", "0")
            ),
            UpstreamStatus::RateLimited
        );
        assert_eq!(
            classify_status(StatusCode::FORBIDDEN, &empty),
            UpstreamStatus::Failed,
            "a 403 without an exhausted quota is a credential problem"
        );
        assert_eq!(
            classify_status(StatusCode::UNAUTHORIZED, &empty),
            UpstreamStatus::Failed
        );
        for status in [
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::GATEWAY_TIMEOUT,
        ] {
            assert_eq!(classify_status(status, &empty), UpstreamStatus::Failed);
        }
    }

    // -- error responses ---------------------------------------------------

    #[tokio::test]
    async fn not_found_is_a_short_lived_json_404() {
        let response = error_response(BridgeError::NotFound);
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=60"
        );
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap(),
            "nosniff"
        );
        let body: serde_json::Value = serde_json::from_str(&body_of(response).await).unwrap();
        assert_eq!(body["error"], "not_found");
        assert!(body["message"].is_string());
    }

    #[tokio::test]
    async fn unavailable_is_a_never_cached_503() {
        let response = error_response(BridgeError::Unavailable);
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store",
            "a transient outage must not be cached for a year"
        );
        assert_eq!(response.headers().get(header::RETRY_AFTER).unwrap(), "30");
        let body: serde_json::Value = serde_json::from_str(&body_of(response).await).unwrap();
        assert_eq!(body["error"], "unavailable");
    }

    #[test]
    fn errors_are_never_a_500() {
        for error in [BridgeError::NotFound, BridgeError::Unavailable] {
            let status = error_response(error).status();
            assert!(
                matches!(
                    status,
                    StatusCode::NOT_FOUND | StatusCode::SERVICE_UNAVAILABLE
                ),
                "{error} produced {status}"
            );
        }
    }

    #[tokio::test]
    async fn no_response_path_can_leak_the_release_token() {
        let config = config_with_token();
        assert_eq!(config.token.as_deref(), Some(TOKEN));

        let mut rendered = vec![
            error_response(BridgeError::NotFound),
            error_response(BridgeError::Unavailable),
            latest_response(&released("0.1.0")),
            latest_response(&release("proa_ui-v1.0.0", &[])),
            redirect_response(&browser_download_url(
                GITHUB_WEB,
                &config.repo,
                "proa-cli-v0.1.0",
                "proa-cli-0.1.0-SHA256SUMS",
            )),
        ];

        // The offline bridge exercises the handler error paths end to end.
        let bridge = Arc::new(DownloadBridge::new(config, None));
        rendered.push(latest_handler(State(Arc::clone(&bridge))).await);
        rendered.push(
            asset_handler(
                State(Arc::clone(&bridge)),
                Path(("0.1.0".to_string(), "proa-cli-0.1.0-SHA256SUMS".to_string())),
            )
            .await,
        );

        for response in rendered {
            let headers = headers_as_text(&response);
            let body = body_of(response).await;
            assert!(!headers.contains(TOKEN), "token leaked into headers");
            assert!(!body.contains(TOKEN), "token leaked into a body");
            assert!(!headers.to_ascii_lowercase().contains("authorization"));
        }

        // The error types themselves carry no data, so nothing can hide there.
        assert!(!BridgeError::NotFound.to_string().contains(TOKEN));
        assert!(!BridgeError::Unavailable.to_string().contains(TOKEN));
    }

    // -- asset responses ---------------------------------------------------

    #[test]
    fn archive_headers_are_immutable_and_nosniff() {
        let headers = archive_headers();
        assert_eq!(headers[0].1, "application/octet-stream");
        assert_eq!(headers[1].1, "public, max-age=31536000, immutable");
        assert_eq!(headers[2].1, "nosniff");
    }

    #[tokio::test]
    async fn redirect_points_at_github_with_an_empty_body() {
        let response = redirect_response(
            "https://github.com/Proa-Labs/proa/releases/download/proa-cli-v0.1.0/proa-cli-0.1.0-SHA256SUMS",
        );
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            "https://github.com/Proa-Labs/proa/releases/download/proa-cli-v0.1.0/proa-cli-0.1.0-SHA256SUMS"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=31536000, immutable"
        );
        assert!(body_of(response).await.is_empty());
    }

    #[test]
    fn an_unrepresentable_redirect_target_degrades_to_404() {
        let response = redirect_response("https://github.com/bad\nheader");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(response.headers().get(header::LOCATION).is_none());
    }

    // -- cache -------------------------------------------------------------

    fn later(base: Instant, seconds: u64) -> Instant {
        base.checked_add(Duration::from_secs(seconds))
            .expect("instant arithmetic")
    }

    #[tokio::test]
    async fn cache_serves_a_release_inside_the_ttl() {
        let cache = ReleaseCache::default();
        let now = Instant::now();
        cache
            .store_at(
                "proa-cli-v0.1.0".to_string(),
                Some(Arc::new(released("0.1.0"))),
                now,
            )
            .await;

        match cache.get_at("proa-cli-v0.1.0", later(now, 299)).await {
            CacheLookup::Present(release) => assert_eq!(release.tag_name, "proa-cli-v0.1.0"),
            other => panic!("expected a hit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cache_expires_release_metadata_after_the_ttl() {
        let cache = ReleaseCache::default();
        let now = Instant::now();
        cache
            .store_at(
                "proa-cli-v0.1.0".to_string(),
                Some(Arc::new(released("0.1.0"))),
                now,
            )
            .await;
        assert!(matches!(
            cache.get_at("proa-cli-v0.1.0", later(now, 301)).await,
            CacheLookup::Miss
        ));
    }

    #[tokio::test]
    async fn cache_remembers_a_missing_release_but_forgets_it_sooner() {
        let cache = ReleaseCache::default();
        let now = Instant::now();
        cache
            .store_at("proa-cli-v9.9.9".to_string(), None, now)
            .await;

        assert!(
            matches!(
                cache.get_at("proa-cli-v9.9.9", later(now, 30)).await,
                CacheLookup::Absent
            ),
            "a known-missing release must not cost another API call"
        );
        assert!(
            matches!(
                cache.get_at("proa-cli-v9.9.9", later(now, 61)).await,
                CacheLookup::Miss
            ),
            "a release must become visible shortly after it publishes"
        );
    }

    #[test]
    fn release_lists_use_the_callers_ttl_except_while_publishing() {
        let now = Instant::now();
        let mut draft = released("0.1.0");
        draft.draft = true;
        let tag_entry = CacheEntry {
            release: Some(Arc::new(draft.clone())),
            stored_at: now,
        };
        assert!(entry_is_fresh(&tag_entry, later(now, 59)));
        assert!(!entry_is_fresh(&tag_entry, later(now, 61)));

        let draft_entry = ListEntry {
            releases: Arc::new(vec![draft]),
            truncated: false,
            stored_at: now,
        };
        assert!(list_is_fresh(&draft_entry, later(now, 59), RELEASE_TTL));
        assert!(!list_is_fresh(&draft_entry, later(now, 61), RELEASE_TTL));

        let published_entry = ListEntry {
            releases: Arc::new(vec![released("0.1.0")]),
            truncated: false,
            stored_at: now,
        };
        assert!(list_is_fresh(
            &published_entry,
            later(now, 299),
            RELEASE_TTL
        ));
        assert!(!list_is_fresh(
            &published_entry,
            later(now, 301),
            RELEASE_TTL
        ));
        assert!(list_is_fresh(
            &published_entry,
            later(now, 59),
            RELEASE_LIST_TTL
        ));
        assert!(!list_is_fresh(
            &published_entry,
            later(now, 61),
            RELEASE_LIST_TTL
        ));
    }

    #[tokio::test]
    async fn cache_keys_are_the_tag_so_one_version_cannot_serve_another() {
        let cache = ReleaseCache::default();
        let now = Instant::now();
        cache
            .store_at(
                "proa-cli-v0.2.0".to_string(),
                Some(Arc::new(released("0.2.0"))),
                now,
            )
            .await;
        cache
            .store_at(
                "proa-cli-v0.1.0".to_string(),
                Some(Arc::new(released("0.1.0"))),
                now,
            )
            .await;

        for version in ["0.1.0", "0.2.0"] {
            match cache.get_at(&tag_for_version(version), now).await {
                CacheLookup::Present(release) => {
                    assert_eq!(release.tag_name, tag_for_version(version));
                    // The asset names belong to the version that was asked for.
                    assert!(release
                        .asset(&format!("proa-cli-{version}-SHA256SUMS"))
                        .is_some());
                }
                other => panic!("expected the {version} entry, got {other:?}"),
            }
        }
        assert!(matches!(
            cache.get_at("proa-cli-v9.9.9", now).await,
            CacheLookup::Miss
        ));
    }

    #[tokio::test]
    async fn cache_stays_bounded_when_versions_are_probed() {
        let cache = ReleaseCache::default();
        let now = Instant::now();
        for index in 0..(MAX_CACHE_ENTRIES * 2) {
            cache
                .store_at(format!("proa-cli-v0.0.{index}"), None, now)
                .await;
        }
        assert!(cache.entries.read().await.len() <= MAX_CACHE_ENTRIES);
    }

    // -- routing -----------------------------------------------------------

    fn offline_router() -> Router {
        // No HTTP client, so these tests can never reach api.github.com.
        router_with(Arc::new(DownloadBridge::new(config_with_token(), None)))
    }

    async fn get_path(uri: &str) -> Response {
        offline_router()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn stable_and_main_routes_are_registered() {
        assert_ne!(
            get_path("/downloads/cli/latest.json").await.status(),
            StatusCode::NOT_FOUND
        );
        assert_ne!(
            get_path("/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS")
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_ne!(
            get_path("/downloads/cli/main/latest.json").await.status(),
            StatusCode::NOT_FOUND
        );
        let version = format!("0.1.1-main.{MAIN_SHA}");
        let uri = format!("/downloads/cli/main/{version}/proa-cli-{version}-SHA256SUMS");
        assert_ne!(get_path(&uri).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            get_path("/downloads/cli/main/0.1.0/proa-cli-0.1.0-SHA256SUMS")
                .await
                .status(),
            StatusCode::NOT_FOUND,
            "the main namespace must not serve stable release identities"
        );
    }

    #[tokio::test]
    async fn a_malformed_version_is_rejected_before_any_network_call() {
        // The bridge has no HTTP client, so anything that reached GitHub would
        // answer 503. A 404 proves validation ran first.
        for uri in [
            "/downloads/cli/latest/proa-cli.tar.gz",
            "/downloads/cli/1.2/proa-cli.tar.gz",
            "/downloads/cli/..%2F..%2Fetc/passwd",
            "/downloads/cli/0.1.0-/proa-cli.tar.gz",
        ] {
            let response = get_path(uri).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
            let body: serde_json::Value = serde_json::from_str(&body_of(response).await).unwrap();
            assert_eq!(body["error"], "not_found");
        }
    }

    #[tokio::test]
    async fn an_unreachable_upstream_is_a_503_not_a_500() {
        for uri in [
            "/downloads/cli/latest.json",
            "/downloads/cli/main/latest.json",
            "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS",
        ] {
            let response = get_path(uri).await;
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{uri}");
            assert_eq!(
                response.headers().get(header::CACHE_CONTROL).unwrap(),
                "no-store"
            );
        }
    }

    // -- adversarial: a live upstream under our control --------------------
    //
    // These drive the real handler against a local server standing in for
    // api.github.com, which is the only way to assert what the bridge sends
    // upstream and what it lets back out.

    use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// How the stand-in GitHub should answer. Every field is read under a lock
    /// so a test can look at what the bridge actually asked for.
    #[derive(Default)]
    struct Upstream {
        /// Path and query of every request that arrived.
        hits: Mutex<Vec<String>>,
        /// The Authorization header value seen on each request, if any.
        auth: Mutex<Vec<Option<String>>>,
        list_status: AtomicU16,
        list_body: Mutex<String>,
        tag_status: AtomicU16,
        tag_body: Mutex<String>,
        /// Bytes served per asset chunk, and how many chunks. `usize::MAX`
        /// chunks means "never stop", which is how a hostile upstream looks.
        asset_chunks: AtomicUsize,
    }

    impl Upstream {
        fn new() -> Arc<Self> {
            let upstream = Arc::new(Self::default());
            upstream.list_status.store(200, Ordering::SeqCst);
            upstream.tag_status.store(404, Ordering::SeqCst);
            *upstream.list_body.lock().unwrap() = "[]".to_string();
            *upstream.tag_body.lock().unwrap() = "{}".to_string();
            upstream.asset_chunks.store(1, Ordering::SeqCst);
            upstream
        }

        fn with_releases(self: &Arc<Self>, releases: &[Release]) -> &Arc<Self> {
            let json: Vec<serde_json::Value> = releases.iter().map(release_json).collect();
            *self.list_body.lock().unwrap() = serde_json::to_string(&json).unwrap();
            self
        }

        fn hits(&self) -> Vec<String> {
            self.hits.lock().unwrap().clone()
        }

        fn api_hits(&self) -> usize {
            self.hits.lock().unwrap().len()
        }

        fn saw_bearer(&self, token: &str) -> bool {
            self.auth
                .lock()
                .unwrap()
                .iter()
                .any(|value| value.as_deref() == Some(&format!("Bearer {token}")))
        }
    }

    /// `Release` only derives `Deserialize`, so the fixtures are turned back
    /// into the shape GitHub actually sends.
    fn release_json(release: &Release) -> serde_json::Value {
        serde_json::json!({
            "tag_name": release.tag_name,
            "draft": release.draft,
            "prerelease": release.prerelease,
            "published_at": release.published_at,
            "assets": release
                .assets
                .iter()
                .map(|asset| serde_json::json!({ "id": asset.id, "name": asset.name }))
                .collect::<Vec<_>>(),
        })
    }

    async fn record(upstream: &Arc<Upstream>, uri: &axum::http::Uri, headers: &HeaderMap) {
        upstream.hits.lock().unwrap().push(
            uri.path_and_query()
                .map(|pq| pq.to_string())
                .unwrap_or_default(),
        );
        upstream.auth.lock().unwrap().push(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
        );
    }

    /// Spawn the stand-in and return endpoints pointing at it.
    async fn spawn_upstream(upstream: Arc<Upstream>) -> Endpoints {
        use axum::extract::OriginalUri;

        let app = Router::new()
            .route(
                "/repos/:owner/:name/releases",
                get(
                    |State(up): State<Arc<Upstream>>,
                     OriginalUri(uri): OriginalUri,
                     headers: HeaderMap| async move {
                        record(&up, &uri, &headers).await;
                        let status =
                            StatusCode::from_u16(up.list_status.load(Ordering::SeqCst)).unwrap();
                        let body = up.list_body.lock().unwrap().clone();
                        (status, [(header::CONTENT_TYPE, "application/json")], body)
                    },
                ),
            )
            .route(
                "/repos/:owner/:name/releases/tags/:tag",
                get(
                    |State(up): State<Arc<Upstream>>,
                     OriginalUri(uri): OriginalUri,
                     headers: HeaderMap| async move {
                        record(&up, &uri, &headers).await;
                        let status =
                            StatusCode::from_u16(up.tag_status.load(Ordering::SeqCst)).unwrap();
                        let body = up.tag_body.lock().unwrap().clone();
                        (status, [(header::CONTENT_TYPE, "application/json")], body)
                    },
                ),
            )
            .route(
                "/repos/:owner/:name/releases/assets/:id",
                get(
                    |State(up): State<Arc<Upstream>>,
                     OriginalUri(uri): OriginalUri,
                     headers: HeaderMap| async move {
                        record(&up, &uri, &headers).await;
                        let chunks = up.asset_chunks.load(Ordering::SeqCst);
                        let stream = futures::stream::iter(
                            (0..chunks).map(|_| Ok::<_, std::io::Error>(vec![b'A'; 1024 * 1024])),
                        );
                        (
                            [(header::CONTENT_TYPE, "application/octet-stream")],
                            Body::from_stream(stream),
                        )
                    },
                ),
            )
            .with_state(upstream);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Endpoints {
            api: format!("http://127.0.0.1:{}", addr.port()),
            web: format!("http://127.0.0.1:{}", addr.port()),
        }
    }

    /// A bridge in proxy mode with a sentinel token, talking to the stand-in.
    async fn live_router(upstream: Arc<Upstream>) -> Router {
        let endpoints = spawn_upstream(upstream).await;
        router_with(Arc::new(
            DownloadBridge::new(config_with_token(), build_client()).with_endpoints(endpoints),
        ))
    }

    async fn live_get(router: &Router, uri: &str) -> Response {
        router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn simultaneous_metadata_misses_share_one_upstream_request() {
        let upstream = Upstream::new();
        upstream.with_releases(&[released("0.1.0")]);
        let router = live_router(Arc::clone(&upstream)).await;

        let responses = futures::future::join_all(
            (0..16).map(|_| live_get(&router, "/downloads/cli/latest.json")),
        )
        .await;

        assert!(responses
            .iter()
            .all(|response| response.status() == StatusCode::OK));
        assert_eq!(
            upstream.api_hits(),
            1,
            "concurrent cache misses must be coalesced: {:?}",
            upstream.hits()
        );
    }

    #[tokio::test]
    async fn the_bridge_authenticates_upstream_but_never_echoes_the_token() {
        let upstream = Upstream::new();
        upstream.with_releases(&[released("0.1.0")]);
        let router = live_router(Arc::clone(&upstream)).await;

        let response = live_get(&router, "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS").await;
        assert_eq!(response.status(), StatusCode::OK);
        let headers = headers_as_text(&response);
        let body = body_of(response).await;

        assert!(
            upstream.saw_bearer(TOKEN),
            "the bridge must authenticate to GitHub: {:?}",
            upstream.hits()
        );
        assert!(!headers.contains(TOKEN), "token leaked into headers");
        assert!(!body.contains(TOKEN), "token leaked into the body");
        assert!(!headers.to_ascii_lowercase().contains("authorization"));
    }

    #[tokio::test]
    async fn main_route_resolves_and_serves_the_same_immutable_snapshot() {
        let upstream = Upstream::new();
        upstream.with_releases(&[main_released("0.1.1", MAIN_SHA)]);
        let router = live_router(Arc::clone(&upstream)).await;

        let metadata = live_get(&router, "/downloads/cli/main/latest.json").await;
        assert_eq!(metadata.status(), StatusCode::OK);
        assert_eq!(
            metadata.headers().get(header::CACHE_CONTROL).unwrap(),
            MAIN_METADATA_CACHE_CONTROL
        );
        let body: serde_json::Value = serde_json::from_str(&body_of(metadata).await).unwrap();
        let version = format!("0.1.1-main.{MAIN_SHA}");
        assert_eq!(body["version"], version);
        assert_eq!(body["commit"], MAIN_SHA);

        let uri = format!("/downloads/cli/main/{version}/proa-cli-{version}-SHA256SUMS");
        assert_eq!(live_get(&router, &uri).await.status(), StatusCode::OK);
        assert_eq!(
            live_get(&router, "/downloads/cli/latest.json")
                .await
                .status(),
            StatusCode::NOT_FOUND,
            "the stable channel must not silently track main"
        );
    }

    #[tokio::test]
    async fn stable_route_rejects_ephemeral_main_snapshot_identities() {
        let upstream = Upstream::new();
        upstream.with_releases(&[main_released("0.1.1", MAIN_SHA)]);
        let router = live_router(Arc::clone(&upstream)).await;
        let version = format!("0.1.1-main.{MAIN_SHA}");
        let uri = format!("/downloads/cli/{version}/proa-cli-{version}-SHA256SUMS");

        assert_eq!(
            live_get(&router, &uri).await.status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            upstream.api_hits(),
            0,
            "channel validation must happen before an upstream request"
        );
    }

    #[tokio::test]
    async fn draft_release_assets_are_not_downloadable() {
        let upstream = Upstream::new();
        let mut draft = released("0.1.0");
        draft.draft = true;
        upstream.with_releases(&[draft]);
        let router = live_router(Arc::clone(&upstream)).await;

        let response = live_get(&router, "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(
            upstream.hits().iter().all(|hit| !hit.contains("/assets/")),
            "a draft must never reach the asset API: {:?}",
            upstream.hits()
        );
    }

    /// Every upstream failure mode, with a sentinel token in play. Nothing the
    /// upstream says and nothing the transport reports may reach the client.
    #[tokio::test]
    async fn no_upstream_failure_mode_can_leak_the_token() {
        // (list status, list body, description)
        let scenarios: Vec<(u16, String, &str)> = vec![
            (
                401,
                format!("{{\"message\":\"Bad credentials {TOKEN}\"}}"),
                "rejected token",
            ),
            (
                403,
                format!("{{\"message\":\"forbidden {TOKEN}\"}}"),
                "forbidden",
            ),
            (404, "{}".to_string(), "repository invisible"),
            (500, format!("oops {TOKEN}"), "upstream error"),
            (200, format!("not json at all {TOKEN}"), "undecodable body"),
            (
                200,
                format!("[{{\"tag_name\": \"{TOKEN}\"}}]"),
                "hostile payload",
            ),
        ];

        for (status, body, description) in scenarios {
            let upstream = Upstream::new();
            upstream.list_status.store(status, Ordering::SeqCst);
            *upstream.list_body.lock().unwrap() = body;
            let router = live_router(Arc::clone(&upstream)).await;

            for uri in [
                "/downloads/cli/latest.json",
                "/downloads/cli/main/latest.json",
                "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS",
            ] {
                let response = live_get(&router, uri).await;
                assert!(
                    matches!(
                        response.status(),
                        StatusCode::NOT_FOUND | StatusCode::SERVICE_UNAVAILABLE
                    ),
                    "{description} at {uri} produced {}",
                    response.status()
                );
                let headers = headers_as_text(&response);
                let body = body_of(response).await;
                assert!(
                    !headers.contains(TOKEN) && !body.contains(TOKEN),
                    "{description} at {uri} leaked the token: {headers}\n{body}"
                );
                // Nothing upstream said reaches the client either.
                assert!(
                    body == NOT_FOUND_BODY || body == UNAVAILABLE_BODY,
                    "{description} at {uri} forwarded an upstream body: {body}"
                );
            }
        }
    }

    /// Logs are the leak path a response-body test cannot see: a `warn!` that
    /// interpolates a `reqwest::Error` prints the URL it was built from, and a
    /// URL can carry credentials. `transport_reason` exists to prevent that,
    /// and this holds it in place.
    #[tokio::test]
    async fn no_log_line_can_leak_the_release_token() {
        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<Vec<u8>>>);

        impl std::io::Write for Capture {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
            type Writer = Self;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let capture = Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let config = BridgeConfig::resolve(None, Some(TOKEN.to_string()), None);
        config.log_startup();

        // Drive every logging path: startup, an upstream that rejects the
        // token, an undecodable body, an oversize body, and a dead upstream.
        for (status, body) in [
            (401, format!("{{\"message\":\"Bad credentials: {TOKEN}\"}}")),
            (500, format!("internal error for {TOKEN}")),
            (200, format!("<html>{TOKEN}</html>")),
            (404, "{}".to_string()),
            (200, format!("[{{\"tag_name\":\"{TOKEN}\"}}]")),
        ] {
            let upstream = Upstream::new();
            upstream.list_status.store(status, Ordering::SeqCst);
            *upstream.list_body.lock().unwrap() = body;
            let endpoints = spawn_upstream(Arc::clone(&upstream)).await;
            let bridge = Arc::new(
                DownloadBridge::new(config.clone(), build_client()).with_endpoints(endpoints),
            );
            let router = router_with(bridge);
            let _ = live_get(&router, "/downloads/cli/latest.json").await;
            let _ = live_get(&router, "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS").await;
        }

        // Both mode warnings, and a rejected repository override.
        BridgeConfig::resolve(None, None, Some("proxy".to_string())).log_startup();
        BridgeConfig::resolve(
            Some("not a repo".to_string()),
            Some(TOKEN.to_string()),
            Some("nonsense".to_string()),
        )
        .log_startup();

        let logged = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(!logged.is_empty(), "the bridge must log something at all");
        assert!(
            !logged.contains(TOKEN),
            "the release token reached a log line:\n{logged}"
        );
        // The URLs the bridge logs are printed verbatim, so the only thing
        // keeping a credential out of them is that none can be put in.
        assert!(
            logged.contains("url=\""),
            "the url field is what is at risk"
        );
    }

    /// The logged URLs above are safe only because a repository override cannot
    /// introduce URL userinfo. `https://user:token@api.github.com/...` would be
    /// logged in full by every warning in this module.
    #[test]
    fn a_repository_override_cannot_smuggle_credentials_into_a_url() {
        for value in [
            "user:token@api.evil.example/repo",
            "Proa-Labs@evil.example/proa",
            "Proa-Labs/proa@evil.example",
            "Proa-Labs/proa:8080",
            "Proa-Labs%40evil/proa",
            "//evil.example/proa",
            "https://user:pass@evil.example/x",
        ] {
            assert!(
                parse_repo(value).is_none(),
                "{value:?} must not become part of a URL"
            );
        }

        // Whatever does get through can only be the characters GitHub allows,
        // none of which have meaning in a URL authority.
        let repo = parse_repo("Owner_1/repo.name-2").expect("a valid repo");
        assert!(repo
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/')));
        let url = releases_url(GITHUB_API, &repo);
        assert!(url.starts_with("https://api.github.com/repos/"));
        assert!(!url.contains('@'));
    }

    /// The whole traversal matrix, end to end, against a real release list.
    #[tokio::test]
    async fn no_traversal_or_injection_reaches_an_asset() {
        let upstream = Upstream::new();
        upstream.with_releases(&[released("0.1.0")]);
        let router = live_router(Arc::clone(&upstream)).await;

        // Warm the list so the count below only reflects extra calls.
        assert_eq!(
            live_get(&router, "/downloads/cli/latest.json")
                .await
                .status(),
            StatusCode::OK
        );
        let baseline = upstream.api_hits();

        for uri in [
            // traversal in the filename, raw and encoded
            "/downloads/cli/0.1.0/../../../etc/passwd",
            "/downloads/cli/0.1.0/..%2f..%2fetc%2fpasswd",
            "/downloads/cli/0.1.0/%2e%2e%2f%2e%2e%2fetc%2fpasswd",
            "/downloads/cli/0.1.0/%252e%252e%252fetc%252fpasswd",
            // traversal in the version
            "/downloads/cli/..%2f..%2fetc/passwd",
            "/downloads/cli/0.1.0%2f..%2f..%2fother-tag/proa-cli-0.1.0-SHA256SUMS",
            "/downloads/cli/%2e%2e/proa-cli-0.1.0-SHA256SUMS",
            // absolute targets
            "/downloads/cli/0.1.0/https:%2f%2fevil.example%2fx",
            "/downloads/cli/0.1.0/%2fetc%2fpasswd",
            "/downloads/cli/https:%2f%2fevil.example/x",
            // null byte and control characters
            "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS%00.evil",
            "/downloads/cli/0.1.0%00/proa-cli-0.1.0-SHA256SUMS",
            "/downloads/cli/0.1.0/a%0d%0aX-Injected:%20yes",
            // unicode lookalikes for the digits
            "/downloads/cli/%EF%BC%90.1.0/proa-cli-0.1.0-SHA256SUMS",
            "/downloads/cli/0.1.0/proa%E2%80%90cli-0.1.0-SHA256SUMS",
            // near misses on a real asset name
            "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS%20",
            "/downloads/cli/0.1.0/PROA-CLI-0.1.0-SHA256SUMS",
            "/downloads/cli/0.1.0/proa-cli-0.2.0-SHA256SUMS",
            // a version that is not the one the asset belongs to
            "/downloads/cli/0.2.0/proa-cli-0.1.0-SHA256SUMS",
        ] {
            let response = live_get(&router, uri).await;
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "{uri} was not rejected"
            );
            assert!(response.headers().get(header::LOCATION).is_none(), "{uri}");
            let body = body_of(response).await;
            // An unencoded traversal adds path segments, so the router itself
            // refuses it before the handler and answers with an empty body.
            assert!(
                body.is_empty() || body == NOT_FOUND_BODY,
                "{uri} produced {body}"
            );
        }

        // A very long filename and a very long version are refused too.
        for uri in [
            format!("/downloads/cli/0.1.0/{}", "a".repeat(8000)),
            format!("/downloads/cli/{}.1.0/x", "9".repeat(8000)),
            format!("/downloads/cli/0.1.0-{}/x", "a".repeat(8000)),
        ] {
            assert_eq!(
                live_get(&router, &uri).await.status(),
                StatusCode::NOT_FOUND,
                "an oversized parameter must be refused"
            );
        }

        assert_eq!(
            upstream.api_hits(),
            baseline,
            "no rejected request may reach the upstream: {:?}",
            upstream.hits()
        );
    }

    /// The bug this guards: walking the version space costs one GitHub API call
    /// per request, and the 5000/hour quota takes every download offline.
    #[tokio::test]
    async fn walking_the_version_space_does_not_spend_the_github_quota() {
        let upstream = Upstream::new();
        upstream.with_releases(&[released("0.1.0")]);
        let router = live_router(Arc::clone(&upstream)).await;

        // Every version except the one that exists.
        for major in 1..41 {
            for minor in 0..10 {
                let uri = format!("/downloads/cli/{major}.{minor}.0/proa-cli-0.1.0-SHA256SUMS");
                assert_eq!(
                    live_get(&router, &uri).await.status(),
                    StatusCode::NOT_FOUND,
                    "{uri}"
                );
            }
        }

        assert_eq!(
            upstream.api_hits(),
            1,
            "400 probed versions must cost one cached list call, not 400: {:?}",
            upstream.hits()
        );
    }

    /// When the list page is full, older versions still resolve, but the
    /// per-tag lookups that enables stay bounded.
    #[tokio::test]
    async fn a_truncated_list_still_resolves_old_versions_but_stays_bounded() {
        let upstream = Upstream::new();
        let mut releases: Vec<Release> = (0..RELEASES_PAGE_SIZE)
            .map(|index| release(&format!("proa_ui-v1.0.{index}"), &["x.tar.gz"]))
            .collect();
        releases.push(released("0.9.0"));
        upstream.with_releases(&releases);
        *upstream.tag_body.lock().unwrap() =
            serde_json::to_string(&release_json(&released("0.1.0"))).unwrap();
        upstream.tag_status.store(200, Ordering::SeqCst);
        let router = live_router(Arc::clone(&upstream)).await;

        // 0.1.0 is not in the (full) list, so the per-tag lookup runs.
        let response = live_get(&router, "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS").await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "an old version must still be downloadable"
        );

        // Probing distinct old versions cannot run away with the quota.
        upstream.tag_status.store(404, Ordering::SeqCst);
        for index in 0..(MAX_CACHE_ENTRIES * 3) {
            let uri = format!("/downloads/cli/7.{index}.0/proa-cli-0.1.0-SHA256SUMS");
            let status = live_get(&router, &uri).await.status();
            assert!(
                matches!(
                    status,
                    StatusCode::NOT_FOUND | StatusCode::SERVICE_UNAVAILABLE
                ),
                "{uri} produced {status}"
            );
        }

        let tag_lookups = upstream
            .hits()
            .iter()
            .filter(|hit| hit.contains("/tags/"))
            .count();
        assert!(
            tag_lookups <= MAX_CACHE_ENTRIES + 1,
            "{tag_lookups} tag lookups for {} probes is unbounded",
            MAX_CACHE_ENTRIES * 3
        );
    }

    /// A hostile upstream must cost a 503, not the process.
    #[tokio::test]
    async fn oversize_metadata_is_refused_rather_than_buffered() {
        let upstream = Upstream::new();
        // Well past MAX_METADATA_BYTES, and valid JSON so only the cap can
        // reject it.
        let filler = "x".repeat(MAX_METADATA_BYTES + 1024);
        *upstream.list_body.lock().unwrap() = format!("[{{\"tag_name\": \"{filler}\"}}]");
        let router = live_router(Arc::clone(&upstream)).await;

        let response = live_get(&router, "/downloads/cli/latest.json").await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store",
            "a transient upstream problem must not be cached"
        );
        assert_eq!(body_of(response).await, UNAVAILABLE_BODY);
    }

    /// If `proxy_asset` buffered, this would never return: the upstream sends
    /// 4GB and never stops.
    #[tokio::test]
    async fn a_multi_gigabyte_asset_is_streamed_not_collected() {
        let upstream = Upstream::new();
        upstream.with_releases(&[released("0.1.0")]);
        upstream.asset_chunks.store(4096, Ordering::SeqCst);
        let router = live_router(Arc::clone(&upstream)).await;

        let response = tokio::time::timeout(
            Duration::from_secs(20),
            live_get(&router, "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS"),
        )
        .await
        .expect("headers must arrive without collecting 4GB first");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/octet-stream"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=31536000, immutable"
        );

        // Read only the first chunk and drop the rest, which is what a client
        // that cancels a download does.
        let mut body = response.into_body().into_data_stream();
        let first = body.next().await.expect("a first chunk").unwrap();
        assert!(!first.is_empty());
        drop(body);
    }

    /// Only the length is taken from upstream; nothing else it sends is
    /// forwarded, so it cannot set cookies or a permissive CORS header on our
    /// origin.
    #[tokio::test]
    async fn upstream_response_headers_are_not_forwarded() {
        let upstream = Upstream::new();
        upstream.with_releases(&[released("0.1.0")]);
        let router = live_router(Arc::clone(&upstream)).await;

        let response = live_get(&router, "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS").await;
        assert_eq!(response.status(), StatusCode::OK);
        let names: Vec<String> = response
            .headers()
            .keys()
            .map(|name| name.as_str().to_string())
            .collect();
        for forbidden in ["set-cookie", "access-control-allow-origin", "server"] {
            assert!(
                !names.contains(&forbidden.to_string()),
                "{forbidden} forwarded"
            );
        }
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap(),
            "nosniff"
        );
    }

    /// Redirect mode must land on github.com even when the release names an
    /// asset that tries to walk out of the release path.
    #[tokio::test]
    async fn redirect_mode_cannot_be_walked_off_github() {
        let upstream = Upstream::new();
        upstream.with_releases(&[released("0.1.0")]);
        let endpoints = spawn_upstream(Arc::clone(&upstream)).await;
        let router = router_with(Arc::new(
            DownloadBridge::new(
                BridgeConfig::resolve(None, Some(TOKEN.to_string()), Some("redirect".to_string())),
                build_client(),
            )
            .with_endpoints(Endpoints {
                api: endpoints.api,
                // Keep the real web base so the assertion is about the URL we
                // would actually send a client to.
                web: GITHUB_WEB.to_string(),
            }),
        ));

        let response = live_get(&router, "/downloads/cli/0.1.0/proa-cli-0.1.0-SHA256SUMS").await;
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            location,
            "https://github.com/Proa-Labs/proa/releases/download/proa-cli-v0.1.0/\
             proa-cli-0.1.0-SHA256SUMS"
        );
        assert!(!headers_as_text(&response).contains(TOKEN));
    }

    #[tokio::test]
    async fn nothing_else_under_downloads_is_routed() {
        for uri in [
            "/downloads/cli",
            "/downloads/cli/0.1.0",
            "/downloads/cli/0.1.0/nested/path.tar.gz",
            "/downloads",
        ] {
            assert_eq!(
                get_path(uri).await.status(),
                StatusCode::NOT_FOUND,
                "{uri} must not be routed"
            );
        }
    }
}
