// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

mod asset_urls;
pub mod bootstrap;
pub mod config;
pub mod csp;
pub mod csrf;
pub mod dev_reload;
pub mod external;
pub mod manifest;
pub mod process_manager;
pub mod proxy;
pub mod readiness;
pub mod route;
#[cfg(feature = "route-export")]
pub mod route_export;
pub mod rsjs_assets;
pub mod security_headers;
pub mod server;
pub mod shard;
pub mod static_service;
pub use asset_urls::base_path_assets;

pub use bootstrap::render_bootstrap_for_islands;
pub use config::{CodegenConfig, PathsConfig, ProaConfig, ServerConfig};
pub use csp::{CspNonceLayer, CspPolicy};
pub use csrf::CrossOriginProtectionLayer;
pub use dev_reload::{DevReloadLayer, RELOAD_PORT_ENV, RELOAD_TOKEN_ENV};
pub use external::{ExternalApp, ExternalAppStrategy};
pub use manifest::{
    CssAsset, HydrationStrategyConfig, IslandEntry, IslandManifest, ManifestAssets, ProviderSpec,
};
pub use proa_core::{AppUrl, BasePath, CspNonce, InvalidBasePath};
pub use readiness::{Readiness, DEFAULT_READINESS_TIMEOUT};
pub use route::{
    app_url, css_asset_path, drive_dyn_render, drive_dyn_send_render, explicit_markdown,
    explicit_markdown_async_with_loader, local_metadata, metadata_from_context, negotiate,
    negotiate_async_with_loader, render_css_link, render_metadata_head, render_metadata_head_send,
    resolved_metadata, router_from_spec, CascadeOpenGraph, CascadeValue, ComputedRouteMeta,
    DynMetadata, DynRenderOutcome, DynSendRenderOutcome, DynWebRender, DynWebRenderSend, Layout,
    LayoutRef, LocalMetadata, LocalMetadataProvider, LocalStreamExecutorConfig, MarkdownAlternate,
    Metadata, MetadataBase, MetadataContext, MetadataError, MetadataPatch, MetadataRef,
    MetadataResult, MetadataText, OpenGraph, OpenGraphImage, OpenGraphMedia, OpenGraphPatch,
    PageCacheLayer, PageCacheService, RepresentationResponse, ResolvedMetadata,
    ResolvedOpenGraphImage, ResolvedOpenGraphMedia, Route, RouteContextLayer, RouteMode,
    RouteResponse, RouteSpec, StaticPathsFn,
};
pub use rsjs_assets::rsjs_assets_router;
pub use security_headers::{FrameOptions, SecurityHeadersConfig, SecurityHeadersLayer};
pub use server::{serve, serve_with_shutdown, ShutdownOutcome, DEFAULT_GRACEFUL_SHUTDOWN_TIMEOUT};
pub use shard::{ShardCache, ShardCacheConfig};
pub use static_service::EmbeddedStaticService;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{header, HeaderValue};
use axum::{extract::DefaultBodyLimit, extract::Extension, Router};
use tower::limit::GlobalConcurrencyLimitLayer;
use tower::Layer as _;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;

/// Default request body size limit applied by `FrameworkBuilder::build`.
///
/// 1 MiB. Override per builder via `with_body_limit(usize)`. Override
/// per route via Axum's `DefaultBodyLimit::max(...)` layer applied to a
/// nested router. Prior versions shipped with no global body limit, so a
/// single POST with a multi-GB body could pin a worker indefinitely.
pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// Default request timeout applied by `FrameworkBuilder::build`.
///
/// The timeout covers producing the response (routing, middleware,
/// rendering) but not draining the response body, so long-lived
/// streaming SSR responses are unaffected once their first bytes are
/// out. Override via `with_request_timeout(Duration)`; disable via
/// `without_request_timeout()`.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Builder for the framework's axum `Router`.
///
/// Security defaults applied by [`build`](Self::build):
///
/// - Cross-origin protection (CSRF defense) is **on** for unsafe methods
///   (everything except GET / HEAD / OPTIONS). Browser-issued
///   cross-origin POSTs are rejected with 403 based on `Sec-Fetch-Site`
///   / `Origin`; non-browser clients are unaffected. Disable with
///   [`with_cross_origin_protection(false)`](Self::with_cross_origin_protection)
///   or allow specific callers with
///   [`with_trusted_origin`](Self::with_trusted_origin).
/// - Security response headers (`X-Content-Type-Options: nosniff`,
///   `X-Frame-Options: DENY`, `Referrer-Policy:
///   strict-origin-when-cross-origin`) are stamped on every response
///   that does not already set them; tune via
///   [`with_security_headers`](Self::with_security_headers).
/// - A 30 second request timeout
///   ([`DEFAULT_REQUEST_TIMEOUT`]) bounds response production.
/// - A global request body limit of 1 MiB
///   ([`DEFAULT_BODY_LIMIT_BYTES`]).
///
/// Opt-in hardening: [`with_strict_csp`](Self::with_strict_csp) (nonce-based
/// Content Security Policy), [`with_concurrency_limit`](Self::with_concurrency_limit),
/// and [`with_catch_panic`](Self::with_catch_panic).
pub struct FrameworkBuilder {
    manifest: Arc<IslandManifest>,
    base_path: BasePath,
    metadata_base: Option<MetadataBase>,
    rsjs_assets: bool,
    config: ProaConfig,
    static_router: Option<Router>,
    dynamic_router: Option<Router>,
    computed_metadata: Option<Arc<HashMap<String, ComputedRouteMeta>>>,
    external_apps: Vec<ExternalApp>,
    process_manager: process_manager::ProcessManager,
    /// Global request-body size limit applied via `DefaultBodyLimit`
    /// in `build`. `None` means use `DEFAULT_BODY_LIMIT_BYTES`. Set to
    /// `Some(usize::MAX)` only when a specific deployment knows it
    /// must accept very large request bodies (file uploads in a
    /// closed admin path); generally prefer a per-route override on a
    /// nested router instead of removing the global cap.
    body_limit: Option<usize>,
    /// Cross-origin protection (CSRF defense). ON by default.
    cross_origin_protection: bool,
    /// Extra origins trusted by the cross-origin protection layer.
    trusted_origins: Vec<String>,
    /// Security response header policy applied to every response.
    security_headers: SecurityHeadersConfig,
    /// Per-request CSP nonce plus policy. OFF by default: a strict policy
    /// has to match what the app's own markup loads.
    csp: Option<CspPolicy>,
    /// Request timeout. `None` disables the timeout layer entirely.
    request_timeout: Option<Duration>,
    /// Optional global in-flight request cap. OFF by default.
    concurrency_limit: Option<usize>,
    /// Opt-in `CatchPanicLayer` turning handler panics into 500s.
    catch_panic: bool,
    /// Bounded pinned-worker policy for non-`Send` render streams.
    local_stream_executor: LocalStreamExecutorConfig,
}

impl FrameworkBuilder {
    pub fn new(manifest: IslandManifest) -> Self {
        Self {
            manifest: Arc::new(manifest),
            base_path: BasePath::default(),
            metadata_base: None,
            rsjs_assets: false,
            config: ProaConfig::default(),
            static_router: None,
            dynamic_router: None,
            computed_metadata: None,
            external_apps: Vec::new(),
            process_manager: process_manager::ProcessManager::new(),
            body_limit: None,
            cross_origin_protection: true,
            trusted_origins: Vec::new(),
            security_headers: SecurityHeadersConfig::default(),
            csp: None,
            request_timeout: Some(DEFAULT_REQUEST_TIMEOUT),
            concurrency_limit: None,
            catch_panic: false,
            local_stream_executor: LocalStreamExecutorConfig::default(),
        }
    }

    /// Set the browser-visible mount prefix for a prefix-stripping proxy.
    /// Configure from an environment variable at startup with
    /// `BasePath::new(&std::env::var("PROA_BASE_PATH").unwrap_or_default())?`.
    /// Incoming routes remain rooted at `/`. Application-authored links and
    /// redirects use `app_url` or `cx.request().url` explicitly.
    pub fn with_base_path(mut self, base_path: BasePath) -> Self {
        self.base_path = base_path;
        self
    }

    /// Serve linked RSJS assets, rewriting runtime imports once at startup.
    /// Do not also register custom routes at `/rsjs-runtime.js`,
    /// `/rsjs-runtime-query.js`, or `/rsjs/island/:file`.
    pub fn with_rsjs_assets(mut self) -> Self {
        self.rsjs_assets = true;
        self
    }

    /// Override the default 1 MiB request-body size limit. Returns the
    /// builder for chaining.
    ///
    /// ```rust,ignore
    /// FrameworkBuilder::new(manifest)
    ///     .with_body_limit(4 * 1024 * 1024) // 4 MiB
    ///     .build();
    /// ```
    pub fn with_body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = Some(bytes);
        self
    }

    /// Enable or disable cross-origin protection (CSRF defense).
    ///
    /// **ON by default.** The built router rejects browser-issued
    /// cross-origin unsafe requests (POST, PUT, DELETE, ...) with 403
    /// based on the `Sec-Fetch-Site` header, falling back to comparing
    /// `Origin` against `Host` for older browsers. Safe methods and
    /// non-browser clients (no `Sec-Fetch-Site`, no `Origin`) always
    /// pass. See [`csrf::CrossOriginProtectionLayer`] for details.
    pub fn with_cross_origin_protection(mut self, enabled: bool) -> Self {
        self.cross_origin_protection = enabled;
        self
    }

    /// Trust an additional origin for cross-origin unsafe requests.
    ///
    /// `origin` is matched exactly against the `Origin` request header,
    /// so pass a serialized origin like `"https://app.example.com"`
    /// (scheme, host, optional port, no trailing slash).
    pub fn with_trusted_origin(mut self, origin: &str) -> Self {
        self.trusted_origins.push(origin.to_owned());
        self
    }

    /// Configure the security response headers applied to every
    /// response (frame options, HSTS, CSP). See
    /// [`SecurityHeadersConfig`] for the defaults.
    pub fn with_security_headers(mut self, config: SecurityHeadersConfig) -> Self {
        self.security_headers = config;
        self
    }

    /// Enable a strict, nonce-based `Content-Security-Policy`.
    ///
    /// **OFF by default**, because a strict policy has to match what the
    /// app's own markup loads. Turning it on mints a fresh 128-bit nonce per
    /// request, applies it to every Proa-emitted script tag (rsjs seed
    /// state, React island / import-map / bootstrap tags, the streaming
    /// bootstrap, and out-of-order boundary swaps), and sends:
    ///
    /// ```text
    /// default-src 'self'; script-src 'nonce-…' 'strict-dynamic';
    /// style-src 'self' 'unsafe-inline'; object-src 'none';
    /// base-uri 'self'; frame-ancestors 'none'
    /// ```
    ///
    /// Note what this does *not* cover: raw `<script>` tags you write
    /// yourself still need an explicit `nonce` attribute (read the request
    /// nonce from `cx.csp_nonce()`), and inline event-handler attributes
    /// (`onclick="…"`) never run under a nonce policy.
    ///
    /// Use [`with_csp_policy`](Self::with_csp_policy) to adjust directives.
    pub fn with_strict_csp(mut self) -> Self {
        self.csp = Some(CspPolicy::strict());
        self
    }

    /// Enable a nonce-based CSP with a customized policy.
    ///
    /// ```rust,ignore
    /// FrameworkBuilder::new(manifest)
    ///     .with_csp_policy(
    ///         CspPolicy::strict()
    ///             .with_directive("img-src", "'self' data: https://cdn.example")
    ///             .report_only(true),
    ///     )
    ///     .build();
    /// ```
    ///
    /// `report_only(true)` sends `Content-Security-Policy-Report-Only`,
    /// which is the safe way to roll a strict policy out on a live app:
    /// violations are reported but nothing is blocked.
    pub fn with_csp_policy(mut self, policy: CspPolicy) -> Self {
        self.csp = Some(policy);
        self
    }

    /// Override the default 30 second request timeout
    /// ([`DEFAULT_REQUEST_TIMEOUT`]).
    ///
    /// The timeout bounds producing the response; streamed bodies keep
    /// flowing after the response head is out.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// Disable the request timeout entirely. Prefer raising the timeout
    /// over removing it; without one, a stalled handler pins its
    /// resources until the client gives up.
    pub fn without_request_timeout(mut self) -> Self {
        self.request_timeout = None;
        self
    }

    /// Cap the number of in-flight requests across the whole router
    /// (off by default). Requests beyond `max` wait for a slot rather
    /// than being rejected, which keeps memory bounded under load
    /// spikes at the cost of added latency.
    pub fn with_concurrency_limit(mut self, max: usize) -> Self {
        self.concurrency_limit = Some(max);
        self
    }

    /// Opt in to `tower-http`'s `CatchPanicLayer` (off by default).
    ///
    /// When enabled, a panicking handler or middleware produces a plain
    /// `500 Internal Server Error` response instead of aborting the
    /// connection mid-stream. Panics are still logged; this only changes
    /// what the client observes. Left off by default so deployments that
    /// prefer crash-loud semantics (panic = bug = alert via connection
    /// reset) keep them.
    pub fn with_catch_panic(mut self, enabled: bool) -> Self {
        self.catch_panic = enabled;
        self
    }

    /// Configure the persistent executor used by local streaming renders.
    ///
    /// This is a joint server resource decision: deployments should account
    /// for these threads alongside Tokio's worker count. Admission and
    /// per-response transport limits provide explicit overload and memory
    /// bounds.
    pub fn with_local_stream_executor(mut self, config: LocalStreamExecutorConfig) -> Self {
        self.local_stream_executor = config;
        self
    }

    pub fn with_config(mut self, config: ProaConfig) -> Self {
        self.config = config;
        self
    }

    /// Serve application assets under `/static`. A configured base path also
    /// composes URL tokens in CSS and native URL operations in JS/MJS assets.
    /// Install compression outside this service so transformations see source bytes.
    pub fn with_static_service(mut self, service: Router) -> Self {
        self.static_router = Some(service);
        self
    }

    pub fn with_dynamic_routes(mut self, router: Router) -> Self {
        self.dynamic_router = Some(router);
        self
    }

    pub fn with_dynamic_routes_and_metadata(
        mut self,
        router: Router,
        metadata: Arc<HashMap<String, ComputedRouteMeta>>,
    ) -> Self {
        self.dynamic_router = Some(router);
        self.computed_metadata = Some(metadata);
        self
    }

    /// Mount an external application (static site, SSR app, or CSR SPA)
    ///
    /// # Example
    /// ```rust
    /// use proa_framework_axum::{
    ///     ExternalApp, ExternalAppStrategy, FrameworkBuilder, IslandManifest, ManifestAssets,
    /// };
    ///
    /// use std::collections::HashMap;
    ///
    /// let manifest = IslandManifest {
    ///     islands: HashMap::new(),
    ///     assets: ManifestAssets::default(),
    ///     import_map: HashMap::new(),
    ///     ssr_bundle: None,
    /// };
    ///
    /// let app = FrameworkBuilder::new(manifest)
    ///     .with_external_app(ExternalApp::static_app("/docs", "./apps/docs/out"))
    ///     .with_external_app(ExternalApp::ssr_app("/admin", "http://localhost:4000".to_string()))
    ///     .build();
    /// ```
    pub fn with_external_app(mut self, external_app: ExternalApp) -> Self {
        self.external_apps.push(external_app);
        self
    }

    /// Public base for canonical and social URLs. Configure per deployment;
    /// include the public mount prefix when the app is mounted below `/`.
    pub fn with_metadata_base(mut self, base: MetadataBase) -> Self {
        self.metadata_base = Some(base);
        self
    }

    pub fn config(&self) -> &ProaConfig {
        &self.config
    }

    pub fn build(self) -> Router {
        let FrameworkBuilder {
            manifest,
            base_path,
            metadata_base,
            rsjs_assets,
            config,
            static_router,
            dynamic_router,
            computed_metadata,
            external_apps,
            process_manager,
            body_limit,
            cross_origin_protection,
            trusted_origins,
            security_headers,
            csp,
            request_timeout,
            concurrency_limit,
            catch_panic,
            local_stream_executor,
        } = self;

        let body_limit_bytes = body_limit.unwrap_or(DEFAULT_BODY_LIMIT_BYTES);

        let mut app = Router::new();
        if rsjs_assets {
            app = app.merge(rsjs_assets_router(&base_path));
        }

        // Mount external apps first (before /static and dynamic routes)
        for external_app in external_apps {
            app = mount_external_app(
                app,
                external_app,
                &config,
                &process_manager,
                body_limit_bytes,
            );
        }

        if let Some(static_router) = static_router {
            app = app.nest(
                "/static",
                base_path_assets(static_router, base_path.clone()),
            );
        }

        if let Some(dynamic_router) = dynamic_router {
            app = app.merge(dynamic_router);
        }

        let computed_metadata = computed_metadata.unwrap_or_else(|| Arc::new(HashMap::new()));
        let mut layer = RouteContextLayer::new(manifest.clone(), computed_metadata)
            .with_base_path(base_path)
            .with_local_stream_executor_config(local_stream_executor);
        if let Some(base) = metadata_base {
            layer = layer.with_metadata_base(base);
        }
        app = app.layer(layer);
        // Apply a global body-size limit so a single oversized POST cannot
        // tie up the worker indefinitely. Per-route overrides via
        // `DefaultBodyLimit::max(...)` on a nested router still apply.
        app = app.layer(DefaultBodyLimit::max(body_limit_bytes));
        // SECURITY: cross-origin protection (CSRF defense) is ON by
        // default; see the builder docs and `csrf` module.
        if cross_origin_protection {
            let mut csrf_layer = CrossOriginProtectionLayer::new();
            for origin in trusted_origins {
                csrf_layer = csrf_layer.with_trusted_origin(origin);
            }
            app = app.layer(csrf_layer);
        }
        // The timeout covers producing the response (routing, render),
        // not draining the body, so streaming SSR keeps working.
        if let Some(timeout) = request_timeout {
            app = app.layer(default_request_timeout_layer(timeout));
        }
        if let Some(max) = concurrency_limit {
            app = app.layer(GlobalConcurrencyLimitLayer::new(max));
        }
        if catch_panic {
            app = app.layer(CatchPanicLayer::new());
        }
        if let Some(policy) = &csp {
            // Both layers write `Content-Security-Policy` and neither
            // clobbers an existing header, so the inner one wins. Say so at
            // startup rather than letting a nonce-free policy silently
            // break hydration. A report-only policy writes a different
            // header, so it does not collide.
            if security_headers.content_security_policy.is_some() && !policy.is_report_only() {
                tracing::warn!(
                    "both with_security_headers(..with_content_security_policy(..)) and a \
                     nonce-based CSP are configured; the static policy is sent and the nonce \
                     directives are not. Keep only one."
                );
            }
            // A relaxed X-Frame-Options means the app expects to be framed,
            // which the CSP default of `frame-ancestors 'none'` would still
            // block in modern browsers. Report-only does not block anything.
            if security_headers.frame_options != FrameOptions::Deny
                && policy.frame_ancestors_is_default()
                && !policy.is_report_only()
            {
                tracing::warn!(
                    "X-Frame-Options is relaxed but the CSP still sends `frame-ancestors \
                     'none'`, which blocks framing anyway; set it with \
                     CspPolicy::with_directive(\"frame-ancestors\", ..)."
                );
            }
        }
        // Applied outside the CSRF, timeout, and panic layers so their
        // generated error responses also carry the security headers.
        app = app.layer(SecurityHeadersLayer::new(security_headers));
        // Outside everything else so every response (including timeout and
        // panic responses) carries the policy, and so the request nonce is
        // installed before `RouteContextLayer` reads it.
        if let Some(policy) = csp {
            app = app.layer(CspNonceLayer::new(policy));
        }
        app.layer(Extension(manifest))
    }
}

// The root workspace currently resolves tower-http 0.6.6, where `new` is the
// only constructor. Downstream consumers may resolve 0.6.11, where it is
// deprecated in favor of `with_status_code`; keep the common source compatible
// with both until the root lockfile is upgraded.
#[allow(deprecated)]
fn default_request_timeout_layer(timeout: Duration) -> TimeoutLayer {
    TimeoutLayer::new(timeout)
}

/// Wrap a `ServeDir` for a filesystem mount with the framework's static
/// file hardening: `nosniff` so browsers never MIME-sniff a served file
/// into executable HTML/JS.
///
/// SECURITY: `tower-http`'s `ServeDir` rejects `..` path traversal but
/// FOLLOWS SYMLINKS inside the served directory, and tower-http 0.6.x
/// exposes no `follow_symlinks(false)` switch. Treat the served
/// directory as untrusted-content-free: never let untrusted writers
/// create files (or symlinks) under it, or a symlink pointing at, e.g.,
/// `/etc/passwd` would be served happily.
fn hardened_serve_dir(
    source_dir: std::path::PathBuf,
) -> tower_http::set_header::SetResponseHeader<ServeDir, HeaderValue> {
    SetResponseHeaderLayer::if_not_present(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    )
    .layer(ServeDir::new(source_dir))
}

/// Mount an external application into the router
fn mount_external_app(
    router: Router,
    external_app: ExternalApp,
    _config: &ProaConfig,
    process_manager: &process_manager::ProcessManager,
    body_limit_bytes: usize,
) -> Router {
    use external::ExternalAppStrategy;

    let mount_path = external_app.mount_path;
    let is_dev = cfg!(debug_assertions);

    match external_app.strategy {
        ExternalAppStrategy::Static {
            source_dir,
            spa_fallback,
        } => {
            // Serve static files from directory
            if spa_fallback {
                // CSR app: fallback to index.html for 404s (History API support)
                // TODO: Implement proper index.html fallback
                tracing::warn!("SPA fallback for {} not yet fully implemented", mount_path);
            }

            // For now, just serve the directory (see `hardened_serve_dir`
            // for the symlink and MIME-sniffing notes)
            let service = hardened_serve_dir(source_dir);
            router.nest_service(&mount_path, service)
        }

        ExternalAppStrategy::Csr {
            source_dir,
            dev_upstream,
        } => {
            if is_dev && dev_upstream.is_some() {
                // In dev mode: proxy to Vite/webpack dev server for HMR
                // TODO: Implement reverse proxy
                tracing::warn!(
                    "Dev proxy for CSR app at {} not yet implemented, serving from {}",
                    mount_path,
                    source_dir.display()
                );
            }

            // Serve built SPA (fallback to index.html handled by ServeDir)
            // TODO: Implement proper History API fallback
            let service = hardened_serve_dir(source_dir);
            router.nest_service(&mount_path, service)
        }

        ExternalAppStrategy::Ssr {
            upstream,
            dev_command,
            health_check,
            strip_prefix,
        } => {
            tracing::info!(
                "Mounting SSR app at {} → {} (strip_prefix: {})",
                mount_path,
                upstream,
                strip_prefix
            );

            // Spawn dev server if in development mode and dev_command is configured
            if is_dev {
                if let Some(cmd) = dev_command {
                    tracing::info!("Spawning dev server for {}: {}", mount_path, cmd);
                    if let Err(e) = process_manager.spawn(mount_path.clone(), cmd) {
                        tracing::error!("Failed to spawn dev server for {}: {}", mount_path, e);
                        tracing::warn!(
                            "Continuing without dev server - make sure upstream {} is running",
                            upstream
                        );
                    }

                    // Wait for health check if configured
                    if let Some(health_url) = health_check {
                        tracing::info!("Waiting for dev server at {} to be ready...", health_url);
                        // Note: This is blocking, but happens during app startup
                        // In a real implementation, we'd want to make this async
                        // For now, we'll just log a warning
                        tracing::warn!("Health check synchronization not yet implemented - continuing without wait");
                    }
                }
            }

            // Create proxy config. The proxy buffers request bodies, so
            // align its request cap with the framework-wide body limit
            // (axum's `DefaultBodyLimit` does not constrain the raw
            // `Request` extractor the proxy handler uses).
            let proxy_config = std::sync::Arc::new(
                crate::proxy::ProxyConfig::new(upstream.clone(), strip_prefix, mount_path.clone())
                    .with_max_request_body_bytes(body_limit_bytes),
            );

            // Create a catch-all route for the proxy
            let proxy_router = axum::Router::new()
                .fallback(crate::proxy::proxy_handler)
                .with_state(proxy_config);

            router.nest(&mount_path, proxy_router)
        }
    }
}

pub fn standalone_static_router(dir: &str) -> Router {
    // See `hardened_serve_dir` for the symlink and MIME-sniffing notes.
    Router::new().nest_service("/", hardened_serve_dir(dir.into()))
}

pub use route::PageCache;
