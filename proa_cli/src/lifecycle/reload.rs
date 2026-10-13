// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Browser live reload for `proa dev`.
//!
//! The dev loop recompiles the site and restarts its server process on every
//! change, so nothing living inside that process can survive long enough to
//! tell an open browser tab what happened. This module keeps the notification
//! channel in the CLI process instead: one small HTTP server that holds a
//! Server-Sent Events stream open per tab and outlives every restart.
//!
//! `proa_framework_axum::serve` injects the matching client script into HTML
//! responses whenever `PROA_DEV_RELOAD_PORT` and `PROA_DEV_RELOAD_TOKEN` are
//! both set, so generated sites need no source changes to participate.
//!
//! The token is what makes the stream safe to run. Build failures carry
//! absolute paths and source snippets, so only a page this dev server actually
//! served may read them, and only the tabs it served may be told to reload.
//!
//! Vite supplies the event vocabulary. A CSS-only edit swaps stylesheets in
//! place with no navigation, anything else is a full reload once the rebuilt
//! server is accepting connections again, and a failed build paints an
//! overlay instead of silently leaving a stale page on screen.

use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    sync::{
        mpsc::{self, RecvTimeoutError, Sender},
        Arc, Mutex, MutexGuard, PoisonError,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

/// The event stream path. `proa_framework_axum::dev_reload` hard-codes the same
/// literal; the two crates share no dependency, so a change here has to be
/// mirrored there.
pub(crate) const DEV_EVENTS_PATH: &str = "/__proa/dev-events";

/// Environment variable carrying the live-reload port to the site's server
/// process.
pub(crate) const RELOAD_PORT_ENV: &str = "PROA_DEV_RELOAD_PORT";

/// Environment variable carrying this dev session's secret. Injection needs
/// both this and the port, so a port left exported in a shell profile cannot
/// switch dev tooling on in an unrelated process.
pub(crate) const RELOAD_TOKEN_ENV: &str = "PROA_DEV_RELOAD_TOKEN";

/// How long a stream sits idle before a comment keep-alive is written. That
/// write is also the liveness check that retires a closed tab's thread.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// Ceiling on one write to a tab. Frames are small and the peer is on this
/// machine, so anything slower is a client that has stopped reading.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Cap on a request head. Live reload talks to browsers over loopback, so
/// anything larger is a client bug or a probe, not a real request.
const MAX_REQUEST_HEAD_BYTES: usize = 8 * 1024;

/// Bytes of entropy in the session secret. Vite uses nine for the same job.
const TOKEN_BYTES: usize = 16;

/// What the dev loop tells open tabs to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReloadEvent {
    /// A change stabilized and work started. Tabs show a progress badge.
    Building,
    /// A new server generation is up and accepting connections.
    Reload,
    /// Stylesheets changed but the server did not, so tabs re-fetch their
    /// stylesheets without navigating and keep all their client state.
    Css,
    /// The rebuild failed. The previous server keeps serving, so the tab
    /// stays usable behind a dismissible overlay.
    Failed(String),
}

impl ReloadEvent {
    const fn name(&self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Reload => "reload",
            Self::Css => "css",
            Self::Failed(_) => "failed",
        }
    }

    fn message(&self) -> Option<&str> {
        match self {
            Self::Failed(message) => Some(message),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct ReloadState {
    generation: u64,
    /// Retained so a tab opened *after* a failed build still gets the
    /// overlay instead of looking healthy.
    failure: Option<String>,
}

/// The live-reload event server. Dropping it does not stop its background
/// threads; they end when the CLI process does.
#[derive(Debug)]
pub(crate) struct ReloadServer {
    port: u16,
    token: Arc<str>,
    #[allow(dead_code)]
    host: Arc<str>,
    clients: Arc<Mutex<Vec<Sender<Arc<str>>>>>,
    state: Arc<Mutex<ReloadState>>,
}

impl ReloadServer {
    /// Bind the event server next to the dev server.
    ///
    /// `requested` is tried first, then an operating-system assigned port.
    /// Nothing scans upward from the request: the ports just above a dev
    /// server are exactly the ones a second Proa site will want for itself.
    pub(crate) fn bind(host: &str, requested: u16) -> Result<Self> {
        let listener = bind_listener(host, requested)?;
        let port = listener
            .local_addr()
            .context("read the live-reload server address")?
            .port();
        let token: Arc<str> = Arc::from(session_token()?);
        let clients: Arc<Mutex<Vec<Sender<Arc<str>>>>> = Arc::new(Mutex::new(Vec::new()));
        let state = Arc::new(Mutex::new(ReloadState::default()));

        let host: Arc<str> = Arc::from(host);
        let accept_clients = Arc::clone(&clients);
        let accept_state = Arc::clone(&state);
        let accept_token = Arc::clone(&token);
        let accept_host = Arc::clone(&host);
        thread::Builder::new()
            .name("proa-dev-reload".into())
            .spawn(move || {
                accept_loop(
                    &listener,
                    &accept_token,
                    &accept_host,
                    &accept_clients,
                    &accept_state,
                )
            })
            .context("start the live-reload server thread")?;

        Ok(Self {
            port,
            token,
            host,
            clients,
            state,
        })
    }

    pub(crate) const fn port(&self) -> u16 {
        self.port
    }

    /// The secret a tab must present to read this session's event stream.
    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    /// Broadcast one event to every connected tab.
    ///
    /// Tabs that have gone away are dropped from the roster here rather than
    /// reported: closing a browser is not a dev-loop failure.
    pub(crate) fn broadcast(&self, event: &ReloadEvent) {
        let frame: Arc<str> = {
            let mut state = lock(&self.state);
            match event {
                // Both outcomes publish a new generation and clear whatever
                // failure the tab was showing.
                ReloadEvent::Reload | ReloadEvent::Css => {
                    state.generation += 1;
                    state.failure = None;
                }
                ReloadEvent::Failed(message) => state.failure = Some(message.clone()),
                // A new cycle makes the previous failure stale. Clearing it
                // here stops a tab opened later from being greeted with an
                // overlay for a build that has already been superseded.
                ReloadEvent::Building => state.failure = None,
            }
            Arc::from(encode_event(
                event.name(),
                event.message(),
                state.generation,
            ))
        };

        lock(&self.clients).retain(|client| client.send(Arc::clone(&frame)).is_ok());
    }

    #[cfg(test)]
    pub(crate) fn client_count(&self) -> usize {
        lock(&self.clients).len()
    }
}

fn bind_listener(host: &str, requested: u16) -> Result<TcpListener> {
    match TcpListener::bind((host, requested)) {
        Ok(listener) => return Ok(listener),
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("bind the live-reload server to {host}:{requested}"))
        }
    }

    let listener = TcpListener::bind((host, 0))
        .with_context(|| format!("bind the live-reload server on {host}"))?;
    let port = listener
        .local_addr()
        .context("read the live-reload server address")?
        .port();
    eprintln!(
        "live reload: port {requested} is in use; listening on {port} instead. Open tabs will not \
         find this port again after a restart, so pin one with PROA_DEV_RELOAD_PORT."
    );
    Ok(listener)
}

/// A per-process secret, so only a page this dev server actually served can
/// read its event stream.
///
/// Without it, any site loaded in the same browser could open the stream and
/// read build failures, which carry absolute paths and source snippets. Vite
/// mints an equivalent token for the same reason.
fn session_token() -> Result<String> {
    use std::fmt::Write as _;

    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| {
        anyhow::anyhow!("draw a live-reload session secret from the system CSPRNG: {error}")
    })?;
    let mut token = String::with_capacity(TOKEN_BYTES * 2);
    for byte in bytes {
        let _ = write!(token, "{byte:02x}");
    }
    Ok(token)
}

fn accept_loop(
    listener: &TcpListener,
    token: &Arc<str>,
    bind_host: &Arc<str>,
    clients: &Arc<Mutex<Vec<Sender<Arc<str>>>>>,
    state: &Arc<Mutex<ReloadState>>,
) {
    for stream in listener.incoming() {
        // One failed accept (a probe that hung up, a transient resource
        // limit) must not take live reload down for the rest of the session.
        let Ok(stream) = stream else { continue };
        let token = Arc::clone(token);
        let bind_host = Arc::clone(bind_host);
        let clients = Arc::clone(clients);
        let state = Arc::clone(state);
        let spawned = thread::Builder::new()
            .name("proa-dev-reload-client".into())
            .spawn(move || {
                let _ = serve_connection(&stream, &token, &bind_host, &clients, &state);
            });
        if spawned.is_err() {
            eprintln!("live reload: could not start a connection thread; dropping this client");
        }
    }
}

fn serve_connection(
    stream: &TcpStream,
    token: &str,
    bind_host: &str,
    clients: &Arc<Mutex<Vec<Sender<Arc<str>>>>>,
    state: &Arc<Mutex<ReloadState>>,
) -> io::Result<()> {
    stream.set_nodelay(true)?;
    // A client that stops reading would otherwise block this thread forever
    // once the socket buffer fills. Time the write out and retire the tab.
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let Some(request) = read_request_head(stream)? else {
        return Ok(());
    };

    // Nothing here answers a cross-origin preflight. `EventSource` issues a
    // simple GET that is never preflighted, so an `OPTIONS` handler would
    // only give an unauthenticated caller a way to confirm what is listening
    // on this port.
    if request.method != "GET" || request.path != DEV_EVENTS_PATH {
        return write_response(stream, "404 Not Found", "proa dev live reload only\n");
    }
    // A `Host` an attacker controls is the DNS-rebinding path onto this
    // stream: point a domain at a public address, let the page load, then
    // re-point it at loopback so the page becomes same-origin with the dev
    // machine. Only names this server was actually asked to answer for pass.
    if !host_is_expected(request.host.as_deref(), bind_host) {
        return write_response(stream, "403 Forbidden", "unexpected Host header\n");
    }
    if !constant_time_eq(request.token.as_deref().unwrap_or_default(), token) {
        // A tab left open from a previous `proa dev`, or from a different
        // site that reused this port, lands here. `EventSource` treats a
        // non-200 as fatal and stops retrying, which is what should happen:
        // that tab must not be reloaded by a server that never served it.
        return write_response(
            stream,
            "403 Forbidden",
            "live reload session token mismatch\n",
        );
    }

    stream_events(stream, clients, state)
}

/// Write a short response, deriving the declared length from the body so the
/// two cannot drift apart.
fn write_response(stream: &TcpStream, status: &str, body: &str) -> io::Result<()> {
    write_all(
        stream,
        format!(
            "HTTP/1.1 {status}\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
}

/// Whether a `Host` header names something this dev server answers for.
///
/// Loopback is always allowed. A server bound to one address also answers for
/// that address. A wildcard bind has to accept whatever name reached it, since
/// the site is deliberately being served to other machines.
fn host_is_expected(host: Option<&str>, bind_host: &str) -> bool {
    let Some(host) = host else {
        // HTTP/1.1 requires `Host`. Something that omits it is not a browser.
        return false;
    };
    let name = host_name(host);
    // Parse rather than pattern-match the text: `127.evil.example` starts with
    // `127.` and is not loopback, which is exactly the name a rebinding attack
    // arrives with.
    if is_loopback_name(name) {
        return true;
    }
    match bind_host.parse::<IpAddr>() {
        Ok(address) if address.is_unspecified() => true,
        _ => name == host_name(bind_host),
    }
}

/// Whether a host names this machine, by address or by reserved name.
fn is_loopback_name(name: &str) -> bool {
    if name == "localhost" || name.ends_with(".localhost") {
        return true;
    }
    let literal = name
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(name);
    literal
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

/// Strip a port from a `Host` value, leaving bracketed IPv6 literals intact.
fn host_name(host: &str) -> &str {
    let host = host.trim();
    if host.starts_with('[') {
        return host
            .split(']')
            .next()
            .map_or(host, |name| &host[..name.len() + 1]);
    }
    host.split(':').next().unwrap_or(host)
}

/// Compare without an early return, so a wrong token cannot be recovered one
/// character at a time from response timing.
fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in left.bytes().zip(right.bytes()) {
        difference |= left ^ right;
    }
    difference == 0
}

fn stream_events(
    stream: &TcpStream,
    clients: &Arc<Mutex<Vec<Sender<Arc<str>>>>>,
    state: &Arc<Mutex<ReloadState>>,
) -> io::Result<()> {
    // `Access-Control-Allow-Origin` is mandatory: the page comes from the
    // site's port and this stream lives on another, so every `EventSource`
    // request here is cross-origin. `X-Accel-Buffering` stops a proxied setup
    // from holding events back until a response that never ends completes.
    write_all(
        stream,
        b"HTTP/1.1 200 OK\r\n\
          Content-Type: text/event-stream\r\n\
          Cache-Control: no-store\r\n\
          Connection: keep-alive\r\n\
          Access-Control-Allow-Origin: *\r\n\
          X-Accel-Buffering: no\r\n\r\n\
          retry: 1000\n\n",
    )?;

    // Register only after the greeting is on the wire, so a tab cannot see a
    // build event before it knows which generation it is looking at.
    // Build the greeting under the lock, write it outside. A tab that has
    // stopped reading must never be able to block a broadcast, which would
    // stall the whole dev loop behind one wedged socket.
    let (sender, receiver) = mpsc::channel::<Arc<str>>();
    let greeting = {
        let state = lock(state);
        let mut greeting = encode_event("hello", None, state.generation);
        if let Some(failure) = &state.failure {
            greeting.push_str(&encode_event("failed", Some(failure), state.generation));
        }
        // Registering while the state is still held is what keeps a tab from
        // seeing an event for a generation its greeting did not mention.
        lock(clients).push(sender);
        greeting
    };
    write_all(stream, greeting.as_bytes())?;

    loop {
        match receiver.recv_timeout(KEEPALIVE_INTERVAL) {
            Ok(frame) => write_all(stream, frame.as_bytes())?,
            // A comment frame is ignored by `EventSource` but still has to
            // reach the socket, which is how a closed tab gets detected.
            Err(RecvTimeoutError::Timeout) => write_all(stream, b": keep-alive\n\n")?,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

#[derive(Debug)]
struct RequestHead {
    method: String,
    path: String,
    token: Option<String>,
    host: Option<String>,
}

impl RequestHead {
    fn parse(line: &str) -> Option<Self> {
        let mut parts = line.split_whitespace();
        let (method, target) = (parts.next()?, parts.next()?);
        let (path, query) = match target.split_once('?') {
            Some((path, query)) => (path, query),
            None => (target, ""),
        };
        Some(Self {
            method: method.to_string(),
            path: path.split('#').next().unwrap_or(path).to_string(),
            token: query_value(query, "token"),
            host: None,
        })
    }
}

/// Match one header by case-insensitive name and return its trimmed value.
fn header_value(line: &str, name: &str) -> Option<String> {
    let (key, value) = line.split_once(':')?;
    key.trim()
        .eq_ignore_ascii_case(name)
        .then(|| value.trim().to_string())
}

fn query_value(query: &str, name: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.split('#').next().unwrap_or(value).to_string())
}

fn read_request_head(stream: &TcpStream) -> io::Result<Option<RequestHead>> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    // Bound the reader rather than the running total: `read_line` grows its
    // buffer until it finds a newline, so a client that never sends one could
    // otherwise make this thread allocate without limit.
    let mut reader = BufReader::new(stream.try_clone()?).take(MAX_REQUEST_HEAD_BYTES as u64);
    let mut request = None;

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }

        if request.is_none() {
            let Some(head) = RequestHead::parse(&line) else {
                return Ok(None);
            };
            request = Some(head);
        } else if let Some(value) = header_value(&line, "host") {
            if let Some(request) = request.as_mut() {
                request.host = Some(value);
            }
        } else if line == "\r\n" || line == "\n" {
            // Clear the timeout: an event stream is idle by design and must
            // not be torn down for having nothing to say.
            stream.set_read_timeout(None)?;
            return Ok(request);
        }
    }
}

fn write_all(mut stream: &TcpStream, bytes: &[u8]) -> io::Result<()> {
    stream.write_all(bytes)?;
    stream.flush()
}

fn encode_event(name: &str, message: Option<&str>, generation: u64) -> String {
    let payload = match message {
        Some(message) => serde_json::json!({ "generation": generation, "message": message }),
        None => serde_json::json!({ "generation": generation }),
    };
    // `serde_json` escapes newlines, so the payload always occupies exactly
    // one `data:` line and cannot forge a frame boundary.
    format!("event: {name}\ndata: {payload}\n\n")
}

/// A poisoned mutex here means a connection thread panicked mid-write. Both
/// guarded values are plain data with no invariant left to corrupt, so
/// recovering keeps live reload working instead of taking down the dev loop.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Wait until a started or restarted server accepts connections.
///
/// Reloading a tab into a connection-refused error page is worse than
/// reloading a beat later, so the dev loop always probes before broadcasting.
pub(crate) fn wait_until_serving(
    host: &str,
    port: u16,
    timeout: Duration,
    still_running: &mut dyn FnMut() -> bool,
) -> bool {
    let targets = probe_addresses(host, port);
    let deadline = Instant::now() + timeout;
    loop {
        if !still_running() {
            return false;
        }
        for target in &targets {
            if TcpStream::connect_timeout(target, Duration::from_millis(250)).is_ok() {
                return still_running();
            }
        }
        // A server that died on boot is never going to answer. Waiting out the
        // full timeout would just delay the message the developer needs.
        if !still_running() || Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Every address the dev server might actually be listening on.
///
/// A bare `127.0.0.1` guess is wrong more often than it looks: `localhost`
/// resolves to `::1` first on most systems, so a site configured with a
/// hostname would be probed on an address it never bound. `0.0.0.0` is the
/// opposite problem, a bind address that is not a destination at all.
fn probe_addresses(host: &str, port: u16) -> Vec<SocketAddr> {
    use std::net::ToSocketAddrs;

    if let Ok(address) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        return if address.is_unspecified() {
            let loopback = match address {
                IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
            };
            vec![SocketAddr::new(loopback, port)]
        } else {
            vec![SocketAddr::new(address, port)]
        };
    }
    match (host, port).to_socket_addrs() {
        Ok(resolved) => {
            let resolved: Vec<SocketAddr> = resolved.collect();
            if resolved.is_empty() {
                vec![SocketAddr::from((Ipv4Addr::LOCALHOST, port))]
            } else {
                resolved
            }
        }
        Err(_) => vec![SocketAddr::from((Ipv4Addr::LOCALHOST, port))],
    }
}

#[cfg(test)]
mod tests;
