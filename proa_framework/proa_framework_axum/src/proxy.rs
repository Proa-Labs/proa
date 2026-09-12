// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Reverse proxy implementation for SSR external apps
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use thiserror::Error;

/// Default connect timeout for upstream requests.
pub const DEFAULT_PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default end-to-end timeout for upstream requests (connect + send +
/// response headers + body).
pub const DEFAULT_PROXY_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Default cap on the buffered request body forwarded upstream (10 MiB).
///
/// `FrameworkBuilder::build` aligns this with the framework-wide body
/// limit (`DEFAULT_BODY_LIMIT_BYTES` / `with_body_limit`) when it mounts
/// SSR external apps; this constant is the fallback for callers that
/// construct a [`ProxyConfig`] directly.
pub const DEFAULT_PROXY_REQUEST_BODY_LIMIT: usize = 10 * 1024 * 1024;

/// Default cap on the buffered upstream response body (16 MiB).
///
/// SECURITY: the proxy buffers the full upstream response in memory
/// before relaying it, so an unbounded read would let a malicious or
/// misbehaving upstream exhaust server memory.
pub const DEFAULT_PROXY_RESPONSE_BODY_LIMIT: usize = 16 * 1024 * 1024;

/// Configuration for the reverse proxy
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Upstream server URL (e.g., "http://localhost:4000")
    pub upstream: String,

    /// Whether to strip the mount path prefix when proxying
    pub strip_prefix: bool,

    /// Mount path to strip if strip_prefix is true
    pub mount_path: String,

    /// TCP connect timeout for upstream requests.
    pub connect_timeout: Duration,

    /// End-to-end timeout for upstream requests.
    pub request_timeout: Duration,

    /// Maximum request body size buffered and forwarded upstream.
    pub max_request_body_bytes: usize,

    /// Maximum upstream response body size buffered and relayed back.
    pub max_response_body_bytes: usize,

    /// Lazily built shared HTTP client. One client per proxy mount, so
    /// connection pools are reused across requests instead of paying a
    /// fresh client (and connection) per proxied request.
    client: OnceLock<reqwest::Client>,
}

impl ProxyConfig {
    /// Create a proxy config with the default timeouts and body limits.
    pub fn new(
        upstream: impl Into<String>,
        strip_prefix: bool,
        mount_path: impl Into<String>,
    ) -> Self {
        Self {
            upstream: upstream.into(),
            strip_prefix,
            mount_path: mount_path.into(),
            connect_timeout: DEFAULT_PROXY_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_PROXY_REQUEST_TIMEOUT,
            max_request_body_bytes: DEFAULT_PROXY_REQUEST_BODY_LIMIT,
            max_response_body_bytes: DEFAULT_PROXY_RESPONSE_BODY_LIMIT,
            client: OnceLock::new(),
        }
    }

    /// Override the upstream TCP connect timeout.
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Override the end-to-end upstream request timeout.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Override the maximum request body size forwarded upstream.
    pub fn with_max_request_body_bytes(mut self, bytes: usize) -> Self {
        self.max_request_body_bytes = bytes;
        self
    }

    /// Override the maximum upstream response body size relayed back.
    pub fn with_max_response_body_bytes(mut self, bytes: usize) -> Self {
        self.max_response_body_bytes = bytes;
        self
    }

    /// Shared HTTP client for this proxy mount, built once on first use
    /// with the configured timeouts. Cloning `reqwest::Client` is cheap
    /// (it is an `Arc` internally).
    fn client(&self) -> Result<reqwest::Client, ProxyError> {
        if let Some(client) = self.client.get() {
            return Ok(client.clone());
        }
        let built = reqwest::Client::builder()
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .build()
            .map_err(ProxyError::ClientBuild)?;
        // Benign race: two concurrent first requests may both build a
        // client; `get_or_init` keeps one and the loser is dropped.
        Ok(self.client.get_or_init(|| built).clone())
    }
}

/// Fixed incoming request headers that must not be forwarded to the upstream.
fn is_fixed_blocked_request_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "connection"
            | "keep-alive"
            | "transfer-encoding"
            | "upgrade"
            | "te"
            | "trailer"
            | "proxy-connection"
            | "proxy-authenticate"
            | "proxy-authorization"
    )
}

/// Fixed upstream response headers that must not be relayed to the client.
fn is_fixed_blocked_response_header(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "transfer-encoding"
            | "upgrade"
            | "te"
            | "trailer"
            | "proxy-connection"
            | "proxy-authenticate"
            | "proxy-authorization"
    )
}

/// A `Connection` header can nominate additional hop-by-hop header names.
/// Those nominated headers are also scoped to one connection and must be
/// stripped before proxying in either direction.
fn is_connection_nominated_header(headers: &HeaderMap, candidate: &str) -> bool {
    headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case(candidate))
}

/// Forwarding metadata headers that clients must never be able to supply
/// themselves. SECURITY: a client-supplied `X-Forwarded-For` or
/// `X-Forwarded-Host` would let an attacker spoof their address or the
/// original host to the upstream app (IP allowlists, audit logs, absolute
/// URL generation). The proxy strips them and re-derives trustworthy
/// values itself.
fn is_client_forwarding_header(name: &str) -> bool {
    matches!(
        name,
        "x-forwarded-for" | "x-forwarded-host" | "x-forwarded-proto" | "forwarded"
    )
}

/// Decide whether a single incoming request header may be forwarded
/// verbatim to the upstream.
fn should_forward_request_header(headers: &HeaderMap, name: &str) -> bool {
    !is_fixed_blocked_request_header(name)
        && !is_connection_nominated_header(headers, name)
        && !is_client_forwarding_header(name)
}

/// Decide whether a single upstream response header may be relayed back
/// to the client.
fn should_forward_response_header(headers: &HeaderMap, name: &str) -> bool {
    !is_fixed_blocked_response_header(name) && !is_connection_nominated_header(headers, name)
}

const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
const X_FORWARDED_HOST: HeaderName = HeaderName::from_static("x-forwarded-host");
const X_FORWARDED_PROTO: HeaderName = HeaderName::from_static("x-forwarded-proto");

/// Build the full header map sent to the upstream from the incoming
/// request headers.
///
/// Pure function so the forwarding policy is unit-testable without a
/// network: hop-by-hop and client-supplied `X-Forwarded-*` / `Forwarded`
/// headers are dropped, everything else is forwarded (duplicates
/// preserved), and fresh `X-Forwarded-Host` / `X-Forwarded-Proto` values
/// are derived from the request itself. `X-Forwarded-For` is only set
/// when a verified peer address is available (axum `ConnectInfo`);
/// otherwise it is omitted entirely rather than trusting client input.
fn build_upstream_headers(
    incoming: &HeaderMap,
    scheme: &str,
    peer_addr: Option<SocketAddr>,
) -> HeaderMap {
    let mut out = HeaderMap::with_capacity(incoming.len() + 3);
    for (name, value) in incoming.iter() {
        if should_forward_request_header(incoming, name.as_str()) {
            // `append`, not `insert`: repeated headers (e.g. multiple
            // `cookie` lines) must all reach the upstream.
            out.append(name.clone(), value.clone());
        }
    }

    if let Some(host) = incoming.get(header::HOST) {
        out.insert(X_FORWARDED_HOST, host.clone());
    }
    if let Ok(proto) = HeaderValue::from_str(scheme) {
        out.insert(X_FORWARDED_PROTO, proto);
    }
    if let Some(peer) = peer_addr {
        if let Ok(value) = HeaderValue::from_str(&peer.ip().to_string()) {
            out.insert(X_FORWARDED_FOR, value);
        }
    }

    out
}

/// Reverse proxy handler that forwards requests to an upstream server
pub async fn proxy_handler(
    State(config): State<Arc<ProxyConfig>>,
    req: Request,
) -> Result<Response, ProxyError> {
    // Get the request URI path
    let path = req.uri().path();
    let query = req.uri().query();

    // Construct the upstream URL
    let upstream_path = if config.strip_prefix && path.starts_with(&config.mount_path) {
        // Strip the mount path prefix
        path.strip_prefix(&config.mount_path).unwrap_or(path)
    } else {
        path
    };

    let upstream_url = if let Some(q) = query {
        format!("{}{}?{}", config.upstream, upstream_path, q)
    } else {
        format!("{}{}", config.upstream, upstream_path)
    };

    tracing::debug!("Proxying {} → {}", path, upstream_url);

    let client = config.client()?;
    let method = req.method().clone();
    let scheme = req.uri().scheme_str().unwrap_or("http");
    // Real peer address, if the app was served with
    // `into_make_service_with_connect_info::<SocketAddr>()`. When absent
    // we omit X-Forwarded-For instead of trusting client headers.
    let peer_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);
    let upstream_headers = build_upstream_headers(req.headers(), scheme, peer_addr);

    let mut upstream_req = client
        .request(method, &upstream_url)
        .headers(upstream_headers);

    // Forward the request body (bounded to prevent memory exhaustion).
    let body_bytes = axum::body::to_bytes(req.into_body(), config.max_request_body_bytes)
        .await
        .map_err(|e| ProxyError::BodyRead(e.to_string()))?;

    if !body_bytes.is_empty() {
        upstream_req = upstream_req.body(body_bytes);
    }

    // Send the request to upstream
    let mut upstream_resp = upstream_req.send().await.map_err(|e| {
        if e.is_timeout() {
            ProxyError::UpstreamTimeout(e)
        } else {
            ProxyError::Upstream(e)
        }
    })?;

    // Convert the upstream response to an axum response
    let status = upstream_resp.status();
    let mut response_builder = Response::builder().status(status);

    // Forward response headers (excluding hop-by-hop headers)
    let upstream_headers = upstream_resp.headers();
    for (name, value) in upstream_headers.iter() {
        if should_forward_response_header(upstream_headers, name.as_str()) {
            response_builder = response_builder.header(name, value);
        }
    }

    // Read the response body with a hard cap so a malicious or broken
    // upstream cannot exhaust memory with an unbounded body.
    let limit = config.max_response_body_bytes;
    if let Some(len) = upstream_resp.content_length() {
        if len > limit as u64 {
            return Err(ProxyError::ResponseTooLarge { limit });
        }
    }
    let mut body = Vec::with_capacity(
        upstream_resp
            .content_length()
            .map(|len| len.min(limit as u64) as usize)
            .unwrap_or(8 * 1024),
    );
    while let Some(chunk) = upstream_resp.chunk().await.map_err(|e| {
        if e.is_timeout() {
            ProxyError::UpstreamTimeout(e)
        } else {
            ProxyError::ResponseRead(e)
        }
    })? {
        if body.len() + chunk.len() > limit {
            return Err(ProxyError::ResponseTooLarge { limit });
        }
        body.extend_from_slice(&chunk);
    }

    let response = response_builder
        .body(Body::from(body))
        .map_err(ProxyError::ResponseBuild)?;

    Ok(response)
}

/// Errors that can occur during proxying.
///
/// SECURITY: the `IntoResponse` impl deliberately maps every variant to a
/// short generic body. Upstream error details (addresses, TLS or DNS
/// failures, internal topology) are logged via `tracing::warn!` only and
/// never leak to the client.
#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("failed to read proxied request body: {0}")]
    BodyRead(String),
    #[error("failed to build upstream HTTP client: {0}")]
    ClientBuild(#[source] reqwest::Error),
    #[error("upstream request timed out: {0}")]
    UpstreamTimeout(#[source] reqwest::Error),
    #[error("upstream request failed: {0}")]
    Upstream(#[source] reqwest::Error),
    #[error("failed to read upstream response: {0}")]
    ResponseRead(#[source] reqwest::Error),
    #[error("upstream response exceeded the configured limit of {limit} bytes")]
    ResponseTooLarge { limit: usize },
    #[error("failed to build proxied response: {0}")]
    ResponseBuild(#[source] axum::http::Error),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        tracing::warn!(error = %self, "proxy request failed");
        let (status, message) = match self {
            ProxyError::BodyRead(_) => (StatusCode::BAD_REQUEST, "Bad Request"),
            ProxyError::UpstreamTimeout(_) => (StatusCode::GATEWAY_TIMEOUT, "Gateway Timeout"),
            ProxyError::ClientBuild(_)
            | ProxyError::Upstream(_)
            | ProxyError::ResponseRead(_)
            | ProxyError::ResponseTooLarge { .. } => (StatusCode::BAD_GATEWAY, "Bad Gateway"),
            ProxyError::ResponseBuild(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
            }
        };

        (status, message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming_headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
        headers
    }

    #[test]
    fn client_supplied_forwarding_headers_are_stripped() {
        let incoming = incoming_headers(&[
            ("host", "example.com"),
            ("x-forwarded-for", "6.6.6.6"),
            ("x-forwarded-host", "evil.example"),
            ("x-forwarded-proto", "https"),
            ("forwarded", "for=6.6.6.6;host=evil.example"),
        ]);

        let out = build_upstream_headers(&incoming, "http", None);

        assert!(out.get("forwarded").is_none());
        // The forwarded values must be re-derived, never the client's.
        assert_eq!(out.get("x-forwarded-host").unwrap(), "example.com");
        assert_eq!(out.get("x-forwarded-proto").unwrap(), "http");
        assert!(
            out.get("x-forwarded-for").is_none(),
            "without a verified peer address X-Forwarded-For must be omitted"
        );
    }

    #[test]
    fn hop_by_hop_headers_are_stripped() {
        let incoming = incoming_headers(&[
            ("host", "example.com"),
            ("connection", "keep-alive"),
            ("keep-alive", "timeout=5"),
            ("transfer-encoding", "chunked"),
            ("upgrade", "websocket"),
            ("te", "trailers"),
            ("trailer", "expires"),
            ("proxy-connection", "keep-alive"),
            ("proxy-authorization", "Basic abc"),
        ]);

        let out = build_upstream_headers(&incoming, "http", None);

        for name in [
            "host",
            "connection",
            "keep-alive",
            "transfer-encoding",
            "upgrade",
            "te",
            "trailer",
            "proxy-connection",
            "proxy-authorization",
        ] {
            assert!(out.get(name).is_none(), "{name} should not be forwarded");
        }
    }

    #[test]
    fn connection_nominated_request_headers_are_stripped() {
        let incoming = incoming_headers(&[
            ("host", "example.com"),
            ("connection", "x-internal-auth, x-debug-token"),
            ("x-internal-auth", "secret"),
            ("x-debug-token", "debug"),
            ("x-request-id", "abc-123"),
        ]);

        let out = build_upstream_headers(&incoming, "http", None);

        assert!(out.get("x-internal-auth").is_none());
        assert!(out.get("x-debug-token").is_none());
        assert_eq!(out.get("x-request-id").unwrap(), "abc-123");
    }

    #[test]
    fn normal_headers_are_forwarded_with_duplicates() {
        let incoming = incoming_headers(&[
            ("accept", "text/html"),
            ("cookie", "a=1"),
            ("cookie", "b=2"),
            ("authorization", "Bearer t"),
            ("x-request-id", "abc-123"),
        ]);

        let out = build_upstream_headers(&incoming, "http", None);

        assert_eq!(out.get("accept").unwrap(), "text/html");
        assert_eq!(out.get("authorization").unwrap(), "Bearer t");
        assert_eq!(out.get("x-request-id").unwrap(), "abc-123");
        let cookies: Vec<_> = out.get_all("cookie").iter().collect();
        assert_eq!(cookies, vec!["a=1", "b=2"]);
    }

    #[test]
    fn forwarded_host_and_proto_are_derived_from_the_request() {
        let incoming = incoming_headers(&[("host", "shop.example.com:8443")]);

        let out = build_upstream_headers(&incoming, "https", None);

        assert_eq!(
            out.get("x-forwarded-host").unwrap(),
            "shop.example.com:8443"
        );
        assert_eq!(out.get("x-forwarded-proto").unwrap(), "https");
    }

    #[test]
    fn forwarded_for_uses_the_verified_peer_address_only() {
        let incoming = incoming_headers(&[("host", "example.com"), ("x-forwarded-for", "6.6.6.6")]);
        let peer: SocketAddr = "203.0.113.9:51724".parse().unwrap();

        let out = build_upstream_headers(&incoming, "http", Some(peer));

        assert_eq!(out.get("x-forwarded-for").unwrap(), "203.0.113.9");
    }

    #[test]
    fn response_hop_by_hop_headers_are_not_relayed() {
        let headers = HeaderMap::new();
        assert!(!should_forward_response_header(&headers, "connection"));
        assert!(!should_forward_response_header(&headers, "keep-alive"));
        assert!(!should_forward_response_header(
            &headers,
            "transfer-encoding"
        ));
        assert!(!should_forward_response_header(&headers, "upgrade"));
        assert!(!should_forward_response_header(
            &headers,
            "proxy-authenticate"
        ));
        assert!(!should_forward_response_header(
            &headers,
            "proxy-authorization"
        ));
        assert!(should_forward_response_header(&headers, "content-type"));
        assert!(should_forward_response_header(&headers, "set-cookie"));
    }

    #[test]
    fn connection_nominated_response_headers_are_not_relayed() {
        let headers = incoming_headers(&[
            ("connection", "x-upstream-secret, x-cache-hop"),
            ("x-upstream-secret", "secret"),
            ("x-cache-hop", "1"),
            ("content-type", "text/html"),
        ]);

        assert!(!should_forward_response_header(
            &headers,
            "x-upstream-secret"
        ));
        assert!(!should_forward_response_header(&headers, "x-cache-hop"));
        assert!(should_forward_response_header(&headers, "content-type"));
    }

    #[test]
    fn proxy_errors_render_generic_bodies() {
        let response = ProxyError::BodyRead("boom".into()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = ProxyError::ResponseTooLarge { limit: 16 }.into_response();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
}
