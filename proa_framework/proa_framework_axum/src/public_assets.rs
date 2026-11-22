// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Startup resolution and serving for the build tool's public-file catalog.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

use axum::{
    body::Body,
    extract::{Path as RequestPath, State},
    http::{header, HeaderValue, Method, Request, Response, StatusCode},
    routing::get,
    Router,
};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use proa_assets::{
    AssetCatalog, AssetError, LiveReader, IMMUTABLE_DIRECTORY, MANIFEST_FILE, MUTABLE_DIRECTORY,
    OUTPUT_ENV, PUBLIC_ROUTE,
};
use proa_core::{BasePath, PublicAssetUrls};
use thiserror::Error;
use tower::ServiceExt;
use tower_http::services::ServeFile;

const PATH_ENCODING: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');
/// A manifest whose modification time is this close to the moment it was
/// read can be replaced again without its size or timestamp changing, because
/// file systems record times as coarsely as two seconds. Inside the window the
/// manifest bytes decide; outside it the metadata alone is exact.
const RACY_MANIFEST_WINDOW: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
pub enum PublicAssetError {
    #[error(transparent)]
    Catalog(#[from] AssetError),
    #[error("could not prepare public asset `{path}`: {reason}")]
    Preparation { path: String, reason: String },
}

/// Load once at startup and attach to the framework builder to serve files
/// and resolve `cx.url("public/...")` in templates.
///
/// The router serves only active manifest entries, each at its one URL:
/// content-hashed files under `/_proa/immutable`, files that keep their name
/// under `/_proa/mutable`.
/// A base path changes outgoing URLs for a prefix-stripping proxy; incoming
/// routes stay rooted at `/`, matching `FrameworkBuilder::with_base_path`.
///
/// A value is a handle to one catalog generation. In production there is only
/// ever one. Under `proa dev` the handle from [`from_env`](Self::from_env) or
/// [`load_live`](Self::load_live) also follows in-place republication: its
/// router, [`render_urls`](Self::render_urls), and [`current`](Self::current)
/// always answer from the newest catalog. [`url`](Self::url) and
/// [`cache_key`](Self::cache_key) borrow from the generation the handle holds,
/// so read them from a per-request handle: the `Extension<PublicAssets>` that
/// `RouteContextLayer` installs, or the result of [`current`](Self::current).
#[derive(Clone)]
pub struct PublicAssets {
    catalog: Arc<PreparedCatalog>,
    live: Option<Arc<LiveCatalog>>,
}

/// What was true of `manifest.json` when a catalog was loaded from it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ManifestStamp {
    modified: SystemTime,
    len: u64,
}

impl ManifestStamp {
    fn read(manifest: &Path) -> Option<Self> {
        let meta = std::fs::metadata(manifest).ok()?;
        Some(Self {
            modified: meta.modified().ok()?,
            len: meta.len(),
        })
    }

    /// Whether a later replacement is guaranteed to change this stamp.
    fn settled(self) -> bool {
        self.modified
            .elapsed()
            .is_ok_and(|age| age > RACY_MANIFEST_WINDOW)
    }
}

struct LiveState {
    catalog: Arc<PreparedCatalog>,
    /// Stamp of a manifest that has been evaluated and was old enough when
    /// read for the stamp alone to prove later requests see the same file.
    settled: Option<ManifestStamp>,
    /// Manifest generation that failed to load. It is not retried until the
    /// manifest changes again, so a broken output costs one attempt.
    rejected: Option<String>,
}

/// Follows atomic manifest replacement for the life of a dev server.
struct LiveCatalog {
    output: PathBuf,
    manifest: PathBuf,
    base_path: BasePath,
    state: RwLock<LiveState>,
    /// Tells `proa dev` that publishing in place is enough for this server.
    _registration: LiveReader,
}

impl LiveCatalog {
    fn current(&self) -> Arc<PreparedCatalog> {
        let stamp = ManifestStamp::read(&self.manifest);
        {
            let state = self.state.read().unwrap_or_else(|e| e.into_inner());
            if stamp.is_some() && stamp == state.settled {
                return state.catalog.clone();
            }
        }
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        // Stamp before bytes: a replacement that lands in between leaves a
        // stamp the next request no longer matches, so it is never missed.
        let stamp = ManifestStamp::read(&self.manifest).filter(|stamp| stamp.settled());
        if stamp.is_some() && stamp == state.settled {
            return state.catalog.clone();
        }
        // The stamp is new, missing, or too fresh to trust. Content decides.
        let Ok(bytes) = std::fs::read(&self.manifest) else {
            // `proa dev` restarts the server when the output disappears.
            return state.catalog.clone();
        };
        let generation = proa_assets::sha256(&bytes);
        if generation != state.catalog.catalog.generation()
            && state.rejected.as_deref() != Some(generation.as_str())
        {
            match AssetCatalog::load(&self.output)
                .map_err(PublicAssetError::from)
                .and_then(|catalog| PreparedCatalog::from_catalog(catalog, &self.base_path))
            {
                Ok(catalog) => {
                    tracing::debug!("public asset catalog reloaded in place");
                    state.catalog = Arc::new(catalog);
                    state.rejected = None;
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "could not reload the public asset catalog; serving the previous one"
                    );
                    state.rejected = Some(generation);
                }
            }
        }
        // These bytes are now loaded, known unchanged, or known broken. A
        // settled stamp identifies them until the manifest is replaced; a
        // fresh one proves nothing, so the next request compares bytes again.
        state.settled = stamp;
        state.catalog.clone()
    }
}

struct PreparedCatalog {
    /// Also holds the reader lock that keeps these objects on disk.
    catalog: AssetCatalog,
    base_path: BasePath,
    urls: PublicAssetUrls,
    routes: BTreeMap<String, PreparedFile>,
    /// Files also answered at a fixed path from the origin root, by that
    /// path with its leading slash.
    root: BTreeMap<String, PreparedFile>,
    cache_key: String,
}

#[derive(Clone)]
struct PreparedFile {
    service: ServeFile,
    etag: HeaderValue,
    content_type: HeaderValue,
    cache_control: HeaderValue,
}

/// The bundled catalog of the running executable, when one was published.
///
/// Symlinks are resolved first, so a link in a `bin` directory still finds
/// the catalog beside the real file. A directory there that carries Proa's
/// ownership marker is the bundle, so one that lost its manifest fails loading
/// instead of starting a server without its assets. Anything else at that
/// path, such as an application's own data directory, is not a catalog.
fn bundled_with_current_executable() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let executable = executable.canonicalize().unwrap_or(executable);
    bundled_catalog(&executable)
}

fn bundled_catalog(executable: &Path) -> Option<PathBuf> {
    let output = proa_assets::bundled_output(executable)?;
    proa_assets::is_output(&output).then_some(output)
}

impl PublicAssets {
    /// An explicitly configured missing/corrupt output fails startup.
    pub fn load(output: impl AsRef<Path>, base_path: &BasePath) -> Result<Self, PublicAssetError> {
        let prepared = PreparedCatalog::from_catalog(AssetCatalog::load(output)?, base_path)?;
        Ok(Self {
            catalog: Arc::new(prepared),
            live: None,
        })
    }

    /// Load a catalog that follows `proa dev` republishing it in place, so an
    /// edited public file is served without restarting the server.
    ///
    /// The returned handle registers itself with the output directory. That
    /// registration is what lets `proa dev` skip the restart, so an output
    /// prepared by a CLI without in-place publication yields a fixed catalog,
    /// exactly like [`load`](Self::load).
    pub fn load_live(
        output: impl AsRef<Path>,
        base_path: &BasePath,
    ) -> Result<Self, PublicAssetError> {
        let output = output.as_ref();
        let fixed = Self::load(output, base_path)?;
        // Register only after a catalog exists: from here on a publication
        // may arrive at any moment and `current` has to be able to answer.
        let Ok(registration) = LiveReader::register(output) else {
            return Ok(fixed);
        };
        Ok(Self {
            live: Some(Arc::new(LiveCatalog {
                manifest: output.join(MANIFEST_FILE),
                output: output.to_path_buf(),
                base_path: base_path.clone(),
                state: RwLock::new(LiveState {
                    catalog: fixed.catalog.clone(),
                    settled: None,
                    rejected: None,
                }),
                _registration: registration,
            })),
            catalog: fixed.catalog,
        })
    }

    /// Whether this process has a managed catalog to load: one named by
    /// `PROA_PUBLIC_ASSETS`, or one bundled beside the executable. A router
    /// uses this to mount its own static fallback only when there is none.
    pub fn configured() -> bool {
        std::env::var_os(OUTPUT_ENV).is_some() || bundled_with_current_executable().is_some()
    }

    /// The catalog this process was built or started with.
    ///
    /// `PROA_PUBLIC_ASSETS` names it when set, which is how `proa dev`,
    /// `proa run`, and `proa test` supply the one they just published.
    /// Otherwise it is the catalog `proa build` bundled beside this
    /// executable (`app.assets` next to `app`), so an exported build runs
    /// from wherever the pair was copied with no configuration. `None` means
    /// neither exists: the application has no managed public directory.
    ///
    /// A process started by `proa dev` with live reload gets a catalog that
    /// follows in-place republication. That takes the same two variables as
    /// the reload client, so a stray `PROA_DEV_RELOAD_PORT` in a deployment
    /// environment cannot switch it on.
    pub fn from_env(base_path: &BasePath) -> Result<Option<Self>, PublicAssetError> {
        let Some(path) = std::env::var_os(OUTPUT_ENV)
            .map(PathBuf::from)
            .or_else(bundled_with_current_executable)
        else {
            return Ok(None);
        };
        if crate::dev_reload::session_from_env() {
            Self::load_live(path, base_path).map(Some)
        } else {
            Self::load(path, base_path).map(Some)
        }
    }

    /// A handle fixed to the newest catalog.
    ///
    /// Call this once per request when rendering outside `RouteContextLayer`
    /// and read URLs and the cache key from the result, so one response never
    /// mixes two generations. Outside `proa dev` it returns the same catalog.
    pub fn current(&self) -> Self {
        match &self.live {
            Some(live) => Self {
                catalog: live.current(),
                live: None,
            },
            None => self.clone(),
        }
    }

    /// Look up one logical path in the catalog this handle holds.
    pub fn url(&self, logical: &str) -> Option<&str> {
        self.catalog.urls.get(logical)
    }

    /// Share the prepared URL table with a manually constructed render context.
    /// A handle that follows `proa dev` answers from the newest catalog.
    pub fn render_urls(&self) -> PublicAssetUrls {
        match &self.live {
            Some(live) => live.current().urls.clone(),
            None => self.catalog.urls.clone(),
        }
    }

    /// The catalog this handle was loaded from.
    pub(crate) fn catalog(&self) -> &AssetCatalog {
        &self.catalog.catalog
    }

    pub(crate) fn base_path(&self) -> &BasePath {
        &self.catalog.base_path
    }

    /// Include this in application caches that hold HTML containing asset URLs.
    /// It incorporates both catalog bytes and the startup delivery base path.
    ///
    /// The key belongs to the catalog this handle holds. Under `proa dev`,
    /// read it from the request's `Extension<PublicAssets>` or from
    /// [`current`](Self::current) so it changes when files are republished.
    pub fn cache_key(&self) -> &str {
        &self.catalog.cache_key
    }

    /// Answer the fixed paths browsers request on their own, such as
    /// `/favicon.ico`, from the public files of those names. A route of the
    /// application's own at one of those paths still wins: a root file is
    /// served only where the router would have answered 404.
    ///
    /// `FrameworkBuilder` applies this. A custom router wraps itself.
    pub fn serve_root_files(&self, router: Router) -> Router {
        let assets = self.clone();
        router.layer(axum::middleware::from_fn(
            move |request: Request<Body>, next: axum::middleware::Next| {
                let assets = assets.clone();
                async move {
                    let assets = assets.current();
                    let wanted = matches!(*request.method(), Method::GET | Method::HEAD)
                        && assets.catalog.root.contains_key(request.uri().path());
                    if !wanted {
                        return next.run(request).await;
                    }
                    let (parts, body) = request.into_parts();
                    let response = next.run(Request::from_parts(parts.clone(), body)).await;
                    if response.status() != StatusCode::NOT_FOUND {
                        return response;
                    }
                    let file = &assets.catalog.root[parts.uri.path()];
                    respond(file, Request::from_parts(parts, Body::empty())).await
                }
            },
        ))
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route(
                &format!("{PUBLIC_ROUTE}/{IMMUTABLE_DIRECTORY}/*path"),
                get(serve_immutable),
            )
            .route(
                &format!("{PUBLIC_ROUTE}/{MUTABLE_DIRECTORY}/*path"),
                get(serve_mutable),
            )
            .with_state(self.clone())
    }
}

impl PreparedCatalog {
    fn from_catalog(catalog: AssetCatalog, base_path: &BasePath) -> Result<Self, PublicAssetError> {
        let mut urls = BTreeMap::new();
        let mut routes = BTreeMap::new();
        for (logical, entry) in &catalog.manifest().assets {
            let fail = |reason: String| PublicAssetError::Preparation {
                path: logical.clone(),
                reason,
            };
            let mime: mime::Mime = entry
                .content_type
                .parse()
                .map_err(|e: mime::FromStrError| fail(e.to_string()))?;
            let path = catalog.object_path(logical).expect("manifest entry");
            let file = PreparedFile {
                // The file on disk is the response, whatever the base path:
                // one build output serves every environment and any host.
                service: ServeFile::new_with_mime(path, &mime),
                etag: format!("\"{}\"", entry.sha256)
                    .parse()
                    .expect("SHA-256 header"),
                content_type: entry
                    .content_type
                    .parse()
                    .map_err(|e: axum::http::header::InvalidHeaderValue| fail(e.to_string()))?,
                cache_control: HeaderValue::from_static(entry.cache_policy.cache_control()),
            };
            // Manifest validation rejects different bytes claiming one path.
            let url_path = entry.url_path();
            let encoded = utf8_percent_encode(&url_path, PATH_ENCODING);
            urls.insert(
                logical.clone(),
                Arc::from(format!("{}{PUBLIC_ROUTE}/{encoded}", base_path.as_str())),
            );
            routes.insert(url_path, file);
        }
        let mut root = BTreeMap::new();
        for (path, logical) in &catalog.manifest().root {
            let entry = &catalog.manifest().assets[logical];
            let mut file = routes[&entry.url_path()].clone();
            // The path stays the same when the bytes change.
            file.cache_control =
                HeaderValue::from_static(proa_assets::CachePolicy::Revalidate.cache_control());
            root.insert(format!("/{path}"), file);
        }
        let cache_key = proa_assets::sha256(
            format!("{}\n{}", catalog.generation(), base_path.as_str()).as_bytes(),
        );
        Ok(Self {
            catalog,
            base_path: base_path.clone(),
            urls: PublicAssetUrls::new(urls),
            routes,
            root,
            cache_key,
        })
    }
}

async fn serve_immutable(
    assets: State<PublicAssets>,
    RequestPath(path): RequestPath<String>,
    request: Request<Body>,
) -> Response<Body> {
    serve(assets, IMMUTABLE_DIRECTORY, &path, request).await
}

async fn serve_mutable(
    assets: State<PublicAssets>,
    RequestPath(path): RequestPath<String>,
    request: Request<Body>,
) -> Response<Body> {
    serve(assets, MUTABLE_DIRECTORY, &path, request).await
}

async fn serve(
    State(assets): State<PublicAssets>,
    directory: &str,
    path: &str,
    request: Request<Body>,
) -> Response<Body> {
    let assets = assets.current();
    match assets.catalog.routes.get(&format!("{directory}/{path}")) {
        Some(file) => respond(file, request).await,
        None => empty(StatusCode::NOT_FOUND),
    }
}

async fn respond(file: &PreparedFile, mut request: Request<Body>) -> Response<Body> {
    let matches = request
        .headers()
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .any(|value| {
            value.to_str().is_ok_and(|value| {
                value.split(',').any(|tag| {
                    let tag = tag.trim();
                    tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == file.etag
                })
            })
        });
    let mut response = if matches {
        empty(StatusCode::NOT_MODIFIED)
    } else {
        // Digest validators are authoritative, including edits that preserve
        // mtime. ServeFile still owns streaming, HEAD, and byte-range parsing.
        request.headers_mut().remove(header::IF_NONE_MATCH);
        request.headers_mut().remove(header::IF_MODIFIED_SINCE);
        if let Some(if_range) = request.headers_mut().remove(header::IF_RANGE) {
            if if_range != file.etag {
                request.headers_mut().remove(header::RANGE);
            }
        }
        file.service
            .clone()
            .oneshot(request)
            .await
            .unwrap()
            .map(Body::new)
    };
    if response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED {
        let headers = response.headers_mut();
        headers.insert(header::ETAG, file.etag.clone());
        headers.insert(header::CONTENT_TYPE, file.content_type.clone());
        headers.insert(header::CACHE_CONTROL, file.cache_control.clone());
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        headers.remove(header::LAST_MODIFIED);
    }
    response
}

fn empty(status: StatusCode) -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests;
