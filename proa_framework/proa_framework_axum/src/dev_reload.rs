// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Development-only browser live reload.
//!
//! `proa dev` runs the live-reload event stream in the CLI process, because
//! the site's own server is stopped and replaced on every rebuild. This module
//! is the other half: it puts a small client script into HTML responses so an
//! open tab knows where that stream lives.
//!
//! Activation is driven entirely by two environment variables that only
//! `proa dev` sets: `PROA_DEV_RELOAD_PORT` and the per-session
//! `PROA_DEV_RELOAD_TOKEN`. Both are required, and the token is redrawn for
//! every `proa dev` process, so neither a stale export nor a shared
//! environment block can switch dev tooling on by accident. A release binary
//! started without them, which is every production start, never builds the
//! layer at all and pays nothing beyond the one environment read that finds
//! the port missing during [`serve`](crate::server::serve).
//!
//! Injection is streaming-safe. The client script is spliced in immediately
//! before the first `</head>` as body frames pass through, so streaming SSR
//! keeps its time to first byte. A document with no `</head>` gets the script
//! appended after its final frame instead.

use std::{
    env,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
};

use axum::{
    body::Body,
    http::{
        header::{CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE},
        HeaderMap, Method, Request, Response,
    },
};
use bytes::{BufMut, Bytes, BytesMut};
use hyper::body::{Body as HttpBody, Frame, SizeHint};
use tower::{Layer, Service};

/// Environment variable carrying the live-reload port. `proa dev` sets it on
/// the server process it spawns; nothing else should.
pub const RELOAD_PORT_ENV: &str = "PROA_DEV_RELOAD_PORT";

/// Environment variable carrying the dev session's secret.
///
/// Injection needs this as well as the port. The secret is regenerated for
/// every `proa dev` process, so a port left exported in a shell profile or a
/// shared deployment environment block cannot switch dev tooling on by
/// itself.
pub const RELOAD_TOKEN_ENV: &str = "PROA_DEV_RELOAD_TOKEN";

/// Path of the CLI's event stream. `proa_cli`'s `lifecycle::reload` hard-codes
/// the same literal; the two crates share no dependency, so a change here has
/// to be mirrored there.
const DEV_EVENTS_PATH: &str = "/__proa/dev-events";

/// Insertion point. Matched case-insensitively, since a hand-written template
/// may well shout its tags.
const HEAD_CLOSE: &[u8] = b"</head";

/// Length in bytes of the session secret `proa dev` generates. Its
/// `lifecycle::reload` owns the other copy of this number.
const TOKEN_BYTES: usize = 16;

const CLIENT_JS: &str = include_str!("dev_reload_client.js");

/// Build the live-reload layer when `proa dev` asked for it.
///
/// Returns `None` unless both variables are present and well formed, so a
/// stray or hostile value cannot switch dev tooling on in a deployed process.
pub fn layer_from_env() -> Option<DevReloadLayer> {
    let port = env::var(RELOAD_PORT_ENV).ok()?;
    let Some(port) = parse_reload_port(&port) else {
        tracing::warn!(
            value = %port,
            "{RELOAD_PORT_ENV} is not a port number; live reload stays off"
        );
        return None;
    };
    let token = env::var(RELOAD_TOKEN_ENV).unwrap_or_default();
    let Some(token) = parse_reload_token(&token) else {
        tracing::warn!(
            "{RELOAD_PORT_ENV} is set without a valid {RELOAD_TOKEN_ENV}; live reload stays off"
        );
        return None;
    };
    let base_path = match proa_core::BasePath::new(&env::var("PROA_BASE_PATH").unwrap_or_default())
    {
        Ok(base_path) => base_path,
        Err(_) => {
            tracing::warn!("invalid PROA_BASE_PATH; live reload stays off");
            return None;
        }
    };
    Some(DevReloadLayer::new(port, token).with_base_path(&base_path))
}

fn parse_reload_port(value: &str) -> Option<u16> {
    match value.trim().parse::<u16>() {
        Ok(0) | Err(_) => None,
        Ok(port) => Some(port),
    }
}

/// Accept only lowercase hex of the exact length `proa dev` emits.
///
/// The token is written straight into a string literal in the injected
/// script, so restricting the alphabet is what makes that splice safe: no
/// quote, backslash, angle bracket, or newline can reach the page.
fn parse_reload_token(value: &str) -> Option<&str> {
    let token = value.trim();
    let expected = TOKEN_BYTES * 2;
    (token.len() == expected
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(token)
}

/// Injects the `proa dev` live-reload client into HTML responses.
///
/// Applied automatically by [`serve`](crate::server::serve) when `proa dev`
/// supplies a port and session token. Add it by hand only when serving with
/// something other than that entry point.
#[derive(Debug, Clone)]
pub struct DevReloadLayer {
    script: Arc<str>,
    /// Content-Security-Policy source: self for mounted apps, the reload port otherwise.
    connect_source: Arc<str>,
    port: u16,
    token: Arc<str>,
    relay: bool,
    relay_stop: tokio::sync::watch::Sender<bool>,
}

impl DevReloadLayer {
    /// Build a layer pointing at a live-reload event stream on `port`.
    ///
    /// `token` authenticates the page to that stream. Values that are not
    /// lowercase hex are dropped rather than escaped, so the script can splice
    /// it in as a plain literal.
    pub fn new(port: u16, token: &str) -> Self {
        let (relay_stop, _) = tokio::sync::watch::channel(false);
        Self {
            script: Arc::from(""),
            connect_source: Arc::from(""),
            port,
            token: Arc::from(parse_reload_token(token).unwrap_or_default()),
            relay: false,
            relay_stop,
        }
        .with_base_path(&proa_core::BasePath::default())
    }

    /// Route the browser event stream through the public mount on this server.
    /// Use `wrap` to install its same-origin relay as well as client injection.
    pub fn with_base_path(mut self, base_path: &proa_core::BasePath) -> Self {
        self.relay = !base_path.as_str().is_empty();
        let path = base_path.url(DEV_EVENTS_PATH).to_string();
        // This JSON literal is embedded in an HTML script raw-text element.
        let path = serde_json::to_string(&path)
            .expect("serialize dev event URL")
            .replace('<', "\\u003c");
        self.script = Arc::from(
            CLIENT_JS
                .replace("__PROA_DEV_RELOAD_PORT__", &self.port.to_string())
                .replace("__PROA_DEV_EVENTS_PATH__", &path)
                .replace(
                    "__PROA_DEV_RELOAD_SAME_ORIGIN__",
                    if self.relay { "true" } else { "false" },
                )
                .replace("__PROA_DEV_RELOAD_TOKEN__", &self.token),
        );
        self.connect_source = if self.relay {
            Arc::from("'self'")
        } else {
            Arc::from(format!("http://*:{}", self.port))
        };
        self
    }

    /// End relayed streams before draining the server's ordinary requests.
    /// `serve_with_shutdown` calls this automatically.
    pub fn shutdown(&self) {
        self.relay_stop.send_replace(true);
    }

    /// Add the development event relay when mounted, then inject the client.
    pub fn wrap(self, app: axum::Router) -> axum::Router {
        let app = if self.relay {
            let host = env::var("PROA_HOST").unwrap_or_else(|_| "127.0.0.1".into());
            let host = match host.parse::<std::net::IpAddr>() {
                Ok(std::net::IpAddr::V6(ip)) if ip.is_unspecified() => "[::1]".to_string(),
                Ok(std::net::IpAddr::V6(ip)) => format!("[{ip}]"),
                Ok(std::net::IpAddr::V4(ip)) if ip.is_unspecified() => "127.0.0.1".to_string(),
                Ok(ip) => ip.to_string(),
                Err(_)
                    if !host.is_empty()
                        && host
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')) =>
                {
                    host
                }
                Err(_) => "localhost".to_string(),
            };
            let target = format!("http://{host}:{}{}", self.port, DEV_EVENTS_PATH);
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(2))
                .build()
                .expect("build local dev event client");
            let relay_stop = self.relay_stop.clone();
            app.route(
                DEV_EVENTS_PATH,
                axum::routing::get(
                    move |axum::extract::OriginalUri(uri): axum::extract::OriginalUri| {
                        let client = client.clone();
                        let target = target.clone();
                        let mut stopped = relay_stop.subscribe();
                        async move {
                            if *stopped.borrow() { return Response::new(Body::empty()); }
                            let target = match uri.query() {
                                Some(query) => format!("{target}?{query}"),
                                None => target,
                            };
                            let upstream = tokio::select! {
                                biased;
                                _ = stopped.changed() => return Response::new(Body::empty()),
                                upstream = client.get(target).send() => upstream,
                            };
                            match upstream {
                                Ok(response) => {
                                    let status = response.status();
                                    let content_type =
                                        response.headers().get(CONTENT_TYPE).cloned();
                                    let stream = futures::stream::try_unfold(
                                        (response, stopped),
                                        |(mut response, mut stopped)| async move {
                                            if *stopped.borrow() { return Ok::<_, reqwest::Error>(None); }
                                            tokio::select! {
                                                biased;
                                                _ = stopped.changed() => Ok(None),
                                                chunk = response.chunk() => Ok(chunk?.map(|bytes| (bytes, (response, stopped)))),
                                            }
                                        },
                                    );
                                    let mut response = Response::new(Body::from_stream(stream));
                                    *response.status_mut() = status;
                                    if let Some(content_type) = content_type {
                                        response.headers_mut().insert(CONTENT_TYPE, content_type);
                                    }
                                    response.headers_mut().insert(
                                        axum::http::header::CACHE_CONTROL,
                                        axum::http::HeaderValue::from_static("no-cache"),
                                    );
                                    response.headers_mut().insert("x-accel-buffering", axum::http::HeaderValue::from_static("no"));
                                    response
                                }
                                Err(_) => Response::builder()
                                    .status(axum::http::StatusCode::BAD_GATEWAY)
                                    .body(Body::empty())
                                    .expect("valid dev relay response"),
                            }
                        }
                    },
                ),
            )
        } else {
            app
        };
        app.layer(self)
    }

    fn tag(&self, nonce: Option<&str>) -> Bytes {
        let mut tag = BytesMut::with_capacity(self.script.len() + 64);
        tag.put_slice(b"<script");
        if let Some(nonce) = nonce {
            tag.put_slice(b" nonce=\"");
            tag.put_slice(nonce.as_bytes());
            tag.put_slice(b"\"");
        }
        tag.put_slice(b">");
        tag.put_slice(self.script.as_bytes());
        tag.put_slice(b"</script>");
        tag.freeze()
    }
}

impl<S> Layer<S> for DevReloadLayer {
    type Service = DevReload<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DevReload {
            inner,
            layer: self.clone(),
        }
    }
}

/// The [`DevReloadLayer`] service.
#[derive(Debug, Clone)]
pub struct DevReload<S> {
    inner: S,
    layer: DevReloadLayer,
}

impl<S, B> Service<Request<B>> for DevReload<S>
where
    S: Service<Request<B>, Response = Response<Body>>,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = DevReloadFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        // Every stream costs one of the browser's six connections per origin.
        // A page that embeds same-origin iframes would spend them on frames
        // that cannot usefully reload on their own, and the tab opened after
        // that would hang unconnected with no error to explain it.
        //
        // A HEAD response is skipped for a different reason: its
        // `content-length` is meant to describe what GET would return, and
        // this layer cannot know the injected length without running the body.
        let skip = is_embedded_destination(request.headers()) || request.method() == Method::HEAD;
        DevReloadFuture {
            inner: self.inner.call(request),
            layer: self.layer.clone(),
            skip,
        }
    }
}

fn is_embedded_destination(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-dest")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            matches!(
                value.trim(),
                "iframe" | "frame" | "embed" | "object" | "fencedframe"
            )
        })
}

#[pin_project::pin_project]
#[derive(Debug)]
pub struct DevReloadFuture<F> {
    #[pin]
    inner: F,
    layer: DevReloadLayer,
    skip: bool,
}

impl<F, E> Future for DevReloadFuture<F>
where
    F: Future<Output = Result<Response<Body>, E>>,
{
    type Output = Result<Response<Body>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let response = ready!(this.inner.poll(cx))?;
        if *this.skip {
            return Poll::Ready(Ok(response));
        }
        Poll::Ready(Ok(inject(response, this.layer)))
    }
}

fn inject(mut response: Response<Body>, layer: &DevReloadLayer) -> Response<Body> {
    if !accepts_injection(response.headers()) {
        return response;
    }

    let nonce = script_nonce(response.headers());
    let tag = layer.tag(nonce.as_deref());
    // A strict policy blocks the event stream as surely as it blocks an
    // inline script, and `connect-src` falls back to `default-src`, so a
    // site with `default-src 'self'` cannot reach another port. Widen the
    // one directive that matters and leave the rest of the policy alone.
    if let Some(policy) = allow_connect_source(response.headers(), &layer.connect_source) {
        if let Ok(value) = policy.parse() {
            response
                .headers_mut()
                .insert(CONTENT_SECURITY_POLICY, value);
        }
    }
    // The body grows by an unknown amount and may be streamed, so a declared
    // length can no longer be honoured. Dropping it hands framing back to
    // hyper, which falls back to chunked transfer.
    response.headers_mut().remove(CONTENT_LENGTH);
    response.map(|body| Body::new(InjectBody::new(body, tag)))
}

fn accepts_injection(headers: &HeaderMap) -> bool {
    let is_html = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/html"))
        });
    // An encoded body would have to be decoded, patched, and re-encoded.
    // Leaving it alone costs one page that does not live reload; corrupting
    // it costs the whole session.
    let is_identity = headers
        .get(CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|value| value.trim().eq_ignore_ascii_case("identity"));
    is_html && is_identity
}

/// Rewrite a Content-Security-Policy so the live-reload stream is reachable.
///
/// Returns `None` when the response carries no policy, or when nothing
/// restricts connections, since there is then nothing to widen.
fn allow_connect_source(headers: &HeaderMap, source: &str) -> Option<String> {
    let policy = headers.get(CONTENT_SECURITY_POLICY)?.to_str().ok()?;
    if directive_value(policy, "connect-src")
        .or_else(|| directive_value(policy, "default-src"))
        .is_some_and(|sources| {
            sources
                .split_whitespace()
                .any(|existing| existing == source)
        })
    {
        return None;
    }
    let mut directives: Vec<String> = Vec::new();
    let mut widened = false;
    let mut default_src = None;

    for directive in policy.split(';') {
        let trimmed = directive.trim();
        if trimmed.is_empty() {
            continue;
        }
        let name = trimmed.split_whitespace().next().unwrap_or_default();
        if name.eq_ignore_ascii_case("connect-src") {
            directives.push(format!("{trimmed} {source}"));
            widened = true;
        } else {
            if name.eq_ignore_ascii_case("default-src") {
                default_src = Some(trimmed.to_string());
            }
            directives.push(trimmed.to_string());
        }
    }

    if !widened {
        // Without an explicit `connect-src`, connections are governed by
        // `default-src`. Carry its sources into a new directive so the app's
        // own requests keep working exactly as before.
        let fallback = default_src?;
        let sources = fallback
            .split_once(char::is_whitespace)
            .map_or("", |(_, sources)| sources.trim());
        directives.push(format!("connect-src {sources} {source}").replace("  ", " "));
    }

    Some(directives.join("; "))
}

/// Reuse the nonce the response already advertises.
///
/// A site that sets a strict Content-Security-Policy would otherwise block the
/// injected script, and live reload would fail with nothing but a console
/// error to explain it.
fn script_nonce(headers: &HeaderMap) -> Option<String> {
    let policy = headers.get(CONTENT_SECURITY_POLICY)?.to_str().ok()?;
    // Take the nonce from the directive that actually governs scripts. A
    // policy can carry a different one for styles, and borrowing that would
    // produce a tag the browser refuses for exactly the reason this reuse
    // exists to avoid.
    let directive =
        directive_value(policy, "script-src").or_else(|| directive_value(policy, "default-src"))?;

    let start = directive.find("'nonce-")? + "'nonce-".len();
    let rest = &directive[start..];
    let end = rest.find('\'')?;
    let nonce = &rest[..end];
    // Only base64 characters reach the attribute, so nothing in the header can
    // close the tag or open a new one.
    nonce
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '='))
        .then(|| nonce.to_string())
}

/// The source list of one Content-Security-Policy directive.
fn directive_value<'policy>(policy: &'policy str, name: &str) -> Option<&'policy str> {
    policy.split(';').find_map(|directive| {
        let directive = directive.trim();
        let (candidate, sources) = directive.split_once(char::is_whitespace)?;
        candidate.eq_ignore_ascii_case(name).then_some(sources)
    })
}

/// A response body with the live-reload script spliced into it.
///
/// Frames pass through as they arrive. Only the last few bytes of each frame
/// are ever held back, so that a `</head>` split across two frames is still
/// found, and only until the script has been placed.
#[pin_project::pin_project]
struct InjectBody<B> {
    #[pin]
    inner: B,
    /// `None` once the script has been emitted.
    script: Option<Bytes>,
    /// Tail of the previous frame, short enough to be free.
    carry: Bytes,
    /// Held back when the body ends with trailers before the script is placed.
    trailers: Option<Frame<Bytes>>,
    /// `None` until the opening bytes reveal whether this is a whole
    /// document. A fragment that never closes a `<head>` is left as it was.
    document: Option<bool>,
    /// The opening bytes themselves, kept separately because the buffer they
    /// arrived in is forwarded onward as soon as it is scanned.
    head: BytesMut,
    finished: bool,
}

impl<B> InjectBody<B> {
    fn new(inner: B, script: Bytes) -> Self {
        Self {
            inner,
            script: Some(script),
            carry: Bytes::new(),
            trailers: None,
            document: None,
            head: BytesMut::new(),
            finished: false,
        }
    }
}

impl<B> HttpBody for InjectBody<B>
where
    B: HttpBody<Data = Bytes>,
{
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();

        loop {
            if *this.finished {
                return Poll::Ready(this.trailers.take().map(Ok));
            }
            // Once the script is placed there is nothing left to inspect, so
            // the rest of the body is forwarded untouched.
            if this.script.is_none() {
                return this.inner.as_mut().poll_frame(cx);
            }

            let Some(frame) = ready!(this.inner.as_mut().poll_frame(cx)) else {
                *this.finished = true;
                let mut tail = BytesMut::from(std::mem::take(this.carry));
                // A body with no `</head>` at all is either a whole document
                // written without one or a fragment, and only the first should
                // grow a script tag. Appending to an island or action fragment
                // would push several kilobytes into every partial response.
                if end_of_body_is_document(this.document, this.head) {
                    tail.put_slice(&this.script.take().unwrap_or_default());
                }
                if tail.is_empty() {
                    return Poll::Ready(None);
                }
                return Poll::Ready(Some(Ok(Frame::data(tail.freeze()))));
            };

            let frame = match frame {
                Ok(frame) => frame,
                Err(error) => return Poll::Ready(Some(Err(error))),
            };
            let data = match frame.into_data() {
                Ok(data) => data,
                // Trailers arrive after the last data frame, so the script has
                // to go out first and the trailers wait one poll.
                Err(trailers) => {
                    *this.finished = true;
                    *this.trailers = Some(trailers);
                    let mut tail = BytesMut::from(std::mem::take(this.carry));
                    if end_of_body_is_document(this.document, this.head) {
                        tail.put_slice(&this.script.take().unwrap_or_default());
                    }
                    if tail.is_empty() {
                        return Poll::Ready(this.trailers.take().map(Ok));
                    }
                    return Poll::Ready(Some(Ok(Frame::data(tail.freeze()))));
                }
            };

            let mut buffer = BytesMut::with_capacity(this.carry.len() + data.len());
            buffer.put_slice(this.carry);
            buffer.put_slice(&data);
            *this.carry = Bytes::new();

            if this.document.is_none() {
                // Append the newly arrived bytes, not the whole buffer: the
                // carry at its front was already recorded on an earlier pass,
                // and counting it twice corrupts the opening being sniffed.
                let room = DOCUMENT_SNIFF_BYTES.saturating_sub(this.head.len());
                this.head.put_slice(&data[..room.min(data.len())]);
                *this.document = document_verdict(this.head, false);
            }

            if let Some(at) = find_ignore_ascii_case(&buffer, HEAD_CLOSE) {
                let script = this.script.take().unwrap_or_default();
                let mut patched = BytesMut::with_capacity(buffer.len() + script.len());
                patched.put_slice(&buffer[..at]);
                patched.put_slice(&script);
                patched.put_slice(&buffer[at..]);
                return Poll::Ready(Some(Ok(Frame::data(patched.freeze()))));
            }

            // Hold back just enough for a marker straddling this boundary.
            let keep = buffer.len().min(HEAD_CLOSE.len() - 1);
            let split = buffer.len() - keep;
            *this.carry = buffer.split_off(split).freeze();
            if buffer.is_empty() {
                continue;
            }
            return Poll::Ready(Some(Ok(Frame::data(buffer.freeze()))));
        }
    }

    fn is_end_stream(&self) -> bool {
        self.finished && self.trailers.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        // Deliberately unknown. The upstream length no longer describes this
        // body, and reporting an exact one would let hyper frame the response
        // with a `content-length` that is short by the size of the script.
        SizeHint::new()
    }
}

/// Whether a body's opening bytes are the start of a whole HTML document.
///
/// Proa applications also return HTML fragments, for islands and action
/// responses, and those must come back byte for byte.
/// How much of the opening is kept while deciding document versus fragment.
/// Long enough for `<!doctype` behind a generous run of leading whitespace.
const DOCUMENT_SNIFF_BYTES: usize = 64;

/// Whether a body's opening bytes begin a whole HTML document.
///
/// `None` means the answer could still change once more bytes arrive, so a
/// first frame too short to hold the prefix it is starting to spell is never
/// mistaken for a fragment.
fn document_verdict(head: &[u8], ended: bool) -> Option<bool> {
    let start = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head);
    let start = start
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(&[][..], |at| &start[at..]);

    let mut undecided = false;
    for prefix in [&b"<!doctype"[..], b"<html"] {
        let shared = prefix.len().min(start.len());
        if !start[..shared].eq_ignore_ascii_case(&prefix[..shared]) {
            continue;
        }
        if shared == prefix.len() {
            return Some(true);
        }
        // Everything seen so far still spells this prefix.
        undecided = true;
    }

    if undecided && !ended && head.len() < DOCUMENT_SNIFF_BYTES {
        return None;
    }
    Some(false)
}

/// Settle the document question once no more bytes are coming.
///
/// An empty body has no opening to judge and is treated as a document, so a
/// page that renders nothing still gets live reload.
fn end_of_body_is_document(document: &Option<bool>, head: &[u8]) -> bool {
    if let Some(document) = document {
        return *document;
    }
    // An empty body has no opening to judge either way, so a page that renders
    // nothing still gets live reload.
    head.is_empty() || document_verdict(head, true).unwrap_or(true)
}

fn find_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests;
