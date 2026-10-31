// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Web (HTML) rendering context

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::cell::{Cell, OnceCell, Ref, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use serde::de::DeserializeOwned;

use super::CspNonce;
use crate::render::{
    BoundaryErrorFallbackHandle, DataLoader, DynDataLoader, DynLoaderHandle, LoadError, LoadHints,
    LoadKey, LoadRequest, NoLoader, RenderMode, RequestLocation, RouteParams, StandardDataLoader,
    StreamBoundaryRecorder, StreamChunkFlusher,
};
use crate::{
    shared::output_meter::{
        MeteredWriteBuf, OutputMeterHandle, OutputReservation, ReservedWriteBuf,
    },
    shared::write_buf::{RenderBuffer, WriteBuf, DEFAULT_CHAINED_BUMP_PAGE_SIZE},
    shared::{escape::EscapeDialect, mode::ContentMode},
};

/// Page size used by the standard chained bump writer (4 KiB).
pub const STANDARD_PAGE_SIZE: usize = DEFAULT_CHAINED_BUMP_PAGE_SIZE;

/// Standard sync rendering context: 4 KiB chained bump writer +
/// [`StandardDataLoader`].
///
/// Use this in app code when you don't want to spell out the generics on
/// `WebContext` directly.
pub type StandardWebContext<D = StandardDataLoader> = WebContext<D, RenderBuffer>;

/// Request metadata available while rendering.
///
/// This is deliberately transport-agnostic: framework adapters can project
/// Axum, Workers, or other request types into this small shape without making
/// `proa_core` depend on those runtimes.
#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    method: Option<Method>,
    location: Option<RequestLocation>,
    route_params: Option<RouteParams>,
    headers: Option<HeaderMap>,
    client_ip: Option<String>,
    original_pathname: Option<String>,
    base_path: super::BasePath,
}

fn empty_headers() -> &'static HeaderMap {
    static EMPTY_HEADERS: OnceLock<HeaderMap> = OnceLock::new();
    EMPTY_HEADERS.get_or_init(HeaderMap::new)
}

impl RequestContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Browser-visible mount prefix. Incoming request paths stay unprefixed.
    pub fn base_path(&self) -> &super::BasePath {
        &self.base_path
    }

    pub fn set_base_path(&mut self, base_path: super::BasePath) {
        self.base_path = base_path;
    }

    pub fn with_base_path(mut self, base_path: super::BasePath) -> Self {
        self.set_base_path(base_path);
        self
    }

    /// Compose a browser URL without allocating a string in the render path.
    pub fn url<'a>(&self, path: &'a str) -> super::AppUrl<'a> {
        self.base_path.url(path)
    }

    pub fn method(&self) -> Option<&Method> {
        self.method.as_ref()
    }

    pub fn set_method(&mut self, method: Method) {
        self.method = Some(method);
    }

    pub fn with_method(mut self, method: Method) -> Self {
        self.set_method(method);
        self
    }

    pub fn location(&self) -> Option<&RequestLocation> {
        self.location.as_ref()
    }

    pub fn set_location(&mut self, location: RequestLocation) {
        self.location = Some(location);
    }

    pub fn with_location(mut self, location: RequestLocation) -> Self {
        self.set_location(location);
        self
    }

    pub fn route_params(&self) -> Option<&RouteParams> {
        self.route_params.as_ref()
    }

    pub fn set_route_params(&mut self, route_params: RouteParams) {
        self.route_params = Some(route_params);
    }

    pub fn with_route_params(mut self, route_params: RouteParams) -> Self {
        self.set_route_params(route_params);
        self
    }

    pub fn param(&self, key: &str) -> Option<&str> {
        self.route_params
            .as_ref()
            .and_then(|route_params| route_params.get(key))
    }

    pub fn headers(&self) -> &HeaderMap {
        match self.headers.as_ref() {
            Some(headers) => headers,
            None => empty_headers(),
        }
    }

    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        self.headers.get_or_insert_with(HeaderMap::new)
    }

    pub fn set_headers(&mut self, headers: HeaderMap) {
        self.headers = Some(headers);
    }

    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.set_headers(headers);
        self
    }

    pub fn header(&self, name: &HeaderName) -> Option<&HeaderValue> {
        self.headers.as_ref()?.get(name)
    }

    pub fn user_agent(&self) -> Option<&str> {
        self.headers
            .as_ref()?
            .get(http::header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        let cookies = self
            .headers
            .as_ref()?
            .get(http::header::COOKIE)?
            .to_str()
            .ok()?;
        cookies.split(';').find_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            if key.trim() == name {
                Some(value.trim())
            } else {
                None
            }
        })
    }

    pub fn client_ip(&self) -> Option<&str> {
        self.client_ip.as_deref()
    }

    pub fn set_client_ip(&mut self, client_ip: impl Into<String>) {
        self.client_ip = Some(client_ip.into());
    }

    pub fn with_client_ip(mut self, client_ip: impl Into<String>) -> Self {
        self.set_client_ip(client_ip);
        self
    }

    pub fn original_pathname(&self) -> Option<&str> {
        self.original_pathname.as_deref()
    }

    pub fn set_original_pathname(&mut self, original_pathname: impl Into<String>) {
        self.original_pathname = Some(original_pathname.into());
    }

    pub fn with_original_pathname(mut self, original_pathname: impl Into<String>) -> Self {
        self.set_original_pathname(original_pathname);
        self
    }
}

#[derive(Debug, Clone, Default)]
struct ResponseState {
    status: Option<StatusCode>,
    headers: HeaderMap,
    streaming_started: bool,
    /// Counts chunks that were actually handed to the stream flusher.
    ///
    /// `streaming_started` is a sticky latch set once, at the top of a
    /// streamed response, so it cannot answer "did any markup leave this
    /// process since point X". This counter can: it advances only when
    /// bytes are shipped, so two reads that agree prove nothing was sent
    /// between them. Rollback of rewound render state (rsjs shared-state
    /// seeds) is gated on that proof.
    stream_flush_generation: u64,
}

/// Response metadata staged during render.
///
/// Framework adapters can read this after rendering and merge the staged
/// headers/status into the transport response. Header mutation is explicitly
/// tracked so streaming integrations can prevent late mutation after the
/// response head has been committed.
#[derive(Debug, Clone, Default)]
pub struct ResponseContext {
    state: ResponseState,
}

impl ResponseContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.state.status.is_none()
            && self.state.headers.is_empty()
            && !self.state.streaming_started
    }

    pub fn status(&self) -> Option<StatusCode> {
        self.state.status
    }

    pub fn set_status(&mut self, status: StatusCode) -> bool {
        if self.state.streaming_started {
            return false;
        }
        self.state.status = Some(status);
        true
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.state.headers
    }

    pub fn insert_header(&mut self, name: HeaderName, value: HeaderValue) -> bool {
        if self.state.streaming_started {
            return false;
        }
        self.state.headers.insert(name, value);
        true
    }

    pub fn append_header(&mut self, name: HeaderName, value: HeaderValue) -> bool {
        if self.state.streaming_started {
            return false;
        }
        self.state.headers.append(name, value);
        true
    }

    pub fn append_set_cookie(&mut self, value: HeaderValue) -> bool {
        self.append_header(http::header::SET_COOKIE, value)
    }

    pub fn streaming_started(&self) -> bool {
        self.state.streaming_started
    }

    pub fn mark_streaming_started(&mut self) {
        self.state.streaming_started = true;
    }

    /// How many chunks have been shipped through the stream flusher.
    ///
    /// The absolute value carries no meaning; only the comparison of two
    /// reads does. Equal reads prove no markup left the process in between,
    /// which is what makes rewinding render state safe. Cold-parts safe: a
    /// response that never streamed reports 0 without materializing state.
    pub fn stream_flush_generation(&self) -> u64 {
        self.state.stream_flush_generation
    }

    /// Record that a chunk of markup was handed to the stream flusher.
    ///
    /// Called by `WebContext::flush_stream_chunk` after the bytes are
    /// unrecallable. Also latches `streaming_started`, since shipping a
    /// chunk commits the response head.
    pub fn mark_stream_chunk_flushed(&mut self) {
        self.state.streaming_started = true;
        self.state.stream_flush_generation = self.state.stream_flush_generation.wrapping_add(1);
    }

    pub fn can_mutate_headers(&self) -> bool {
        !self.streaming_started()
    }

    pub(super) fn fork_for_detached(&self) -> Self {
        Self {
            state: ResponseState {
                status: self.state.status,
                headers: if self.state.streaming_started {
                    HeaderMap::new()
                } else {
                    self.state.headers.clone()
                },
                streaming_started: true,
                stream_flush_generation: self.state.stream_flush_generation,
            },
        }
    }
}

/// One page-level head entry recorded during render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadEntry {
    Meta { name: String, content: String },
    Link { rel: String, href: String },
    RawHtml(String),
}

#[derive(Debug, Clone, Default)]
struct HeadState {
    title: Option<String>,
    entries: Vec<HeadEntry>,
}

// Managed state is cold; the default context keeps the existing storage shape.
#[derive(Debug, Clone)]
enum HeadStorage {
    Legacy(HeadState),
    Managed(Box<ManagedHeadState>),
}
#[derive(Debug, Clone)]
struct ManagedHeadState {
    state: HeadState,
    document: crate::managed_head::ManagedHead,
}
impl Default for HeadStorage {
    fn default() -> Self {
        Self::Legacy(HeadState::default())
    }
}
impl std::ops::Deref for HeadStorage {
    type Target = HeadState;
    fn deref(&self) -> &HeadState {
        match self {
            Self::Legacy(s) => s,
            Self::Managed(m) => &m.state,
        }
    }
}
impl std::ops::DerefMut for HeadStorage {
    fn deref_mut(&mut self) -> &mut HeadState {
        match self {
            Self::Legacy(s) => s,
            Self::Managed(m) => &mut m.state,
        }
    }
}

/// Accumulates page-level head metadata and resource hints during render.
#[derive(Debug, Clone, Default)]
pub struct HeadContext {
    state: HeadStorage,
}

/// Checked writer view. Replacing this view never replaces document state.
pub struct HeadWriter<'a>(&'a mut HeadContext);
macro_rules! metadata_setter {
    ($set:ident, $clear:ident, $field:ident, $ty:ty) => {
        #[doc = concat!("Set `", stringify!($field), "` using the checked metadata patch API.")]
        pub fn $set(&mut self, value: $ty) -> Result<(), crate::WriteError> {
            self.metadata(crate::HeadMetadata {
                $field: value.into(),
                ..Default::default()
            })
        }
        #[doc = concat!("Remove inherited `", stringify!($field), "` metadata.")]
        pub fn $clear(&mut self) -> Result<(), crate::WriteError> {
            self.metadata(crate::HeadMetadata {
                $field: crate::HeadValue::Clear,
                ..Default::default()
            })
        }
    };
}
macro_rules! metadata_list_setter {
    ($set:ident, $clear:ident, $field:ident, $ty:ty) => {
        #[doc = concat!("Replace the inherited `", stringify!($field), "` list.")]
        pub fn $set(
            &mut self,
            values: impl IntoIterator<Item = $ty>,
        ) -> Result<(), crate::WriteError> {
            // proa-perf-ok: bounded lists are constructed only during managed head preparation.
            let values: Vec<_> = values.into_iter().take(129).collect();
            self.metadata(crate::HeadMetadata {
                $field: values.into(),
                ..Default::default()
            })
        }
        #[doc = concat!("Remove the inherited `", stringify!($field), "` list.")]
        pub fn $clear(&mut self) -> Result<(), crate::WriteError> {
            self.metadata(crate::HeadMetadata {
                $field: crate::HeadValue::Clear,
                ..Default::default()
            })
        }
    };
}
impl<'a> HeadWriter<'a> {
    metadata_setter!(twitter, clear_twitter, twitter, crate::Twitter);
    metadata_setter!(robots, clear_robots, robots, crate::Robots);
    metadata_setter!(googlebot, clear_googlebot, googlebot, crate::Robots);
    metadata_setter!(referrer, clear_referrer, referrer, crate::ReferrerPolicy);
    metadata_list_setter!(icons, clear_icons, icons, crate::HeadIcon);
    metadata_list_setter!(
        alternates,
        clear_alternates,
        alternates,
        crate::HeadAlternate
    );
    metadata_list_setter!(authors, clear_authors, authors, crate::HeadAuthor);
    metadata_list_setter!(
        verification,
        clear_verification,
        verification,
        crate::HeadVerification
    );

    /// Set a manifest URL resolved against the trusted public origin and mount.
    pub fn manifest(
        &mut self,
        value: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<(), crate::WriteError> {
        self.metadata(crate::HeadMetadata {
            manifest: value.into().into(),
            ..Default::default()
        })
    }
    pub fn clear_manifest(&mut self) -> Result<(), crate::WriteError> {
        self.metadata(crate::HeadMetadata {
            manifest: crate::HeadValue::Clear,
            ..Default::default()
        })
    }
    pub fn application_name(
        &mut self,
        value: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<(), crate::WriteError> {
        self.meta("application-name", value)
    }
    pub fn creator(
        &mut self,
        value: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<(), crate::WriteError> {
        self.meta("creator", value)
    }
    pub fn publisher(
        &mut self,
        value: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<(), crate::WriteError> {
        self.meta("publisher", value)
    }
    pub fn keywords<T: Into<std::borrow::Cow<'static, str>>>(
        &mut self,
        values: impl IntoIterator<Item = T>,
    ) -> Result<(), crate::WriteError> {
        // proa-perf-ok: bounded keyword composition is part of head preparation.
        let values: Vec<_> = values.into_iter().take(129).map(Into::into).collect();
        self.metadata(crate::HeadMetadata {
            keywords: values.into(),
            ..Default::default()
        })
    }

    fn managed(&self) -> Result<crate::managed_head::ManagedHead, crate::WriteError> {
        self.0
            .document()
            .ok_or_else(|| crate::WriteError::render("managed head setup required"))
    }
    /// Apply a typed metadata patch atomically. Defaults inherit; `HeadValue::Clear` removes.
    // Keep literal patch discriminants visible so omitted fields disappear
    // from the caller's preparation path rather than copying a full object.
    #[inline(always)]
    pub fn metadata(&mut self, mut metadata: crate::HeadMetadata) -> Result<(), crate::WriteError> {
        self.managed()?.metadata_mut(&mut metadata)
    }
    pub fn meta(
        &mut self,
        name: &str,
        value: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<(), crate::WriteError> {
        self.managed()?.meta(name, Some(value.into()))
    }
    pub fn description(
        &mut self,
        value: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<(), crate::WriteError> {
        self.meta("description", value)
    }
    pub fn canonical(&mut self, value: &str) -> Result<(), crate::WriteError> {
        self.managed()?.canonical(value)
    }
    pub fn module<K: Eq + Send + Sync + 'static>(
        &mut self,
        key: K,
        value: &str,
    ) -> Result<(), crate::WriteError> {
        self.managed()?
            .resource(crate::head_fields::ResourceFamily::Module, key, Some(value))
    }
    pub fn stylesheet<K: Eq + Send + Sync + 'static>(
        &mut self,
        key: K,
        value: &str,
    ) -> Result<(), crate::WriteError> {
        self.managed()?.resource(
            crate::head_fields::ResourceFamily::Stylesheet,
            key,
            Some(value),
        )
    }
    pub fn remove_module<K: Eq + Send + Sync + 'static>(
        &mut self,
        key: K,
    ) -> Result<(), crate::WriteError> {
        self.managed()?
            .resource(crate::head_fields::ResourceFamily::Module, key, None)
    }

    pub fn absolute_title(
        &mut self,
        value: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<(), crate::WriteError> {
        self.managed()?.absolute_title(value.into())
    }
    pub fn clear_title(&mut self) -> Result<(), crate::WriteError> {
        self.managed()?.clear_title()
    }
    pub fn title_template(
        &mut self,
        value: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<(), crate::WriteError> {
        self.managed()?.title_template(Some(value.into()))
    }
    pub fn clear_title_template(&mut self) -> Result<(), crate::WriteError> {
        self.managed()?.title_template(None)
    }
    pub fn clear_canonical(&mut self) -> Result<(), crate::WriteError> {
        self.managed()?.clear_canonical()
    }
    pub fn remove_meta(&mut self, name: &str) -> Result<(), crate::WriteError> {
        self.managed()?.meta(name, None)
    }
    pub fn clear_description(&mut self) -> Result<(), crate::WriteError> {
        self.remove_meta("description")
    }
    pub fn open_graph(
        &mut self,
        patch: crate::head_fields::OpenGraph,
    ) -> Result<(), crate::WriteError> {
        self.managed()?.open_graph(patch)
    }
    pub fn remove_stylesheet<K: Eq + Send + Sync + 'static>(
        &mut self,
        key: K,
    ) -> Result<(), crate::WriteError> {
        self.managed()?
            .resource(crate::head_fields::ResourceFamily::Stylesheet, key, None)
    }

    pub(crate) fn new(head: &'a mut HeadContext) -> Self {
        Self(head)
    }
    pub fn title(
        &mut self,
        title: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<(), crate::WriteError> {
        self.managed()?.title(title.into())
    }
    #[doc(hidden)]
    pub fn manage(&mut self, document: crate::managed_head::ManagedHead) {
        self.0.manage(document);
    }
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.0.set_title(title);
    }
    pub fn push_meta(&mut self, name: impl Into<String>, content: impl Into<String>) {
        self.0.push_meta(name, content);
    }
    pub fn push_link(&mut self, rel: impl Into<String>, href: impl Into<String>) {
        self.0.push_link(rel, href);
    }
    pub fn push_raw_html(&mut self, html: impl Into<String>) {
        self.0.push_raw_html(html);
    }
}
impl std::ops::Deref for HeadWriter<'_> {
    type Target = HeadContext;
    fn deref(&self) -> &HeadContext {
        self.0
    }
}

impl HeadContext {
    pub fn detached(&self) -> Self {
        match &self.state {
            HeadStorage::Legacy(state) => Self {
                state: HeadStorage::Legacy(state.clone()),
            },
            HeadStorage::Managed(m) => Self {
                // proa-perf-ok: only a managed detached context allocates this state
                state: HeadStorage::Managed(Box::new(ManagedHeadState {
                    state: m.state.clone(),
                    document: m.document.detached(),
                })),
            },
        }
    }
    pub fn manage(&mut self, document: crate::managed_head::ManagedHead) {
        if let Some(current) = self.document() {
            current.reject_replacement();
            return;
        }
        // proa-perf-ok: managed documents opt into cold state allocation
        self.state = HeadStorage::Managed(Box::new(ManagedHeadState {
            state: HeadState::default(),
            document,
        }));
    }
    #[doc(hidden)]
    pub fn document(&self) -> Option<crate::managed_head::ManagedHead> {
        match &self.state {
            HeadStorage::Managed(m) => Some(m.document.clone()),
            _ => None,
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        if let Some(document) = self.document() {
            let _ = document.fail::<()>(crate::managed_head::HeadError::LegacyMutation);
            return;
        }
        self.state.title = Some(title.into());
    }

    pub fn title(&self) -> Option<String> {
        self.document()
            .map_or_else(|| self.state.title.clone(), |d| d.resolved_title())
    }

    pub fn push_meta(&mut self, name: impl Into<String>, content: impl Into<String>) {
        if let Some(document) = self.document() {
            let _ = document.fail::<()>(crate::managed_head::HeadError::LegacyMutation);
            return;
        }
        self.state.entries.push(HeadEntry::Meta {
            name: name.into(),
            content: content.into(),
        });
    }

    pub fn push_link(&mut self, rel: impl Into<String>, href: impl Into<String>) {
        if let Some(document) = self.document() {
            let _ = document.fail::<()>(crate::managed_head::HeadError::LegacyMutation);
            return;
        }
        self.state.entries.push(HeadEntry::Link {
            rel: rel.into(),
            href: href.into(),
        });
    }

    pub fn push_raw_html(&mut self, html: impl Into<String>) {
        if let Some(document) = self.document() {
            let _ = document.fail::<()>(crate::managed_head::HeadError::LegacyMutation);
            return;
        }
        self.state.entries.push(HeadEntry::RawHtml(html.into()));
    }

    pub fn entries(&self) -> Vec<HeadEntry> {
        self.state.entries.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheDependency {
    Tag(String),
    Path(String),
}

#[derive(Debug, Clone, Default)]
struct CacheTraceState {
    ttl: Option<Duration>,
    dependencies: Vec<CacheDependency>,
    dynamic_reasons: Vec<String>,
}

/// Records cache dependencies and dynamic-rendering reasons discovered while
/// rendering a response.
#[derive(Debug, Clone, Default)]
pub struct CacheTrace {
    state: CacheTraceState,
}

impl CacheTrace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_ttl(&mut self, ttl: Duration) {
        self.state.ttl = Some(ttl);
    }

    pub fn ttl(&self) -> Option<Duration> {
        self.state.ttl
    }

    pub fn depend_tag(&mut self, tag: impl Into<String>) {
        self.state
            .dependencies
            .push(CacheDependency::Tag(tag.into()));
    }

    pub fn depend_path(&mut self, path: impl Into<String>) {
        self.state
            .dependencies
            .push(CacheDependency::Path(path.into()));
    }

    pub fn mark_dynamic(&mut self, reason: impl Into<String>) {
        self.state.dynamic_reasons.push(reason.into());
    }

    pub fn dependencies(&self) -> Vec<CacheDependency> {
        self.state.dependencies.clone()
    }

    pub fn dynamic_reasons(&self) -> Vec<String> {
        self.state.dynamic_reasons.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryEvent {
    pub name: String,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
struct RenderTelemetryState {
    started_at: Instant,
    request_id: Option<String>,
    events: Vec<TelemetryEvent>,
}

impl RenderTelemetryState {
    fn started(at: Instant) -> Self {
        Self {
            started_at: at,
            request_id: None,
            events: Vec::new(),
        }
    }
}

/// Per-render observability state.
#[derive(Debug, Clone)]
pub struct RenderTelemetry {
    state: RenderTelemetryState,
}

impl Default for RenderTelemetry {
    fn default() -> Self {
        Self {
            state: RenderTelemetryState::started(Instant::now()),
        }
    }
}

impl RenderTelemetry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_request_id(&mut self, request_id: impl Into<String>) {
        self.state.request_id = Some(request_id.into());
    }

    pub fn request_id(&self) -> Option<String> {
        self.state.request_id.clone()
    }

    pub fn elapsed(&self) -> Duration {
        self.state.started_at.elapsed()
    }

    pub fn record_event(&mut self, name: impl Into<String>) {
        let elapsed = self.state.started_at.elapsed();
        self.state.events.push(TelemetryEvent {
            name: name.into(),
            elapsed,
        });
    }

    pub fn events(&self) -> Vec<TelemetryEvent> {
        self.state.events.clone()
    }
}

/// Map backing [`Locals`].
type LocalsMap = HashMap<TypeId, Arc<dyn Any + Send + Sync>>;

/// Typed request-local extension storage.
#[derive(Clone, Default)]
pub struct Locals {
    values: LocalsMap,
}

impl Locals {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<T>(&mut self, value: T)
    where
        T: Send + Sync + 'static,
    {
        self.values.insert(TypeId::of::<T>(), Arc::new(value));
    }

    pub fn insert_arc<T>(&mut self, value: Arc<T>)
    where
        T: Send + Sync + 'static,
    {
        self.values.insert(TypeId::of::<T>(), value);
    }

    pub fn get<T>(&self) -> Option<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.values
            .get(&TypeId::of::<T>())
            .cloned()
            .and_then(|value| value.downcast::<T>().ok())
    }

    pub fn contains<T>(&self) -> bool
    where
        T: Send + Sync + 'static,
    {
        self.values.contains_key(&TypeId::of::<T>())
    }

    pub fn remove<T>(&mut self) -> Option<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.values
            .remove(&TypeId::of::<T>())
            .and_then(|value| value.downcast::<T>().ok())
    }
}

impl fmt::Debug for Locals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Locals")
            .field("len", &self.values.len())
            .finish()
    }
}

/// One page-level rsjs query seed captured during server rendering.
///
/// The JSON fragments are already encoded for inline-script-safe assembly by
/// the rsjs macro. Keeping them owned lets an artifact snapshot travel with a
/// detached streaming boundary or a cached rendered fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsjsQuerySeedArtifact {
    key_json: String,
    state_json: String,
}

impl RsjsQuerySeedArtifact {
    pub fn key_json(&self) -> &str {
        &self.key_json
    }

    pub fn state_json(&self) -> &str {
        &self.state_json
    }
}

/// One response-local SharedState provider seed made live by a committed
/// consumer.
///
/// Providers are registered separately from publication. This artifact is
/// recorded only when generated island code acquires the exact lexical scope,
/// so an unused provider never exposes its value merely because its server
/// shell rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsjsSharedStateSeedArtifact {
    scope_id: u64,
    type_key: &'static str,
    schema_hash: &'static str,
    rust_type: &'static str,
    wide_paths_json: Cow<'static, str>,
    state_json: String,
}

impl RsjsSharedStateSeedArtifact {
    pub fn scope_id(&self) -> u64 {
        self.scope_id
    }

    pub fn type_key(&self) -> &'static str {
        self.type_key
    }

    pub fn schema_hash(&self) -> &'static str {
        self.schema_hash
    }

    pub fn rust_type(&self) -> &'static str {
        self.rust_type
    }

    pub fn state_json(&self) -> &str {
        &self.state_json
    }
}

/// Deterministic, transportable rsjs artifacts recorded by a rendered subtree.
///
/// This type deliberately contains no rsjs manifest data, URLs, or CSP state:
/// `proa_core` records only response facts. Framework adapters combine the
/// rendered island ids with their linked manifest and apply request-specific
/// delivery policy later.
///
/// All collections are sorted and deduplicated. The same type represents a
/// complete response snapshot or a delta returned by [`Self::delta_since`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RsjsRenderArtifacts {
    rendered_islands: Vec<&'static str>,
    query_seeds: Vec<RsjsQuerySeedArtifact>,
    shared_state_seeds: Vec<Arc<RsjsSharedStateSeedArtifact>>,
}

/// Deterministic browser-runtime artifacts recorded by a rendered subtree.
///
/// React island names share the same request-local ownership transactions as
/// rsjs artifacts. A detached Suspense branch therefore publishes both sets
/// together only after its HTML is ready, while cancellation and error
/// fallback rollback discard both sets together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderArtifacts {
    /// rsjs modules and serialized seed state required by the subtree.
    pub rsjs: RsjsRenderArtifacts,
    /// React island manifest names used by the subtree, sorted and deduplicated.
    pub react_islands: Vec<String>,
}

impl RenderArtifacts {
    /// Whether this snapshot carries no browser-runtime work.
    pub fn is_empty(&self) -> bool {
        self.rsjs.is_empty() && self.react_islands.is_empty()
    }

    /// Merge another committed branch snapshot into this owned snapshot.
    ///
    /// This is used by the send-safe render context, where detached branches
    /// own their mutable deltas instead of sharing the local `Rc` transaction
    /// ledger. The merge remains deterministic across completion order.
    #[doc(hidden)]
    pub fn merge_committed(&mut self, other: Self) {
        self.rsjs.merge_committed(other.rsjs);
        self.react_islands.extend(other.react_islands);
        self.react_islands.sort_unstable();
        self.react_islands.dedup();
    }

    #[doc(hidden)]
    pub fn record_react_island(&mut self, name: &str) {
        match self
            .react_islands
            .binary_search_by(|candidate| candidate.as_str().cmp(name))
        {
            Ok(_) => {}
            Err(index) => self.react_islands.insert(index, name.to_owned()),
        }
    }
}

impl RsjsRenderArtifacts {
    /// Whether this snapshot carries no browser-runtime or seed work.
    pub fn is_empty(&self) -> bool {
        self.rendered_islands.is_empty()
            && self.query_seeds.is_empty()
            && self.shared_state_seeds.is_empty()
    }

    #[doc(hidden)]
    pub fn record_island(&mut self, asset_id: &'static str) {
        if let Err(index) = self.rendered_islands.binary_search(&asset_id) {
            self.rendered_islands.insert(index, asset_id);
        }
    }

    #[doc(hidden)]
    pub fn record_query_seed_json(&mut self, key_json: &str, state_json: &str) {
        match self
            .query_seeds
            .binary_search_by(|seed| seed.key_json.as_str().cmp(key_json))
        {
            Ok(_) => {}
            Err(index) => self.query_seeds.insert(
                index,
                RsjsQuerySeedArtifact {
                    key_json: key_json.to_owned(),
                    state_json: state_json.to_owned(),
                },
            ),
        }
    }

    #[doc(hidden)]
    pub fn merge_committed(&mut self, other: Self) {
        for island in other.rendered_islands {
            self.record_island(island);
        }
        for seed in other.query_seeds {
            match self
                .query_seeds
                .binary_search_by(|candidate| candidate.key_json.cmp(&seed.key_json))
            {
                Ok(index) if seed.state_json < self.query_seeds[index].state_json => {
                    self.query_seeds[index] = seed;
                }
                Ok(_) => {}
                Err(index) => self.query_seeds.insert(index, seed),
            }
        }
        for seed in other.shared_state_seeds {
            match self
                .shared_state_seeds
                .binary_search_by_key(&seed.scope_id, |candidate| candidate.scope_id)
            {
                Ok(_) => {}
                Err(index) => self.shared_state_seeds.insert(index, seed),
            }
        }
    }

    /// Hydrated island asset ids, in deterministic lexical order.
    pub fn rendered_islands(&self) -> impl ExactSizeIterator<Item = &'static str> + '_ {
        self.rendered_islands.iter().copied()
    }

    /// Page-level query seeds, sorted by their serialized structural key.
    pub fn query_seeds(&self) -> &[RsjsQuerySeedArtifact] {
        &self.query_seeds
    }

    /// SharedState provider seeds made live by committed consumers, ordered by
    /// response-local provider scope.
    pub fn shared_state_seeds(
        &self,
    ) -> impl ExactSizeIterator<Item = &RsjsSharedStateSeedArtifact> {
        self.shared_state_seeds.iter().map(Arc::as_ref)
    }

    /// Return the semantic entries present in `self` but absent from `baseline`.
    ///
    /// Deltas are content-keyed rather than cursor-based, so they remain valid
    /// when independently completed streaming boundaries are observed in a
    /// different order. Query seeds are located by `key_json`, matching the
    /// browser query cache, but a changed winning `state_json` is emitted as a
    /// replacement for that key.
    pub fn delta_since(&self, baseline: &Self) -> Self {
        let rendered_islands = self
            .rendered_islands
            .iter()
            .copied()
            .filter(|asset_id| baseline.rendered_islands.binary_search(asset_id).is_err())
            .collect();
        let query_seeds = self
            .query_seeds
            .iter()
            .filter(|seed| {
                match baseline.query_seeds.binary_search_by(|candidate| {
                    candidate.key_json.as_str().cmp(seed.key_json.as_str())
                }) {
                    Ok(index) => baseline.query_seeds[index].state_json != seed.state_json,
                    Err(_) => true,
                }
            })
            .cloned()
            .collect();
        let shared_state_seeds = self
            .shared_state_seeds
            .iter()
            .filter(|seed| {
                match baseline
                    .shared_state_seeds
                    .binary_search_by_key(&seed.scope_id, |candidate| candidate.scope_id)
                {
                    Ok(index) => baseline.shared_state_seeds[index].as_ref() != seed.as_ref(),
                    Err(_) => true,
                }
            })
            .cloned()
            .collect();
        Self {
            rendered_islands,
            query_seeds,
            shared_state_seeds,
        }
    }

    /// Assemble the page-level inert query-seed table payload.
    ///
    /// Framework finalizers use this snapshot method instead of consulting a
    /// live `WebContext`, which is important for detached streaming boundaries.
    pub fn query_seed_table_json(&self) -> Option<String> {
        query_seed_table_json_from_artifacts(&self.query_seeds)
    }

    /// Assemble the inert SharedState seed-table payload for the exact
    /// provider scopes acquired by this committed artifact snapshot.
    pub fn shared_state_seed_table_json(&self) -> Option<String> {
        shared_state_seed_table_json_from_artifacts(&self.shared_state_seeds)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RsjsArtifactTransactionState {
    Open,
    Committed,
    RolledBack,
}

#[derive(Debug)]
struct RsjsArtifactTransaction {
    parent: Option<Rc<RsjsArtifactTransaction>>,
    state: Cell<RsjsArtifactTransactionState>,
}

impl RsjsArtifactTransaction {
    fn new(parent: Option<Rc<Self>>) -> Rc<Self> {
        Rc::new(Self {
            parent,
            state: Cell::new(RsjsArtifactTransactionState::Open),
        })
    }
}

#[derive(Debug, Clone)]
struct RsjsIslandArtifactContribution {
    asset_id: &'static str,
    owner: Option<Rc<RsjsArtifactTransaction>>,
}

#[derive(Debug, Clone)]
struct ReactIslandArtifactContribution {
    name: String,
    owner: Option<Rc<RsjsArtifactTransaction>>,
}

#[derive(Debug, Clone)]
struct RsjsQuerySeedContribution {
    seed: RsjsQuerySeedArtifact,
    owner: Option<Rc<RsjsArtifactTransaction>>,
}

#[derive(Debug, Clone)]
struct RsjsSharedStateSeedContribution {
    seed: Arc<RsjsSharedStateSeedArtifact>,
    owner: Option<Rc<RsjsArtifactTransaction>>,
}

#[derive(Debug, Clone)]
enum ResolvedRsjsArtifactOwner {
    Root,
    Open(Rc<RsjsArtifactTransaction>),
    RolledBack,
}

fn resolve_rsjs_artifact_owner(
    mut owner: Option<Rc<RsjsArtifactTransaction>>,
) -> ResolvedRsjsArtifactOwner {
    let mut open = None;
    while let Some(transaction) = owner {
        match transaction.state.get() {
            RsjsArtifactTransactionState::RolledBack => {
                return ResolvedRsjsArtifactOwner::RolledBack;
            }
            RsjsArtifactTransactionState::Committed => {
                owner = transaction.parent.clone();
            }
            RsjsArtifactTransactionState::Open => {
                if open.is_none() {
                    open = Some(Rc::clone(&transaction));
                }
                owner = transaction.parent.clone();
            }
        }
    }
    match open {
        Some(transaction) => ResolvedRsjsArtifactOwner::Open(transaction),
        None => ResolvedRsjsArtifactOwner::Root,
    }
}

fn same_rsjs_artifact_owner(
    left: &Option<Rc<RsjsArtifactTransaction>>,
    right: &Option<Rc<RsjsArtifactTransaction>>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => Rc::ptr_eq(left, right),
        _ => false,
    }
}

fn owner_resolves_to_transaction(
    owner: Option<Rc<RsjsArtifactTransaction>>,
    transaction: &Rc<RsjsArtifactTransaction>,
) -> bool {
    matches!(
        resolve_rsjs_artifact_owner(owner),
        ResolvedRsjsArtifactOwner::Open(resolved)
            if Rc::ptr_eq(&resolved, transaction)
    )
}

#[derive(Debug, Default)]
struct RsjsArtifactPage {
    islands: HashSet<&'static str>,
    island_contributions: Vec<RsjsIslandArtifactContribution>,
    react_island_contributions: Vec<ReactIslandArtifactContribution>,
    query_seed_contributions: Vec<RsjsQuerySeedContribution>,
    shared_state_seed_contributions: Vec<RsjsSharedStateSeedContribution>,
    next_react_island_id: u64,
}

impl RsjsArtifactPage {
    fn record_island(
        &mut self,
        asset_id: &'static str,
        owner: Option<Rc<RsjsArtifactTransaction>>,
    ) {
        if matches!(
            resolve_rsjs_artifact_owner(owner.clone()),
            ResolvedRsjsArtifactOwner::RolledBack
        ) {
            return;
        }
        if self.island_contributions.iter().any(|contribution| {
            contribution.asset_id == asset_id
                && same_rsjs_artifact_owner(&contribution.owner, &owner)
        }) {
            return;
        }
        self.island_contributions
            .push(RsjsIslandArtifactContribution { asset_id, owner });
        self.islands.insert(asset_id);
    }

    fn record_react_island(&mut self, name: &str, owner: Option<Rc<RsjsArtifactTransaction>>) {
        if matches!(
            resolve_rsjs_artifact_owner(owner.clone()),
            ResolvedRsjsArtifactOwner::RolledBack
        ) {
            return;
        }
        if self.react_island_contributions.iter().any(|contribution| {
            contribution.name == name && same_rsjs_artifact_owner(&contribution.owner, &owner)
        }) {
            return;
        }
        self.react_island_contributions
            .push(ReactIslandArtifactContribution {
                name: name.to_owned(),
                owner,
            });
    }

    fn next_react_island_id(&mut self) -> u64 {
        let id = self.next_react_island_id;
        self.next_react_island_id = self.next_react_island_id.wrapping_add(1);
        id
    }

    fn record_query_seed(
        &mut self,
        seed: RsjsQuerySeedArtifact,
        owner: Option<Rc<RsjsArtifactTransaction>>,
    ) {
        if matches!(
            resolve_rsjs_artifact_owner(owner.clone()),
            ResolvedRsjsArtifactOwner::RolledBack
        ) {
            return;
        }
        if self.query_seed_contributions.iter().any(|contribution| {
            contribution.seed.key_json == seed.key_json
                && same_rsjs_artifact_owner(&contribution.owner, &owner)
        }) {
            return;
        }
        self.query_seed_contributions
            .push(RsjsQuerySeedContribution { seed, owner });
    }

    fn validate_shared_state_seed(
        &self,
        seed: &RsjsSharedStateSeedArtifact,
    ) -> Result<(), crate::WriteError> {
        let conflict = self
            .shared_state_seed_contributions
            .iter()
            .filter(|contribution| {
                !matches!(
                    resolve_rsjs_artifact_owner(contribution.owner.clone()),
                    ResolvedRsjsArtifactOwner::RolledBack
                )
            })
            .find(|contribution| {
                contribution.seed.scope_id == seed.scope_id && contribution.seed.as_ref() != seed
            });
        if let Some(conflict) = conflict {
            return Err(crate::WriteError::render(format!(
                "rsjs/shared-state-seed-conflict: provider scope {} was already recorded as \
                 `{}` ({}) with a different canonical seed and cannot also be published as \
                 `{}` ({}); cached fragments must resolve against the exact provider instance \
                 established by the current response",
                seed.scope_id,
                conflict.seed.type_key,
                conflict.seed.schema_hash,
                seed.type_key,
                seed.schema_hash,
            )));
        }
        Ok(())
    }

    fn record_shared_state_seed(
        &mut self,
        seed: Arc<RsjsSharedStateSeedArtifact>,
        owner: Option<Rc<RsjsArtifactTransaction>>,
    ) -> Result<(), crate::WriteError> {
        if matches!(
            resolve_rsjs_artifact_owner(owner.clone()),
            ResolvedRsjsArtifactOwner::RolledBack
        ) {
            return Ok(());
        }
        self.validate_shared_state_seed(seed.as_ref())?;
        if self
            .shared_state_seed_contributions
            .iter()
            .any(|contribution| {
                contribution.seed.as_ref() == seed.as_ref()
                    && same_rsjs_artifact_owner(&contribution.owner, &owner)
            })
        {
            return Ok(());
        }
        self.shared_state_seed_contributions
            .push(RsjsSharedStateSeedContribution { seed, owner });
        Ok(())
    }

    fn snapshot(&self) -> RsjsRenderArtifacts {
        self.snapshot_matching(|owner| {
            matches!(
                resolve_rsjs_artifact_owner(owner),
                ResolvedRsjsArtifactOwner::Root
            )
        })
    }

    fn render_snapshot(&self) -> RenderArtifacts {
        self.render_snapshot_matching(|owner| {
            matches!(
                resolve_rsjs_artifact_owner(owner),
                ResolvedRsjsArtifactOwner::Root
            )
        })
    }

    fn snapshot_for_transaction(
        &self,
        transaction: &Rc<RsjsArtifactTransaction>,
    ) -> RsjsRenderArtifacts {
        self.snapshot_matching(|owner| owner_resolves_to_transaction(owner, transaction))
    }

    fn render_snapshot_for_transaction(
        &self,
        transaction: &Rc<RsjsArtifactTransaction>,
    ) -> RenderArtifacts {
        self.render_snapshot_matching(|owner| owner_resolves_to_transaction(owner, transaction))
    }

    fn render_snapshot_matching(
        &self,
        mut include: impl FnMut(Option<Rc<RsjsArtifactTransaction>>) -> bool,
    ) -> RenderArtifacts {
        let rsjs = self.snapshot_matching(&mut include);
        let mut react_islands: Vec<_> = self
            .react_island_contributions
            .iter()
            .filter(|contribution| include(contribution.owner.clone()))
            .map(|contribution| contribution.name.clone())
            .collect();
        react_islands.sort_unstable();
        react_islands.dedup();
        RenderArtifacts {
            rsjs,
            react_islands,
        }
    }

    fn snapshot_matching(
        &self,
        mut include: impl FnMut(Option<Rc<RsjsArtifactTransaction>>) -> bool,
    ) -> RsjsRenderArtifacts {
        let mut rendered_islands: Vec<_> = self
            .island_contributions
            .iter()
            .filter(|contribution| include(contribution.owner.clone()))
            .map(|contribution| contribution.asset_id)
            .collect();
        rendered_islands.sort_unstable();
        rendered_islands.dedup();

        // Multiple speculative branches may register the same structural key
        // with different snapshots. Within one owner, registration preserves
        // the historical first-key-wins behavior. Across independent owners,
        // sorting by both key and state makes the surviving choice independent
        // of branch completion order: the lexically smallest serialized state
        // wins. Identical pairs collapse naturally.
        let mut query_seeds: Vec<_> = self
            .query_seed_contributions
            .iter()
            .filter(|contribution| include(contribution.owner.clone()))
            .map(|contribution| contribution.seed.clone())
            .collect();
        query_seeds.sort_unstable_by(|left, right| {
            left.key_json
                .cmp(&right.key_json)
                .then_with(|| left.state_json.cmp(&right.state_json))
        });
        query_seeds.dedup_by(|right, left| right.key_json == left.key_json);

        let mut shared_state_seeds: Vec<_> = self
            .shared_state_seed_contributions
            .iter()
            .filter(|contribution| include(contribution.owner.clone()))
            .map(|contribution| Arc::clone(&contribution.seed))
            .collect();
        shared_state_seeds.sort_unstable_by_key(|seed| seed.scope_id);
        shared_state_seeds.dedup_by(|right, left| right.as_ref() == left.as_ref());

        RsjsRenderArtifacts {
            rendered_islands,
            query_seeds,
            shared_state_seeds,
        }
    }

    fn prune_rolled_back(&mut self) {
        self.island_contributions.retain(|contribution| {
            !matches!(
                resolve_rsjs_artifact_owner(contribution.owner.clone()),
                ResolvedRsjsArtifactOwner::RolledBack
            )
        });
        self.query_seed_contributions.retain(|contribution| {
            !matches!(
                resolve_rsjs_artifact_owner(contribution.owner.clone()),
                ResolvedRsjsArtifactOwner::RolledBack
            )
        });
        self.react_island_contributions.retain(|contribution| {
            !matches!(
                resolve_rsjs_artifact_owner(contribution.owner.clone()),
                ResolvedRsjsArtifactOwner::RolledBack
            )
        });
        self.shared_state_seed_contributions.retain(|contribution| {
            !matches!(
                resolve_rsjs_artifact_owner(contribution.owner.clone()),
                ResolvedRsjsArtifactOwner::RolledBack
            )
        });
        self.islands.clear();
        self.islands.extend(
            self.island_contributions
                .iter()
                .filter(|contribution| {
                    matches!(
                        resolve_rsjs_artifact_owner(contribution.owner.clone()),
                        ResolvedRsjsArtifactOwner::Root
                    )
                })
                .map(|contribution| contribution.asset_id),
        );
    }
}

/// Lazy response-scoped rsjs artifact channel shared by detached contexts.
#[derive(Default)]
struct RsjsArtifactContext {
    page: OnceCell<Rc<RefCell<RsjsArtifactPage>>>,
}

impl RsjsArtifactContext {
    fn ensure_page(&self) -> &Rc<RefCell<RsjsArtifactPage>> {
        self.page
            .get_or_init(|| Rc::new(RefCell::new(RsjsArtifactPage::default())))
    }
}

impl RsjsArtifactContext {
    fn fork(&mut self) -> Self {
        let clone = Self::default();
        let _ = clone.page.set(Rc::clone(self.ensure_page()));
        clone
    }
}

impl fmt::Debug for RsjsArtifactContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RsjsArtifactContext")
            .field(
                "island_count",
                &self
                    .page
                    .get()
                    .map_or(0, |page| page.borrow().islands.len()),
            )
            .field(
                "query_seed_count",
                &self
                    .page
                    .get()
                    .map_or(0, |page| page.borrow().query_seed_contributions.len()),
            )
            .field(
                "shared_state_seed_count",
                &self.page.get().map_or(0, |page| {
                    page.borrow().shared_state_seed_contributions.len()
                }),
            )
            .finish()
    }
}

/// Request-scoped state carried by a [`WebContext`].
///
/// This type deliberately does not implement `Clone`: detached rendering has
/// different semantics for immutable request data, branch-local accumulators,
/// and the RSJS registries that coordinate future work. Use
/// [`WebContext::context_parts`] to create that explicit fork.
#[derive(Debug, Default)]
pub struct WebContextParts {
    request: RequestContext,
    response: ResponseContext,
    head: HeadContext,
    cache_trace: CacheTrace,
    telemetry: RenderTelemetry,
    locals: Locals,
    csp_nonce: Option<CspNonce>,
    rsjs_artifacts: RsjsArtifactContext,
    rsjs_artifact_transaction: Option<Rc<RsjsArtifactTransaction>>,
    rsjs_shared_state: RsjsSharedStateContext,
}

/// Exact compiler-linked occurrence of one `rsjs::SharedStateProvider`.
///
/// The schema `type_key` identifies a shared-state type, not one lexical
/// provider instance. Final RSJS plans use this allocation-free key to bind a
/// consumer to the exact provider occurrence selected by the linker, including
/// when an inner provider shadows another provider with the same schema.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RsjsLinkedProviderOccurrenceKey {
    scope_id: [u8; 32],
    host_candidate_id: u64,
    provider_site_id: u32,
}

impl RsjsLinkedProviderOccurrenceKey {
    /// Construct one exact linked provider occurrence.
    #[doc(hidden)]
    pub const fn new(scope_id: [u8; 32], host_candidate_id: u64, provider_site_id: u32) -> Self {
        Self {
            scope_id,
            host_candidate_id,
            provider_site_id,
        }
    }

    /// Stable logical render-root scope identity.
    #[doc(hidden)]
    pub const fn scope_id(self) -> [u8; 32] {
        self.scope_id
    }

    /// Component candidate whose render program contains the provider site.
    #[doc(hidden)]
    pub const fn host_candidate_id(self) -> u64 {
        self.host_candidate_id
    }

    /// Stable provider-site identity local to the host component definition.
    #[doc(hidden)]
    pub const fn provider_site_id(self) -> u32 {
        self.provider_site_id
    }
}

#[derive(Clone)]
struct RsjsSharedStateBinding {
    scope_id: u64,
    type_key: &'static str,
    schema_hash: &'static str,
    rust_type: &'static str,
    value: Rc<dyn Any>,
    linked_occurrence: Option<RsjsLinkedProviderOccurrenceKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RsjsSharedStateSeed {
    artifact: Arc<RsjsSharedStateSeedArtifact>,
    /// Error-boundary transaction that owns this seed. Successful nested
    /// boundaries merge their transaction into the parent; a rewound
    /// boundary removes only seeds carrying its id, so concurrent sibling
    /// contexts cannot erase each other's registrations.
    transaction_id: Option<u64>,
}

#[derive(Default)]
struct RsjsSharedStatePage {
    next_scope_id: u64,
    seed_table_published: bool,
    seed_table_emitted: bool,
    schemas: HashMap<&'static str, (&'static str, &'static str)>,
    seeds: Vec<RsjsSharedStateSeed>,
}

/// Request-local rsjs shared-state registry plus the current lexical provider
/// stack. Page seeds are shared by deferred render contexts; the active stack
/// is forked by value so concurrent Suspense branches cannot change each
/// other's nearest-provider resolution.
///
/// Both cells follow the crate's lazy cold-parts convention (see
/// [`ResponseContext`]): a request that never touches shared state pays zero
/// heap allocations for this channel, and `WebContextParts::new()` stays
/// allocation-free.
#[derive(Default)]
struct RsjsSharedStateContext {
    page: OnceCell<Rc<RefCell<RsjsSharedStatePage>>>,
    active: OnceCell<Rc<RefCell<Vec<RsjsSharedStateBinding>>>>,
    /// Context-local copy of the seed-table-finalized latch.
    ///
    /// The shared `page.seed_table_published` flag reaches every context
    /// that already aliases the page registry (each clone materializes and
    /// shares `page`). This copy covers the remaining case: a context forked
    /// after finalization from a root whose registry was never materialized
    /// (zero providers, zero boundary forks). Without it, a late provider on
    /// such a fork would mint a scope the client can never receive instead
    /// of failing with the streaming diagnostic.
    published: Cell<bool>,
    /// Transport slot used when this request-local channel is forked through
    /// `WebContextParts`. The live render context keeps the authoritative
    /// value in `WebContext::rsjs_shared_state_transaction` so taking an
    /// ErrorBoundary checkpoint remains allocation-free on cold pages.
    current_transaction: Cell<Option<u64>>,
}

impl RsjsSharedStateContext {
    fn ensure_page(&self) -> &Rc<RefCell<RsjsSharedStatePage>> {
        self.page
            .get_or_init(|| Rc::new(RefCell::new(RsjsSharedStatePage::default())))
    }

    fn ensure_active(&self) -> &Rc<RefCell<Vec<RsjsSharedStateBinding>>> {
        self.active
            .get_or_init(|| Rc::new(RefCell::new(Vec::new())))
    }
}

/// Forking creates a boundary view of the channel: the page registry (scope-id
/// counter, seeds, finalization latch) is materialized and Rc-shared so a
/// handle cloned before any provider ran still feeds the same response-level
/// table, while the active provider stack is copied by value so concurrent
/// Suspense branches cannot change each other's nearest-provider resolution.
/// An empty active stack forks to an untouched cell with no allocation.
impl RsjsSharedStateContext {
    fn fork(&mut self) -> Self {
        let clone = Self::default();
        let _ = clone.page.set(Rc::clone(self.ensure_page()));
        if let Some(active) = self.active.get() {
            let snapshot = active.borrow();
            if !snapshot.is_empty() {
                // proa-perf-ok: per-boundary fork of a non-empty provider stack is the documented isolation contract
                let _ = clone.active.set(Rc::new(RefCell::new(snapshot.clone())));
            }
        }
        clone.published.set(self.published.get());
        clone
            .current_transaction
            .set(self.current_transaction.get());
        clone
    }
}

impl fmt::Debug for RsjsSharedStateContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RsjsSharedStateContext")
            .field(
                "seed_count",
                &self.page.get().map_or(0, |page| page.borrow().seeds.len()),
            )
            .field(
                "active_depth",
                &self.active.get().map_or(0, |active| active.borrow().len()),
            )
            .finish()
    }
}

/// RAII guard for one active rsjs shared-state provider.
///
/// The guard owns the stack handle rather than borrowing `WebContext`, so it
/// remains live across an async child render and restores the provider stack
/// during both ordinary errors and unwinding.
#[doc(hidden)]
#[must_use = "the provider scope ends when this guard is dropped; bind it \
              (for example `let _scope = cx.rsjs_shared_state_enter(...)?;`) \
              and keep it alive while the children render, or every consuming \
              island will fail with rsjs/shared-state-missing-provider"]
pub struct RsjsSharedStateScopeGuard {
    active: Rc<RefCell<Vec<RsjsSharedStateBinding>>>,
    scope_id: u64,
}

/// RAII tag binding the current provider to one exact linked occurrence.
///
/// Like [`RsjsSharedStateScopeGuard`], this guard owns the active-stack handle
/// instead of borrowing `WebContext`, so generated wrappers can keep it alive
/// across an async child render. Dropping it removes only its own tag and is
/// harmless if the provider scope was already unwound.
#[doc(hidden)]
#[must_use = "the exact linked provider tag ends when this guard is dropped; \
              keep it alive while the provider's linked children render"]
pub struct RsjsLinkedProviderTagGuard {
    active: Rc<RefCell<Vec<RsjsSharedStateBinding>>>,
    scope_id: u64,
    occurrence: RsjsLinkedProviderOccurrenceKey,
}

/// Cancellation-safe scope for one async [`crate::ErrorBoundary`] fallback.
///
/// The guard owns the lazy stack handle rather than borrowing [`WebContext`],
/// so an async boundary can keep it alive while its child borrows the context.
/// Dropping a pending render removes only that boundary's entry.
#[doc(hidden)]
pub struct BoundaryErrorFallbackScopeGuard<L: DataLoader> {
    stack: Rc<RefCell<BoundaryErrorFallbackStackEntries<L>>>,
    fallback: BoundaryErrorFallbackHandle<L>,
}

const INLINE_BOUNDARY_ERROR_FALLBACKS: usize = 8;

struct BoundaryErrorFallbackStackEntries<L: DataLoader> {
    inline: heapless::Vec<BoundaryErrorFallbackHandle<L>, INLINE_BOUNDARY_ERROR_FALLBACKS>,
    overflow: Vec<BoundaryErrorFallbackHandle<L>>,
}

impl<L: DataLoader> Default for BoundaryErrorFallbackStackEntries<L> {
    fn default() -> Self {
        Self {
            inline: heapless::Vec::new(),
            overflow: Vec::new(),
        }
    }
}

impl<L: DataLoader> BoundaryErrorFallbackStackEntries<L> {
    fn push(&mut self, fallback: BoundaryErrorFallbackHandle<L>) {
        if self.overflow.is_empty() && self.inline.len() < self.inline.capacity() {
            self.inline
                .push(fallback)
                .unwrap_or_else(|_| unreachable!("inline capacity checked"));
        } else {
            self.overflow.push(fallback);
        }
    }

    fn pop(&mut self) -> Option<BoundaryErrorFallbackHandle<L>> {
        self.overflow.pop().or_else(|| self.inline.pop())
    }

    fn last(&self) -> Option<&BoundaryErrorFallbackHandle<L>> {
        self.overflow.last().or_else(|| self.inline.last())
    }

    fn remove(&mut self, fallback: &BoundaryErrorFallbackHandle<L>) {
        if let Some(index) = self
            .overflow
            .iter()
            .rposition(|entry| Rc::ptr_eq(entry, fallback))
        {
            self.overflow.remove(index);
            return;
        }
        if let Some(index) = self
            .inline
            .iter()
            .rposition(|entry| Rc::ptr_eq(entry, fallback))
        {
            self.inline.remove(index);
        }
    }

    fn snapshot(&self) -> Vec<BoundaryErrorFallbackHandle<L>> {
        self.inline
            .iter()
            .chain(self.overflow.iter())
            .cloned()
            .collect()
    }
}

impl<L: DataLoader> Drop for BoundaryErrorFallbackScopeGuard<L> {
    fn drop(&mut self) {
        let mut stack = self.stack.borrow_mut();
        if stack
            .last()
            .is_some_and(|entry| Rc::ptr_eq(entry, &self.fallback))
        {
            stack.pop();
            return;
        }
        stack.remove(&self.fallback);
    }
}

/// Input accepted by the internal shared-state provider bridge. Generated
/// providers pass the recursive path JSON directly. The slice impls retain
/// source compatibility for low-level tests and adapters that used the
/// original top-level `wide` field list.
#[doc(hidden)]
pub trait RsjsSharedStateWidePaths {
    fn into_wide_paths_json(self) -> Cow<'static, str>;
}

impl RsjsSharedStateWidePaths for &'static str {
    fn into_wide_paths_json(self) -> Cow<'static, str> {
        Cow::Borrowed(self)
    }
}

impl RsjsSharedStateWidePaths for &'static [&'static str] {
    fn into_wide_paths_json(self) -> Cow<'static, str> {
        let paths: Vec<[&str; 1]> = self.iter().map(|field| [*field]).collect();
        Cow::Owned(serde_json::to_string(&paths).expect("static wide field names must serialize"))
    }
}

impl<const N: usize> RsjsSharedStateWidePaths for &'static [&'static str; N] {
    fn into_wide_paths_json(self) -> Cow<'static, str> {
        self.as_slice().into_wide_paths_json()
    }
}

impl fmt::Debug for RsjsSharedStateScopeGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RsjsSharedStateScopeGuard")
            .field("scope_id", &self.scope_id)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for RsjsLinkedProviderTagGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RsjsLinkedProviderTagGuard")
            .field("scope_id", &self.scope_id)
            .field("occurrence", &self.occurrence)
            .finish_non_exhaustive()
    }
}

/// Ownership checkpoint for rsjs island ids, query seeds, and SharedState
/// consumer liveness recorded by one render subtree.
///
/// Normal render code uses the `WebContext` commit/rollback methods. Detached
/// cancelable work should use [`RsjsArtifactBranch`], whose drop path is
/// deliberately rollback-only.
#[doc(hidden)]
#[derive(Debug)]
pub struct RsjsArtifactCheckpoint {
    transaction: Rc<RsjsArtifactTransaction>,
    current_slot: Rc<RefCell<Option<Rc<RsjsArtifactTransaction>>>>,
    flush_generation: u64,
    armed: bool,
}

impl RsjsArtifactCheckpoint {
    fn reset_current_slot(&self) {
        let mut current = self.current_slot.borrow_mut();
        if current
            .as_ref()
            .is_some_and(|active| Rc::ptr_eq(active, &self.transaction))
        {
            *current = self.transaction.parent.clone();
        }
    }

    fn commit_now(&mut self) {
        self.transaction
            .state
            .set(RsjsArtifactTransactionState::Committed);
        self.reset_current_slot();
        self.armed = false;
    }

    fn rollback_now(&mut self) {
        self.transaction
            .state
            .set(RsjsArtifactTransactionState::RolledBack);
        self.reset_current_slot();
        self.armed = false;
    }
}

impl Drop for RsjsArtifactCheckpoint {
    fn drop(&mut self) {
        if self.armed {
            // Cancellation must not publish speculative artifact contributions.
            // The shared transaction node makes this visible even to detached
            // context forks that still hold the inherited owner.
            self.rollback_now();
        }
    }
}

/// RAII collector for a detached or cancelable rsjs render branch.
///
/// Dropping an uncommitted branch rolls back its island ids, query seeds, and
/// SharedState consumer liveness.
/// This is the safe shape for out-of-order boundaries: an `Abortable` timeout
/// drops the future and therefore the branch before it can produce a
/// `RenderedBoundary`.
#[must_use = "dropping an rsjs artifact branch rolls its speculative artifacts back"]
#[derive(Debug)]
pub struct RsjsArtifactBranch {
    checkpoint: Option<RsjsArtifactCheckpoint>,
    page: Rc<RefCell<RsjsArtifactPage>>,
}

impl RsjsArtifactBranch {
    /// Deterministic artifacts owned by this branch, including nested
    /// transactions already committed into it.
    pub fn snapshot(&self) -> RsjsRenderArtifacts {
        let checkpoint = self
            .checkpoint
            .as_ref()
            .expect("an rsjs artifact branch can only be finalized once");
        self.page
            .borrow()
            .snapshot_for_transaction(&checkpoint.transaction)
    }

    /// Deterministic React and rsjs artifacts owned by this branch.
    pub fn render_artifacts_snapshot(&self) -> RenderArtifacts {
        let checkpoint = self
            .checkpoint
            .as_ref()
            .expect("an artifact branch can only be finalized once");
        self.page
            .borrow()
            .render_snapshot_for_transaction(&checkpoint.transaction)
    }

    /// Commit this branch into its parent transaction and return the exact
    /// transportable artifacts that committed.
    pub fn commit<B, L>(mut self, cx: &WebContext<L, B>) -> RsjsRenderArtifacts
    where
        B: WriteBuf,
        L: DataLoader,
    {
        let snapshot = self.snapshot();
        let checkpoint = self
            .checkpoint
            .take()
            .expect("an rsjs artifact branch can only be finalized once");
        cx.rsjs_artifact_commit(checkpoint);
        snapshot
    }

    /// Commit this branch and return every browser artifact published with it.
    pub fn commit_render_artifacts<B, L>(mut self, cx: &WebContext<L, B>) -> RenderArtifacts
    where
        B: WriteBuf,
        L: DataLoader,
    {
        let snapshot = self.render_artifacts_snapshot();
        let checkpoint = self
            .checkpoint
            .take()
            .expect("an artifact branch can only be finalized once");
        cx.rsjs_artifact_commit(checkpoint);
        snapshot
    }

    /// Explicitly roll this branch back. Dropping it has the same semantic
    /// effect; this method also eagerly prunes discarded contributions.
    pub fn rollback<B, L>(mut self, cx: &WebContext<L, B>)
    where
        B: WriteBuf,
        L: DataLoader,
    {
        let checkpoint = self
            .checkpoint
            .take()
            .expect("an rsjs artifact branch can only be finalized once");
        cx.rsjs_artifact_rollback_unconditionally(checkpoint);
    }
}

impl Drop for RsjsArtifactBranch {
    fn drop(&mut self) {
        // Dropping the armed checkpoint marks the transaction rolled back.
        self.checkpoint.take();
        self.page.borrow_mut().prune_rolled_back();
    }
}

/// Checkpoint of the rsjs shared-state seed registry, captured before a
/// subtree renders so the seeds it registers can be discarded if that
/// subtree's output is rewound.
///
/// Taken by [`WebContext::rsjs_shared_state_seed_checkpoint`] before
/// `ErrorBoundary` renders its child and consumed by
/// [`WebContext::rsjs_shared_state_rollback_seeds`] when the child's bytes
/// are rewound: without the rollback, a `SharedStateProvider` inside the
/// discarded subtree would still ship its full serialized state in the page
/// seed table even though no surviving markup references the scope.
#[doc(hidden)]
#[derive(Debug)]
pub struct RsjsSharedStateSeedCheckpoint {
    transaction_id: u64,
    parent_transaction_id: Option<u64>,
    /// Stream flush generation observed when the checkpoint was taken.
    ///
    /// A rewound output buffer only proves the subtree's markup is gone if
    /// none of it was streamed away first. In streaming modes a nested
    /// `Suspense` ships the enclosing buffer mid-subtree, and on `Vec<u8>`
    /// shells that also swaps in a fresh buffer, which makes the recorded
    /// byte offset describe a buffer that no longer exists (offset 0 then
    /// rewinds "successfully" forever). Comparing this generation against
    /// the live one at rollback time is the missing proof.
    flush_generation: u64,
}

impl RsjsSharedStateSeedCheckpoint {
    /// Existing request-unique transaction id, also used by managed recovery.
    #[doc(hidden)]
    pub fn transaction_id(&self) -> u64 {
        self.transaction_id
    }
}

static NEXT_RSJS_SHARED_STATE_TRANSACTION_ID: AtomicU64 = AtomicU64::new(1);

impl Drop for RsjsSharedStateScopeGuard {
    fn drop(&mut self) {
        let mut active = self.active.borrow_mut();
        if active
            .last()
            .is_some_and(|binding| binding.scope_id == self.scope_id)
        {
            active.pop();
            return;
        }
        if let Some(index) = active
            .iter()
            .rposition(|binding| binding.scope_id == self.scope_id)
        {
            active.remove(index);
        }
    }
}

impl Drop for RsjsLinkedProviderTagGuard {
    fn drop(&mut self) {
        let mut active = self.active.borrow_mut();
        let Some(binding) = active
            .iter_mut()
            .rfind(|binding| binding.scope_id == self.scope_id)
        else {
            return;
        };
        if binding.linked_occurrence == Some(self.occurrence) {
            binding.linked_occurrence = None;
        }
    }
}

impl WebContextParts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request(&self) -> &RequestContext {
        &self.request
    }

    pub fn request_mut(&mut self) -> &mut RequestContext {
        &mut self.request
    }

    pub fn set_request(&mut self, request: RequestContext) {
        self.request = request;
    }

    pub fn with_request(mut self, request: RequestContext) -> Self {
        self.request = request;
        self
    }

    pub fn response(&self) -> &ResponseContext {
        &self.response
    }

    pub fn response_mut(&mut self) -> &mut ResponseContext {
        &mut self.response
    }

    pub fn set_response(&mut self, response: ResponseContext) {
        self.response = response;
    }

    pub fn with_response(mut self, response: ResponseContext) -> Self {
        self.response = response;
        self
    }

    pub fn head_state(&self) -> &HeadContext {
        &self.head
    }

    pub fn head_mut(&mut self) -> HeadWriter<'_> {
        HeadWriter::new(&mut self.head)
    }

    pub fn set_head(&mut self, head: HeadContext) {
        if let Some(document) = self.head.document() {
            document.reject_replacement();
            return;
        }
        self.head = head;
    }

    pub fn with_head(mut self, head: HeadContext) -> Self {
        self.set_head(head);
        self
    }

    pub fn cache_trace(&self) -> &CacheTrace {
        &self.cache_trace
    }

    pub fn cache_trace_mut(&mut self) -> &mut CacheTrace {
        &mut self.cache_trace
    }

    pub fn set_cache_trace(&mut self, cache_trace: CacheTrace) {
        self.cache_trace = cache_trace;
    }

    pub fn with_cache_trace(mut self, cache_trace: CacheTrace) -> Self {
        self.cache_trace = cache_trace;
        self
    }

    pub fn telemetry(&self) -> &RenderTelemetry {
        &self.telemetry
    }

    pub fn telemetry_mut(&mut self) -> &mut RenderTelemetry {
        &mut self.telemetry
    }

    pub fn set_telemetry(&mut self, telemetry: RenderTelemetry) {
        self.telemetry = telemetry;
    }

    pub fn with_telemetry(mut self, telemetry: RenderTelemetry) -> Self {
        self.telemetry = telemetry;
        self
    }

    pub fn locals(&self) -> &Locals {
        &self.locals
    }

    pub fn locals_mut(&mut self) -> &mut Locals {
        &mut self.locals
    }

    pub fn set_locals(&mut self, locals: Locals) {
        self.locals = locals;
    }

    pub fn with_locals(mut self, locals: Locals) -> Self {
        self.locals = locals;
        self
    }

    pub fn csp_nonce(&self) -> Option<&CspNonce> {
        self.csp_nonce.as_ref()
    }

    pub fn set_csp_nonce(&mut self, nonce: CspNonce) {
        // A managed document reads the nonce at publication. Installing one
        // after preparation began poisons the document with a named error.
        if let Some(document) = self.head.document() {
            if document.reject_late_nonce().is_err() {
                return;
            }
        }
        self.csp_nonce = Some(nonce);
    }

    pub fn with_csp_nonce(mut self, nonce: CspNonce) -> Self {
        self.set_csp_nonce(nonce);
        self
    }
}

/// Shared request context for rendering HTML and Markdown for the web.
///
/// Generic first over the request-scoped data loader type `L`, then over the
/// output type `B`. The standard writer defaults to [`RenderBuffer`], so
/// application components normally customize only their loader.
///
/// # Why merge HTML and Markdown contexts?
///
/// HTML and Markdown are representations of the same web request. Both need
/// output ownership, a request-scoped loader, response metadata, metering, and
/// streaming machinery. Separate contexts duplicated that machinery and let
/// Markdown fall behind changes such as loader support and lifetime-free
/// render signatures. Sharing the implementation makes future fixes apply to
/// both formats.
///
/// The refactor therefore chooses `WebContext<L, B>` as the common context.
/// Owning `B` removes the context's output lifetime; borrowed writers remain
/// possible through `B = &mut Writer`. HTML-in-Markdown bridges can reborrow
/// the live loader and state instead of constructing a fresh `NoLoader`
/// context, allocating another parts box and loader `Arc` on every bridge.
///
/// The HTML and Markdown render traits remain distinct: each format keeps its
/// escaping, composition rules, and formatting state. Sharing a context does
/// not make their output interchangeable. The `WebContext` name describes the
/// shared web request environment; a future native renderer may need a
/// separate context for graphics resources.
///
pub struct WebContext<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    /// The owned output buffer for the selected representation.
    pub out: B,

    /// Current content mode (HTML, JavaScript, etc.) for proper escaping.
    mode: ContentMode,

    /// Current escape dialect for text and attribute escaping.
    escape_dialect: EscapeDialect,

    /// Request render mode used by streaming-aware features.
    render_mode: RenderMode,

    /// Format-local Markdown state, independent of output flushes.
    pub(crate) md_state: crate::md::MdState,

    /// Each ordinary `WebContext` owns one of these roughly 680-byte bundles.
    /// It is boxed so those bytes are not stored inline in the `WebContext`
    /// itself. Boxing is a performance tradeoff rather than a correctness
    /// requirement: it adds one allocation to each ordinary context and
    /// detached Suspense child, but keeps the context itself cheap to move. An
    /// inline `Option<WebContextParts>` would also work.
    ///
    /// Detached Suspense first calls [`WebContext::context_parts`] to build an
    /// unboxed, field-aware snapshot. The boundary future captures that
    /// snapshot by value. When first polled, it moves the snapshot into the
    /// child context's box, which remains alive across the child's `.await`.
    /// The future must therefore still have room for the initial snapshot;
    /// boxing does not eliminate the per-boundary state. It keeps the child
    /// `WebContext` small after construction. The total state scales with
    /// concurrently pending boundaries, not ordinary render depth.
    ///
    /// The snapshot is deliberately not a plain `Clone`. If streaming already
    /// started, its response state omits the header map because those headers
    /// are committed to the wire. Other fields follow the fork semantics
    /// documented on [`WebContextParts`].
    ///
    /// Ordinary nested renders borrow `&mut WebContext` and do not duplicate
    /// this state. Async request drivers own one root context across `.await`,
    /// while each detached Suspense boundary owns an additional child context.
    ///
    /// The `Option` serves a separate purpose. [`WebContext::with_output`] and
    /// its scoped-output variants use `mem::take` to move the box into a
    /// temporary child and restore it on drop. `None` also represents an
    /// explicitly stateless transport writer that only needs rendering
    /// machinery for bytes such as boundary ids. Every ordinary component
    /// render context owns `Some` parts.
    ///
    /// A direct benchmark is required to decide whether this box beats inline
    /// plain state. The existing context-parts benchmark compared the former
    /// per-field shared-cell design with the broader plain-state refactor; it
    /// did not isolate boxed versus inline `WebContextParts`.
    ///
    /// See [`WebContextParts`] for why it is not `Clone` and how the fork
    /// treats each field.
    context_parts: Option<Box<WebContextParts>>,

    /// Lazily materialized current rsjs artifact transaction slot.
    ///
    /// Detached contexts receive their own slot pointing at the same immutable
    /// transaction node. That lets a parent commit or roll back after the fork
    /// without letting a late child write under a stranded raw transaction id.
    ///
    /// TODO(@hariria): revisit whether this can be made simpler. Writing the
    /// rest of this down because the type is genuinely horrible to read and I
    /// am going to forget why it looks like this.
    ///
    /// The two `Rc`s look redundant. They are not: they share two different
    /// things, with two different lifetimes.
    ///
    /// - The **outer** `Rc<RefCell<..>>` shares the *slot*.
    ///   [`WebContext::rsjs_artifact_checkpoint`] takes `&self` and hands back
    ///   an RAII [`RsjsArtifactCheckpoint`] that outlives the borrow. On drop
    ///   (cancellation) it has to put the previous transaction back, and it
    ///   cannot hold a `&WebContext` to do it, so it keeps its own
    ///   `Rc::clone` of the cell instead.
    /// - The **inner** `Rc<RsjsArtifactTransaction>` shares the *transaction*.
    ///   The parent chain, every recorded contribution's `owner`, and the
    ///   checkpoint all point at the same node, and it has to be shared by
    ///   identity because rollback compares with `Rc::ptr_eq`.
    ///
    /// The `OnceCell` is not only laziness (though it does keep a render that
    /// never touches rsjs from allocating the cell at all). `set_context_parts`
    /// deliberately throws the cell away and re-seeds a *fresh* one holding the
    /// same transaction, and that is what gives a fork its own slot. Collapse
    /// it and parent and child alias one cell, so a parent commit retargets the
    /// child.
    ///
    /// If you do simplify this, the tests that care are the artifact
    /// commit/rollback and detached-branch ones; a wrong answer here shows up
    /// as islands whose JS silently stops shipping.
    rsjs_artifact_transaction: OnceCell<Rc<RefCell<Option<Rc<RsjsArtifactTransaction>>>>>,

    /// Active ErrorBoundary seed transaction. This deliberately lives on the
    /// render context rather than in the cloneable side-channel snapshot so
    /// live transaction transitions use exclusive mutation. Detached context
    /// snapshots copy it through `RsjsSharedStateContext::current_transaction`.
    rsjs_shared_state_transaction: Option<u64>,

    /// Request-scoped data loader for `cx.load(...)`. Owned via `Arc` so it
    /// can be shared with futures that outlive the borrow on `WebContext`.
    loader: Arc<L>,

    /// Optional recorder for out-of-order streaming boundaries.
    stream_recorder: Option<Rc<dyn StreamBoundaryRecorder>>,

    /// Optional sink for pure HTML chunk flushing in in-order streaming mode.
    stream_chunk_flusher: Option<Rc<dyn StreamChunkFlusher>>,

    /// Stack of nearest active error fallbacks. `ErrorBoundary` pushes here
    /// while rendering its child; streaming `Suspense` captures the top entry
    /// before deferring child work past the boundary's call stack.
    boundary_error_fallbacks: Option<Rc<RefCell<BoundaryErrorFallbackStackEntries<L>>>>,
}

/// Owns the non-output state moved into a scoped [`WebContext`] reborrow.
///
/// Restoring from `Drop` keeps the parent usable even if a renderer panics.
/// The parent output itself is never moved; only the child context points at
/// the temporary output type.
struct ScopedWebState<'scope, L: DataLoader> {
    mode: &'scope mut ContentMode,
    escape_dialect: &'scope mut EscapeDialect,
    render_mode: &'scope mut RenderMode,
    md_state: &'scope mut crate::md::MdState,
    context_parts: &'scope mut Option<Box<WebContextParts>>,
    rsjs_artifact_transaction:
        &'scope mut OnceCell<Rc<RefCell<Option<Rc<RsjsArtifactTransaction>>>>>,
    rsjs_shared_state_transaction: &'scope mut Option<u64>,
    loader: &'scope mut Arc<L>,
    stream_recorder: &'scope mut Option<Rc<dyn StreamBoundaryRecorder>>,
    stream_chunk_flusher: &'scope mut Option<Rc<dyn StreamChunkFlusher>>,
    boundary_error_fallbacks: &'scope mut Option<Rc<RefCell<BoundaryErrorFallbackStackEntries<L>>>>,
}

/// Restores scoped non-output state into the parent when a render returns or
/// unwinds. Keeping only field-level borrows makes it possible to safely wrap
/// the parent's own output in a different [`WriteBuf`] type.
struct ScopedWebOutput<'scope, C, L>
where
    C: WriteBuf,
    L: DataLoader,
{
    parent: ScopedWebState<'scope, L>,
    child: Option<WebContext<L, C>>,
}

impl<'scope, C, L> ScopedWebOutput<'scope, C, L>
where
    C: WriteBuf,
    L: DataLoader,
{
    fn new(parent: ScopedWebState<'scope, L>, out: C) -> Self {
        let child = WebContext {
            out,
            mode: *parent.mode,
            escape_dialect: *parent.escape_dialect,
            render_mode: *parent.render_mode,
            md_state: *parent.md_state,
            context_parts: std::mem::take(parent.context_parts),
            rsjs_artifact_transaction: std::mem::take(parent.rsjs_artifact_transaction),
            rsjs_shared_state_transaction: *parent.rsjs_shared_state_transaction,
            loader: Arc::clone(parent.loader),
            stream_recorder: parent.stream_recorder.take(),
            stream_chunk_flusher: parent.stream_chunk_flusher.take(),
            boundary_error_fallbacks: std::mem::take(parent.boundary_error_fallbacks),
        };
        Self {
            parent,
            child: Some(child),
        }
    }

    fn child_mut(&mut self) -> &mut WebContext<L, C> {
        self.child
            .as_mut()
            .expect("scoped output remains present until restoration")
    }

    fn restore(&mut self) -> Option<C> {
        let mut child = self.child.take()?;
        *self.parent.mode = child.mode;
        *self.parent.escape_dialect = child.escape_dialect;
        *self.parent.render_mode = child.render_mode;
        *self.parent.md_state = child.md_state;
        *self.parent.context_parts = std::mem::take(&mut child.context_parts);
        *self.parent.rsjs_artifact_transaction =
            std::mem::take(&mut child.rsjs_artifact_transaction);
        *self.parent.rsjs_shared_state_transaction = child.rsjs_shared_state_transaction;
        *self.parent.loader = Arc::clone(&child.loader);
        *self.parent.stream_recorder = child.stream_recorder.take();
        *self.parent.stream_chunk_flusher = child.stream_chunk_flusher.take();
        *self.parent.boundary_error_fallbacks = std::mem::take(&mut child.boundary_error_fallbacks);
        Some(child.out)
    }
}

impl<C, L> Drop for ScopedWebOutput<'_, C, L>
where
    C: WriteBuf,
    L: DataLoader,
{
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn with_scoped_web_output<C, L, R>(
    parent: ScopedWebState<'_, L>,
    out: C,
    render: impl FnOnce(&mut WebContext<L, C>) -> R,
) -> R
where
    C: WriteBuf,
    L: DataLoader,
{
    let mut scoped = ScopedWebOutput::new(parent, out);
    render(scoped.child_mut())
}

/// The observer owns its suffix separately from the child's Markdown state.
/// Nested Markdown fragments may reset their own state without losing the
/// physical suffix produced through the observer. Restore before output-state
/// cleanup so even unwinding returns the observed suffix to the parent.
struct HtmlInMdScope<'scope, L: DataLoader, B: WriteBuf> {
    output: ScopedWebOutput<'scope, crate::md::context::ObservedMdWriteBuf<'scope, B>, L>,
    saved: crate::md::MdState,
    mode: ContentMode,
    escape_dialect: EscapeDialect,
}

impl<L: DataLoader, B: WriteBuf> Drop for HtmlInMdScope<'_, L, B> {
    fn drop(&mut self) {
        let child = self.output.child_mut();
        child.out.merge_suffix(&mut self.saved);
        child.md_state = self.saved;
        child.mode = self.mode;
        child.escape_dialect = self.escape_dialect;
    }
}

/// Isolate representation-local state while capturing HTML for conversion.
/// Request mutations still flow back through `ScopedWebOutput` on drop.
#[cfg(feature = "html-to-markdown")]
struct HtmlCaptureScope<'scope, L: DataLoader> {
    output: ScopedWebOutput<'scope, RenderBuffer, L>,
    md_state: crate::md::MdState,
    mode: ContentMode,
    escape_dialect: EscapeDialect,
    render_mode: RenderMode,
    stream_recorder: Option<Rc<dyn StreamBoundaryRecorder>>,
    stream_chunk_flusher: Option<Rc<dyn StreamChunkFlusher>>,
}

#[cfg(feature = "html-to-markdown")]
impl<L: DataLoader> Drop for HtmlCaptureScope<'_, L> {
    fn drop(&mut self) {
        let child = self.output.child_mut();
        child.md_state = self.md_state;
        child.mode = self.mode;
        child.escape_dialect = self.escape_dialect;
        child.render_mode = self.render_mode;
        child.stream_recorder = self.stream_recorder.take();
        child.stream_chunk_flusher = self.stream_chunk_flusher.take();
    }
}

impl WebContext<NoLoader, RenderBuffer> {
    /// Create a standard context that owns its reusable render buffer.
    pub fn new() -> Self {
        Self::with_buffer(RenderBuffer::new())
    }
}

impl Default for WebContext<NoLoader, RenderBuffer> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L: DataLoader> WebContext<L, Vec<u8>> {
    /// Flush the currently buffered HTML bytes to the configured streamed
    /// response sink, if any.
    pub fn flush_stream_chunk(&mut self) -> Result<(), crate::WriteError> {
        let Some(flusher) = self.stream_chunk_flusher.as_ref().cloned() else {
            return Ok(());
        };
        if let Some(d) = self.head_state().document() {
            d.limit_check(self.out.len())?;
            if !d.is_published() {
                return Ok(());
            }
        }

        if self.out.is_empty() {
            return Ok(());
        }

        let capacity = self.out.capacity().max(128);
        let chunk = std::mem::replace(&mut self.out, Vec::with_capacity(capacity));
        // These bytes are now unrecallable, and the buffer they came from was
        // replaced, so any byte offset recorded before this point no longer
        // describes them. Advance the flush generation so rewind-based
        // rollback (rsjs shared-state seeds) can tell.
        self.response_mut().mark_stream_chunk_flushed();
        flusher.flush_chunk(chunk)
    }
}

impl<L: DataLoader> WebContext<L, RenderBuffer> {
    /// Create a standard context with a custom request-scoped loader.
    pub fn with_loader(loader: Arc<L>) -> Self {
        Self::with_loader_buffer(RenderBuffer::new(), loader)
    }

    /// Flush the currently buffered HTML bytes to the configured streamed
    /// response sink, then recycle the standard buffer's pages for reuse.
    pub fn flush_stream_chunk(&mut self) -> Result<(), crate::WriteError> {
        let Some(flusher) = self.stream_chunk_flusher.as_ref().cloned() else {
            return Ok(());
        };
        if let Some(d) = self.head_state().document() {
            d.limit_check(self.out.len())?;
            if !d.is_published() {
                return Ok(());
            }
        }

        if WriteBuf::is_empty(&self.out) {
            // Match the framework's current Vec-backed root behavior: an
            // empty pre-boundary flush ships nothing and must not freeze
            // response metadata or advance the flush generation.
            return Ok(());
        }

        // The synchronous flusher must consume or copy every borrowed slice
        // before returning. Whether it succeeds or fails, the attempted
        // logical chunk is no longer retryable; the standard writer follows
        // the same discard-on-error contract as the old streaming writer.
        self.response_mut().mark_stream_chunk_flushed();
        let result = flusher.flush_chunk_view(&self.out);
        self.out.clear();
        result
    }
}

impl<B: WriteBuf> WebContext<NoLoader, B> {
    /// Create an advanced context that owns the supplied output value.
    pub fn with_buffer(out: B) -> Self {
        Self::with_optional_context_parts(out, Some(Box::new(WebContextParts::new())))
    }

    /// Create a context for transport formatting that cannot access request or
    /// response state.
    ///
    /// Boundary framing uses this to reuse numeric rendering without paying
    /// for request-scoped state that the formatter never touches.
    #[doc(hidden)]
    pub fn with_stateless_buffer(out: B) -> Self {
        Self::with_optional_context_parts(out, None)
    }

    fn with_optional_context_parts(out: B, context_parts: Option<Box<WebContextParts>>) -> Self {
        Self {
            out,
            mode: ContentMode::Html,
            escape_dialect: EscapeDialect::Html,
            render_mode: RenderMode::Static,
            md_state: crate::md::MdState::default(),
            context_parts,
            rsjs_artifact_transaction: OnceCell::new(),
            rsjs_shared_state_transaction: None,
            loader: Arc::new(NoLoader),
            stream_recorder: None,
            stream_chunk_flusher: None,
            boundary_error_fallbacks: None,
        }
    }
}

impl<B: WriteBuf> WebContext<DynLoaderHandle, B> {
    /// Install a type-erased loader into a context. Used by the framework to
    /// thread an `Arc<dyn DynDataLoader>` (built from `DataResolver` /
    /// `PreparedLoader`) into the request render context.
    pub fn set_loader_dyn(&mut self, loader: Arc<dyn DynDataLoader>) {
        self.loader = Arc::new(DynLoaderHandle::new(loader));
    }

    /// Builder-style variant of [`Self::set_loader_dyn`].
    pub fn with_loader_dyn(mut self, loader: Arc<dyn DynDataLoader>) -> Self {
        self.set_loader_dyn(loader);
        self
    }

    /// Create a context with a type-erased loader.
    pub fn with_dyn_loader_buffer(out: B, loader: Arc<dyn DynDataLoader>) -> Self {
        Self::with_loader_buffer(out, Arc::new(DynLoaderHandle::new(loader)))
    }
}

impl<L: DataLoader, B: WriteBuf> WebContext<L, B> {
    /// Create a context with a custom loader.
    pub fn with_loader_buffer(out: B, loader: Arc<L>) -> Self {
        Self::with_loader_buffer_and_context_parts(out, loader, WebContextParts::new())
    }

    /// Create a detached context directly from an explicit state fork.
    ///
    /// This avoids constructing and immediately discarding a default boxed
    /// `WebContextParts` when out-of-order Suspense installs its child state.
    #[doc(hidden)]
    pub fn with_loader_buffer_and_context_parts(
        out: B,
        loader: Arc<L>,
        context_parts: WebContextParts,
    ) -> Self {
        let artifact_transaction = context_parts.rsjs_artifact_transaction.clone();
        let rsjs_shared_state_transaction =
            context_parts.rsjs_shared_state.current_transaction.get();
        let rsjs_artifact_transaction = OnceCell::new();
        if artifact_transaction.is_some() {
            let _ = rsjs_artifact_transaction.set(Rc::new(RefCell::new(artifact_transaction)));
        }

        Self {
            out,
            mode: ContentMode::Html,
            escape_dialect: EscapeDialect::Html,
            render_mode: RenderMode::Static,
            md_state: crate::md::MdState::default(),
            context_parts: Some(Box::new(context_parts)),
            rsjs_artifact_transaction,
            rsjs_shared_state_transaction,
            loader,
            stream_recorder: None,
            stream_chunk_flusher: None,
            boundary_error_fallbacks: None,
        }
    }

    /// Borrow the owned output.
    #[inline]
    pub fn output(&self) -> &B {
        &self.out
    }

    /// Mutably borrow the owned output.
    #[inline]
    pub fn output_mut(&mut self) -> &mut B {
        &mut self.out
    }

    /// Recover the owned output after rendering.
    #[inline]
    pub fn into_output(self) -> B {
        self.out
    }

    fn state_slots(&mut self) -> ScopedWebState<'_, L> {
        let WebContext {
            out: _,
            mode,
            escape_dialect,
            render_mode,
            md_state,
            context_parts,
            rsjs_artifact_transaction,
            rsjs_shared_state_transaction,
            loader,
            stream_recorder,
            stream_chunk_flusher,
            boundary_error_fallbacks,
        } = self;
        ScopedWebState {
            mode,
            escape_dialect,
            render_mode,
            md_state,
            context_parts,
            rsjs_artifact_transaction,
            rsjs_shared_state_transaction,
            loader,
            stream_recorder,
            stream_chunk_flusher,
            boundary_error_fallbacks,
        }
    }

    fn output_and_state(&mut self) -> (&mut B, ScopedWebState<'_, L>) {
        let WebContext {
            out,
            mode,
            escape_dialect,
            render_mode,
            md_state,
            context_parts,
            rsjs_artifact_transaction,
            rsjs_shared_state_transaction,
            loader,
            stream_recorder,
            stream_chunk_flusher,
            boundary_error_fallbacks,
        } = self;
        (
            out,
            ScopedWebState {
                mode,
                escape_dialect,
                render_mode,
                md_state,
                context_parts,
                rsjs_artifact_transaction,
                rsjs_shared_state_transaction,
                loader,
                stream_recorder,
                stream_chunk_flusher,
                boundary_error_fallbacks,
            },
        )
    }

    /// Temporarily render through another output buffer while preserving this
    /// context's complete request and render state.
    ///
    /// This is a scoped output-type reborrow, not a side-channel snapshot:
    /// mutations made through the temporary context are moved back into the
    /// parent on success, error, or panic. The boxed parts and optional lazy
    /// channels are transferred without cloning, and the ordinary path
    /// performs no heap allocation.
    ///
    /// It is primarily used by wrappers such as metered and reservation-scoped
    /// buffers, whose concrete output type differs from the caller's buffer.
    pub fn with_output<C, R>(
        &mut self,
        out: C,
        render: impl FnOnce(&mut WebContext<L, C>) -> R,
    ) -> R
    where
        C: WriteBuf,
    {
        let _excluded = self
            .context_parts
            .as_ref()
            .and_then(|p| p.head.document())
            .map(|d| d.exclude());
        let parent = self.state_slots();
        with_scoped_web_output(parent, out, render)
    }

    /// Capture a sync component using its standard owned writer. The scope
    /// restores formatting and stream hooks on success, errors, and panics.
    #[cfg(feature = "html-to-markdown")]
    pub(crate) fn capture_html_for_markdown<T>(
        &mut self,
        node: T,
    ) -> Result<RenderBuffer, crate::WriteError>
    where
        T: crate::WebRenderSync<L>,
    {
        let _excluded = self
            .context_parts
            .as_ref()
            .and_then(|p| p.head.document())
            .map(|d| d.exclude());
        let mut output = ScopedWebOutput::new(self.state_slots(), RenderBuffer::new());
        let child = output.child_mut();
        let mut scope = HtmlCaptureScope {
            md_state: std::mem::take(&mut child.md_state),
            mode: std::mem::replace(&mut child.mode, ContentMode::Html),
            escape_dialect: std::mem::replace(&mut child.escape_dialect, EscapeDialect::Html),
            render_mode: std::mem::replace(&mut child.render_mode, RenderMode::Static),
            stream_recorder: child.stream_recorder.take(),
            stream_chunk_flusher: child.stream_chunk_flusher.take(),
            output,
        };
        let child = scope.output.child_mut();
        node.render(child)?;
        Ok(std::mem::take(&mut child.out))
    }

    /// Render through this context's own output under one activation meter.
    ///
    /// The child context receives a [`MeteredWriteBuf`] over a scoped borrow of
    /// the parent's physical output. Context state and side-channel mutations
    /// are restored on success, error, and panic, and the ordinary path does
    /// not allocate.
    pub fn with_metered_output<H, R>(
        &mut self,
        meter: H,
        render: impl FnOnce(&mut WebContext<L, MeteredWriteBuf<&mut B, H>>) -> R,
    ) -> R
    where
        H: OutputMeterHandle,
    {
        let (out, parent) = self.output_and_state();
        let metered = MeteredWriteBuf::new(out, meter);
        with_scoped_web_output(parent, metered, render)
    }

    /// Render HTML into Markdown with the same loader and live request state.
    ///
    /// Every successful write updates the logical Markdown suffix, including
    /// static, shared, and in-place writes. HTML escaping and the caller's
    /// Markdown formatting are scoped independently of the observed suffix.
    /// All scopes restore on errors and panics without allocating a context.
    pub fn render_html_in_md<'scope, T>(&'scope mut self, node: T) -> Result<(), crate::WriteError>
    where
        T: crate::WebRenderSync<L, crate::md::context::ObservedMdWriteBuf<'scope, B>>,
    {
        let (out, parent) = self.output_and_state();
        let saved = *parent.md_state;
        let mode = *parent.mode;
        let escape_dialect = *parent.escape_dialect;
        let observed = crate::md::context::ObservedMdWriteBuf::new(out, saved);
        let mut scope = HtmlInMdScope {
            output: ScopedWebOutput::new(parent, observed),
            saved,
            mode,
            escape_dialect,
        };
        let child = scope.output.child_mut();
        child.mode = ContentMode::Html;
        child.escape_dialect = EscapeDialect::Html;
        node.render(child)
    }

    /// Async counterpart to [`Self::with_metered_output`].
    ///
    /// The scoped child and metered output remain alive for the complete
    /// future, so async renderers may borrow the child across `.await` without
    /// boxing. The workspace MSRV supports async closures and the
    /// [`AsyncFnOnce`] bound used here.
    pub async fn with_metered_output_async<H, R>(
        &mut self,
        meter: H,
        render: impl AsyncFnOnce(&mut WebContext<L, MeteredWriteBuf<&mut B, H>>) -> R,
    ) -> R
    where
        H: OutputMeterHandle,
    {
        let (out, parent) = self.output_and_state();
        let metered = MeteredWriteBuf::new(out, meter);
        let mut scoped = ScopedWebOutput::new(parent, metered);
        render(scoped.child_mut()).await
    }

    /// Returns the active render mode.
    pub fn render_mode(&self) -> RenderMode {
        self.render_mode
    }

    /// Set the render mode on this context.
    pub fn set_render_mode(&mut self, render_mode: RenderMode) {
        self.render_mode = render_mode;
    }

    /// Builder-style render-mode setter.
    pub fn with_render_mode(mut self, render_mode: RenderMode) -> Self {
        self.render_mode = render_mode;
        self
    }

    /// Returns the configured request-scoped data loader.
    pub fn loader(&self) -> &Arc<L> {
        &self.loader
    }

    fn ensure_context_parts(&self) -> &WebContextParts {
        self.context_parts
            .as_deref()
            .expect("a usable web context always owns its context parts")
    }

    fn ensure_context_parts_mut(&mut self) -> &mut WebContextParts {
        self.context_parts
            .as_deref_mut()
            .expect("a usable web context always owns its context parts")
    }

    fn rsjs_artifact_transaction_slot(&self) -> &Rc<RefCell<Option<Rc<RsjsArtifactTransaction>>>> {
        self.rsjs_artifact_transaction
            .get_or_init(|| Rc::new(RefCell::new(None)))
    }

    fn current_rsjs_artifact_transaction(&self) -> Option<Rc<RsjsArtifactTransaction>> {
        self.rsjs_artifact_transaction
            .get()
            .and_then(|current| current.borrow().clone())
    }

    /// Returns the request metadata available during render.
    pub fn request(&self) -> &RequestContext {
        &self.ensure_context_parts().request
    }

    /// Returns mutable request metadata for framework setup code.
    pub fn request_mut(&mut self) -> &mut RequestContext {
        &mut self.ensure_context_parts_mut().request
    }

    /// Replace the request metadata available during render.
    pub fn set_request(&mut self, request: RequestContext) {
        self.ensure_context_parts_mut().request = request;
    }

    /// Builder-style request metadata setter.
    pub fn with_request(mut self, request: RequestContext) -> Self {
        self.set_request(request);
        self
    }

    /// Returns the staged response metadata side channel.
    pub fn response(&self) -> &ResponseContext {
        &self.ensure_context_parts().response
    }

    /// Returns mutable staged response metadata for render code.
    pub fn response_mut(&mut self) -> &mut ResponseContext {
        &mut self.ensure_context_parts_mut().response
    }

    /// Replace the staged response metadata side channel.
    pub fn set_response(&mut self, response: ResponseContext) {
        self.ensure_context_parts_mut().response = response;
    }

    /// Builder-style response metadata setter.
    pub fn with_response(mut self, response: ResponseContext) -> Self {
        self.set_response(response);
        self
    }

    /// Returns the page-level head metadata accumulator.
    pub fn head_state(&self) -> &HeadContext {
        &self.ensure_context_parts().head
    }

    /// Returns the mutable page-level head metadata accumulator.
    pub fn head_mut(&mut self) -> HeadWriter<'_> {
        HeadWriter::new(&mut self.ensure_context_parts_mut().head)
    }

    /// Replace the page-level head metadata accumulator.
    pub fn set_head(&mut self, head: HeadContext) {
        self.ensure_context_parts_mut().set_head(head);
    }

    /// Builder-style head metadata setter.
    pub fn with_head(mut self, head: HeadContext) -> Self {
        self.set_head(head);
        self
    }

    /// Returns the cache dependency trace side channel.
    pub fn cache_trace(&self) -> &CacheTrace {
        &self.ensure_context_parts().cache_trace
    }

    /// Returns the mutable cache dependency trace side channel.
    pub fn cache_trace_mut(&mut self) -> &mut CacheTrace {
        &mut self.ensure_context_parts_mut().cache_trace
    }

    /// Replace the cache dependency trace side channel.
    pub fn set_cache_trace(&mut self, cache_trace: CacheTrace) {
        self.ensure_context_parts_mut().cache_trace = cache_trace;
    }

    /// Builder-style cache trace setter.
    pub fn with_cache_trace(mut self, cache_trace: CacheTrace) -> Self {
        self.set_cache_trace(cache_trace);
        self
    }

    /// Returns the render telemetry side channel.
    pub fn telemetry(&self) -> &RenderTelemetry {
        &self.ensure_context_parts().telemetry
    }

    /// Returns the mutable render telemetry side channel.
    pub fn telemetry_mut(&mut self) -> &mut RenderTelemetry {
        &mut self.ensure_context_parts_mut().telemetry
    }

    /// Replace the render telemetry side channel.
    pub fn set_telemetry(&mut self, telemetry: RenderTelemetry) {
        self.ensure_context_parts_mut().telemetry = telemetry;
    }

    /// Builder-style telemetry setter.
    pub fn with_telemetry(mut self, telemetry: RenderTelemetry) -> Self {
        self.set_telemetry(telemetry);
        self
    }

    /// Returns typed request-local extension storage.
    pub fn locals(&self) -> &Locals {
        &self.ensure_context_parts().locals
    }

    /// Returns mutable typed request-local extension storage.
    pub fn locals_mut(&mut self) -> &mut Locals {
        &mut self.ensure_context_parts_mut().locals
    }

    /// Replace typed request-local extension storage.
    pub fn set_locals(&mut self, locals: Locals) {
        self.ensure_context_parts_mut().locals = locals;
    }

    /// Builder-style typed locals setter.
    pub fn with_locals(mut self, locals: Locals) -> Self {
        self.set_locals(locals);
        self
    }

    /// Returns the request's validated Content Security Policy nonce.
    pub fn csp_nonce(&self) -> Option<&CspNonce> {
        self.ensure_context_parts().csp_nonce.as_ref()
    }

    /// Clone the request nonce for a render value that also mutably borrows
    /// this context. Cloning only increments an `Arc`.
    pub fn csp_nonce_cloned(&self) -> Option<CspNonce> {
        self.csp_nonce().cloned()
    }

    /// Install a validated request-scoped Content Security Policy nonce.
    pub fn set_csp_nonce(&mut self, nonce: CspNonce) {
        self.ensure_context_parts_mut().set_csp_nonce(nonce);
    }

    /// Builder-style CSP nonce setter.
    pub fn with_csp_nonce(mut self, nonce: CspNonce) -> Self {
        self.set_csp_nonce(nonce);
        self
    }

    /// Fork request-scoped state for detached rendering.
    ///
    /// Ordinary accumulators are copied so mutations remain branch-local.
    /// The RSJS artifact and SharedState registries establish shared identity
    /// here because a fork created before either registry is touched must still
    /// observe later response finalization and transaction state.
    pub fn context_parts(&mut self) -> WebContextParts {
        let artifact_transaction = self.current_rsjs_artifact_transaction();
        let shared_state_transaction = self.rsjs_shared_state_transaction;
        let parts = self.ensure_context_parts_mut();
        let snapshot = WebContextParts {
            request: parts.request.clone(),
            response: parts.response.fork_for_detached(),
            head: parts.head.detached(),
            cache_trace: parts.cache_trace.clone(),
            telemetry: parts.telemetry.clone(),
            locals: parts.locals.clone(),
            csp_nonce: parts.csp_nonce.clone(),
            rsjs_artifacts: parts.rsjs_artifacts.fork(),
            rsjs_artifact_transaction: artifact_transaction,
            rsjs_shared_state: parts.rsjs_shared_state.fork(),
        };
        snapshot
            .rsjs_shared_state
            .current_transaction
            .set(shared_state_transaction);
        snapshot
    }

    /// Replace all forkable request-scoped state.
    pub fn set_context_parts(&mut self, parts: WebContextParts) {
        if let Some(document) = self.context_parts.as_ref().and_then(|p| p.head.document()) {
            document.reject_replacement();
            return;
        }
        let artifact_transaction = parts.rsjs_artifact_transaction.clone();
        self.rsjs_artifact_transaction = OnceCell::new();
        if artifact_transaction.is_some() {
            let _ = self
                .rsjs_artifact_transaction
                .set(Rc::new(RefCell::new(artifact_transaction)));
        }
        self.rsjs_shared_state_transaction = parts.rsjs_shared_state.current_transaction.get();
        *self.ensure_context_parts_mut() = parts;
    }

    /// Builder-style request-scoped side-channel setter.
    pub fn with_context_parts(mut self, parts: WebContextParts) -> Self {
        self.set_context_parts(parts);
        self
    }

    /// Returns the current request location metadata, if any.
    pub fn request_location(&self) -> Option<&RequestLocation> {
        self.ensure_context_parts().request.location()
    }

    /// Set the request location on this context.
    pub fn set_request_location(&mut self, request_location: RequestLocation) {
        self.ensure_context_parts_mut()
            .request
            .set_location(request_location);
    }

    /// Builder-style request-location setter.
    pub fn with_request_location(mut self, request_location: RequestLocation) -> Self {
        self.set_request_location(request_location);
        self
    }

    /// Returns the current matched route params, if any.
    pub fn route_params(&self) -> Option<&RouteParams> {
        self.ensure_context_parts().request.route_params()
    }

    /// Return one matched route param by name.
    pub fn param(&self, key: &str) -> Option<&str> {
        self.ensure_context_parts().request.param(key)
    }

    /// Set the matched route params on this context.
    pub fn set_route_params(&mut self, route_params: RouteParams) {
        self.ensure_context_parts_mut()
            .request
            .set_route_params(route_params);
    }

    /// Builder-style route-param setter.
    pub fn with_route_params(mut self, route_params: RouteParams) -> Self {
        self.set_route_params(route_params);
        self
    }

    /// Returns the configured streaming boundary recorder, if any.
    pub fn stream_recorder(&self) -> Option<&Rc<dyn StreamBoundaryRecorder>> {
        self.stream_recorder.as_ref()
    }

    /// Install the streaming boundary recorder on this context.
    pub fn set_stream_recorder(&mut self, stream_recorder: Rc<dyn StreamBoundaryRecorder>) {
        self.stream_recorder = Some(stream_recorder);
    }

    /// Builder-style streaming recorder setter.
    pub fn with_stream_recorder(mut self, stream_recorder: Rc<dyn StreamBoundaryRecorder>) -> Self {
        self.stream_recorder = Some(stream_recorder);
        self
    }

    /// Returns the configured in-order stream chunk flusher, if any.
    pub fn stream_chunk_flusher(&self) -> Option<&Rc<dyn StreamChunkFlusher>> {
        self.stream_chunk_flusher.as_ref()
    }

    /// Install the in-order stream chunk flusher on this context.
    pub fn set_stream_chunk_flusher(&mut self, stream_chunk_flusher: Rc<dyn StreamChunkFlusher>) {
        self.stream_chunk_flusher = Some(stream_chunk_flusher);
    }

    /// Builder-style in-order stream chunk flusher setter.
    pub fn with_stream_chunk_flusher(
        mut self,
        stream_chunk_flusher: Rc<dyn StreamChunkFlusher>,
    ) -> Self {
        self.stream_chunk_flusher = Some(stream_chunk_flusher);
        self
    }

    /// Push an error fallback for children rendered under an `ErrorBoundary`.
    pub fn push_boundary_error_fallback(
        &mut self,
        fallback: BoundaryErrorFallbackHandle<L>,
    ) -> BoundaryErrorFallbackScopeGuard<L> {
        let stack = self.boundary_error_fallbacks.get_or_insert_with(|| {
            Rc::new(RefCell::new(BoundaryErrorFallbackStackEntries::default()))
        });
        stack.borrow_mut().push(Rc::clone(&fallback));
        BoundaryErrorFallbackScopeGuard {
            stack: Rc::clone(stack),
            fallback,
        }
    }

    /// Pop the nearest error fallback.
    pub fn pop_boundary_error_fallback(&mut self) -> Option<BoundaryErrorFallbackHandle<L>> {
        self.boundary_error_fallbacks
            .as_ref()
            .and_then(|stack| stack.borrow_mut().pop())
    }

    /// Return the nearest active error fallback, if any.
    pub fn current_boundary_error_fallback(&self) -> Option<BoundaryErrorFallbackHandle<L>> {
        self.boundary_error_fallbacks
            .as_ref()
            .and_then(|stack| stack.borrow().last().cloned())
    }

    /// Snapshot the active error fallback stack for a deferred child context.
    pub fn boundary_error_fallback_stack(&self) -> Vec<BoundaryErrorFallbackHandle<L>> {
        self.boundary_error_fallbacks
            .as_ref()
            .map_or_else(Vec::new, |stack| stack.borrow().snapshot())
    }

    /// Replace the active error fallback stack on a child context.
    pub fn set_boundary_error_fallback_stack(
        &mut self,
        stack: Vec<BoundaryErrorFallbackHandle<L>>,
    ) {
        self.boundary_error_fallbacks = if stack.is_empty() {
            None
        } else {
            let mut entries = BoundaryErrorFallbackStackEntries::default();
            for fallback in stack {
                entries.push(fallback);
            }
            Some(Rc::new(RefCell::new(entries)))
        };
    }

    /// Builder-style error fallback stack setter.
    pub fn with_boundary_error_fallback_stack(
        mut self,
        stack: Vec<BoundaryErrorFallbackHandle<L>>,
    ) -> Self {
        self.set_boundary_error_fallback_stack(stack);
        self
    }

    /// Load JSON request-scoped data through the configured [`DataLoader`].
    #[deprecated(
        note = "use load_json for JSON bytes, load_bytes for raw bytes, or load_typed for typed values"
    )]
    pub async fn load<T>(&self, key: LoadKey) -> Result<T, LoadError>
    where
        T: DeserializeOwned,
    {
        self.load_json(key).await
    }

    /// Load JSON request-scoped data with advisory policy hints.
    #[deprecated(
        note = "use load_json_with for JSON bytes, load_bytes_request for raw bytes, or load_typed_request for typed values"
    )]
    pub async fn load_with<T>(&self, key: LoadKey, hints: LoadHints) -> Result<T, LoadError>
    where
        T: DeserializeOwned,
    {
        self.load_json_request(LoadRequest::with_hints(key, hints))
            .await
    }

    /// Load JSON request-scoped data from a full [`LoadRequest`].
    #[deprecated(
        note = "use load_json_request for JSON bytes, load_bytes_request for raw bytes, or load_typed_request for typed values"
    )]
    pub async fn load_request<T>(&self, request: LoadRequest) -> Result<T, LoadError>
    where
        T: DeserializeOwned,
    {
        self.load_json_request(request).await
    }

    /// Load raw bytes by key.
    pub async fn load_bytes(&self, key: LoadKey) -> Result<Bytes, LoadError> {
        self.load_bytes_request(LoadRequest::new(key)).await
    }

    /// Load raw bytes with a full [`LoadRequest`].
    pub async fn load_bytes_request(&self, request: LoadRequest) -> Result<Bytes, LoadError> {
        self.loader.load(&request).await?.into_bytes()
    }

    /// Load JSON bytes by key and deserialize into `T`.
    pub async fn load_json<T>(&self, key: LoadKey) -> Result<T, LoadError>
    where
        T: DeserializeOwned,
    {
        self.load_json_request(LoadRequest::new(key)).await
    }

    /// Load JSON bytes by key with advisory policy hints.
    pub async fn load_json_with<T>(&self, key: LoadKey, hints: LoadHints) -> Result<T, LoadError>
    where
        T: DeserializeOwned,
    {
        self.load_json_request(LoadRequest::with_hints(key, hints))
            .await
    }

    /// Load JSON bytes from a full [`LoadRequest`] and deserialize into `T`.
    pub async fn load_json_request<T>(&self, request: LoadRequest) -> Result<T, LoadError>
    where
        T: DeserializeOwned,
    {
        let bytes = self.load_bytes_request(request).await?;
        serde_json::from_slice(bytes.as_ref()).map_err(LoadError::from)
    }

    /// Load an already-materialized typed value by key.
    pub async fn load_typed<T>(&self, key: LoadKey) -> Result<Arc<T>, LoadError>
    where
        T: Send + Sync + 'static,
    {
        self.load_typed_request(LoadRequest::new(key)).await
    }

    /// Load an already-materialized typed value with a full [`LoadRequest`].
    pub async fn load_typed_request<T>(&self, request: LoadRequest) -> Result<Arc<T>, LoadError>
    where
        T: Send + Sync + 'static,
    {
        self.loader.load(&request).await?.downcast::<T>()
    }

    /// Record that an rsjs island with the given `asset_id` was
    /// rendered into this response.
    ///
    /// Side-channel for the route-asset-graph optimisation
    /// (`rsjs/OPTIMIZATIONS.md` § 1). The rsjs proc macro emits this
    /// call only for render bodies whose strategy needs browser JS, so
    /// the set fills incrementally as hydrated islands render.
    /// `ServerOnly` rsjs output records no id. The framework adapter reads
    /// [`Self::rsjs_artifacts_snapshot`] after render to compute the exact
    /// asset graph: the `<script>` tags it emits cover only the
    /// asset ids present in this set, not the entire compile-time
    /// island manifest.
    ///
    /// `asset_id` is `&'static str` because the rsjs macro bakes the
    /// id into `.rodata` at compile time. An island that renders N
    /// times records its id N times but deduplicates to one set
    /// entry: the cost is one pointer-equality compare per render
    /// after the first.
    ///
    /// Streaming-safe: callers record before flushing the chunk that
    /// emitted the island wrapper, so the framework adapter sees the
    /// correct set when it composes the closing `</body>` script
    /// tags. Re-entrant: the recording call takes `&self` (interior
    /// mutability via `RefCell`) so a render method holding `&mut
    /// cx.out` can still record.
    pub fn rsjs_island_used(&self, asset_id: &'static str) {
        let owner = self.current_rsjs_artifact_transaction();
        self.ensure_context_parts()
            .rsjs_artifacts
            .ensure_page()
            .borrow_mut()
            .record_island(asset_id, owner);
    }

    /// Record a React island manifest name in this request's artifact ledger.
    ///
    /// The contribution is owned by the active render transaction, if any,
    /// so a cancelled or rolled-back Suspense/ErrorBoundary subtree cannot
    /// leak client bootstrap work into the response.
    pub fn react_island_used(&self, name: &str) {
        let owner = self.current_rsjs_artifact_transaction();
        self.ensure_context_parts()
            .rsjs_artifacts
            .ensure_page()
            .borrow_mut()
            .record_react_island(name, owner);
    }

    /// Allocate the next unnamed React island id for this request.
    ///
    /// Counters are shared by detached contexts belonging to the same
    /// response, but never by unrelated responses or threads.
    pub fn next_react_island_id(&self) -> u64 {
        self.ensure_context_parts()
            .rsjs_artifacts
            .ensure_page()
            .borrow_mut()
            .next_react_island_id()
    }

    /// Read-only view of the set of rsjs island asset ids that have
    /// been recorded so far on this context.
    ///
    /// Returns a guard scoped to a transient borrow on the underlying
    /// `RefCell`; clone the returned set's contents if you need to
    /// hold the data across calls that might mutate the context.
    /// Retained for compatibility with low-level adapters. New response
    /// finalizers should use [`Self::rsjs_artifacts_snapshot`], which also
    /// transports query seeds and SharedState liveness and supports
    /// speculative-branch rollback.
    pub fn rsjs_islands_used(&self) -> std::cell::Ref<'_, HashSet<&'static str>> {
        let page = self.ensure_context_parts().rsjs_artifacts.ensure_page();
        page.borrow_mut().prune_rolled_back();
        Ref::map(page.borrow(), |page| &page.islands)
    }

    /// Record one page-level rsjs query seed entry.
    ///
    /// `key_json` and `state_json` must already be JSON safe to place inside
    /// an inert `<script type="application/json">` tag. The rsjs macro
    /// obtains both through `rsjs::__private::to_script_json(...)`, so this
    /// method stores the strings verbatim instead of reparsing them.
    ///
    /// Re-registering a key in the same render owner keeps the first state,
    /// preserving the historical query-cache behavior. If independent
    /// speculative owners commit conflicting states for one key, the final
    /// deterministic snapshot keeps the lexically smallest `state_json`;
    /// render scheduling therefore cannot change the response payload.
    #[doc(hidden)]
    pub fn rsjs_query_seed_json(&self, key_json: &str, state_json: &str) {
        let owner = self.current_rsjs_artifact_transaction();
        self.ensure_context_parts()
            .rsjs_artifacts
            .ensure_page()
            .borrow_mut()
            .record_query_seed(
                RsjsQuerySeedArtifact {
                    key_json: key_json.to_owned(),
                    state_json: state_json.to_owned(),
                },
                owner,
            );
    }

    /// Return the page-level rsjs query seed table JSON, if any query seeds
    /// were recorded during this render.
    ///
    /// Shape:
    /// `{"queries":[{"key":<structural-key>,"state":<query-state>,"rank":<serialized-state-string>}, ...]}`.
    ///
    /// `rank` preserves the server's exact serialized-state ordering so
    /// independently completed streaming boundaries resolve conflicting keys
    /// the same way regardless of completion order.
    /// The framework appends this as a single inert JSON script after the
    /// root component has rendered and before rsjs bootstrap modules run.
    ///
    /// SECURITY: the stored fragments are appended through
    /// [`append_inline_script_safe_json`], which rewrites `<`, `>`,
    /// U+2028, and U+2029 into their `\uXXXX` JSON escapes. The rsjs
    /// macro already produces safe fragments via `to_script_json(...)`,
    /// but this assembler is also reachable with caller-provided
    /// strings, and a fragment containing a literal `</script>` would
    /// otherwise break out of the inline script tag the framework
    /// writes the table into.
    #[doc(hidden)]
    pub fn rsjs_query_seed_table_json(&self) -> Option<String> {
        self.rsjs_artifacts_snapshot().query_seed_table_json()
    }

    /// Snapshot the committed rsjs artifacts recorded by this response.
    ///
    /// Open speculative branches are excluded until they commit. The returned
    /// collections are sorted and deduplicated, so the value is stable across
    /// render scheduling and can be cached or passed to a response finalizer.
    pub fn rsjs_artifacts_snapshot(&self) -> RsjsRenderArtifacts {
        let Some(page) = self.ensure_context_parts().rsjs_artifacts.page.get() else {
            return RsjsRenderArtifacts::default();
        };
        let mut page = page.borrow_mut();
        page.prune_rolled_back();
        page.snapshot()
    }

    /// Snapshot every committed browser artifact recorded by this response.
    ///
    /// Open speculative branches are excluded until they commit. The result
    /// is stable across interleaving because both collections are sorted and
    /// deduplicated.
    pub fn render_artifacts_snapshot(&self) -> RenderArtifacts {
        let Some(page) = self.ensure_context_parts().rsjs_artifacts.page.get() else {
            return RenderArtifacts::default();
        };
        let mut page = page.borrow_mut();
        page.prune_rolled_back();
        page.render_snapshot()
    }

    /// Return committed response artifacts absent from `baseline`.
    pub fn rsjs_artifacts_delta_since(
        &self,
        baseline: &RsjsRenderArtifacts,
    ) -> RsjsRenderArtifacts {
        self.rsjs_artifacts_snapshot().delta_since(baseline)
    }

    /// Replay a cached fragment's artifact snapshot into the current render
    /// transaction.
    ///
    /// Replay uses the same ownership and deduplication rules as live
    /// rendering, so a cached fragment inside a discarded boundary cannot
    /// leak hydration or seed metadata into the response. SharedState
    /// references are rebound to the exact provider scope already established
    /// by this response. The cached provider value is never replayed: the
    /// current response's canonical seed wins, and a missing or schema-mismatched
    /// scope fails closed.
    pub fn replay_rsjs_artifacts(
        &self,
        artifacts: &RsjsRenderArtifacts,
    ) -> Result<(), crate::WriteError> {
        let owner = self.current_rsjs_artifact_transaction();
        let shared_state_seeds: Vec<_> = if artifacts.shared_state_seeds.is_empty() {
            Vec::new()
        } else {
            let shared = &self.ensure_context_parts().rsjs_shared_state;
            let Some(shared_page) = shared.page.get() else {
                return Err(crate::WriteError::render(
                    "rsjs/shared-state-cache-scope-missing: a cached fragment references \
                     SharedState, but this response established no provider scopes; render the \
                     provider in the current response shell or reject this cache entry",
                ));
            };
            let shared_page = shared_page.borrow();
            let mut resolved = Vec::with_capacity(artifacts.shared_state_seeds.len());
            for cached in &artifacts.shared_state_seeds {
                let Some(current) = shared_page
                    .seeds
                    .iter()
                    .find(|seed| seed.artifact.scope_id == cached.scope_id)
                else {
                    return Err(crate::WriteError::render(format!(
                        "rsjs/shared-state-cache-scope-missing: cached fragment references \
                         provider scope {} (`{}`), but that exact scope was not established by \
                         this response; response-local provider identities cannot be replayed \
                         under different topology",
                        cached.scope_id, cached.type_key,
                    )));
                };
                if current.artifact.type_key != cached.type_key
                    || current.artifact.schema_hash != cached.schema_hash
                    || current.artifact.rust_type != cached.rust_type
                {
                    return Err(crate::WriteError::render(format!(
                        "rsjs/shared-state-cache-schema-mismatch: cached provider scope {} \
                         expects `{}` ({}, {}), but this response established `{}` ({}, {}); \
                         reject the cache entry and render the consumer against the current \
                         provider topology",
                        cached.scope_id,
                        cached.type_key,
                        cached.schema_hash,
                        cached.rust_type,
                        current.artifact.type_key,
                        current.artifact.schema_hash,
                        current.artifact.rust_type,
                    )));
                }
                resolved.push(Arc::clone(&current.artifact));
            }
            resolved
        };

        let page = self.ensure_context_parts().rsjs_artifacts.ensure_page();
        let mut page = page.borrow_mut();
        for seed in &shared_state_seeds {
            page.validate_shared_state_seed(seed.as_ref())?;
        }
        for asset_id in artifacts.rendered_islands() {
            page.record_island(asset_id, owner.clone());
        }
        for seed in artifacts.query_seeds() {
            page.record_query_seed(seed.clone(), owner.clone());
        }
        for seed in shared_state_seeds {
            page.record_shared_state_seed(seed, owner.clone())?;
        }
        Ok(())
    }

    /// Start an artifact ownership transaction for a render subtree.
    #[doc(hidden)]
    pub fn rsjs_artifact_checkpoint(&self) -> RsjsArtifactCheckpoint {
        let parent = self.current_rsjs_artifact_transaction();
        let transaction = RsjsArtifactTransaction::new(parent);
        let current_slot = Rc::clone(self.rsjs_artifact_transaction_slot());
        *current_slot.borrow_mut() = Some(Rc::clone(&transaction));
        RsjsArtifactCheckpoint {
            transaction,
            current_slot,
            flush_generation: self.stream_flush_generation(),
            armed: true,
        }
    }

    /// Start a cancellation-safe artifact transaction for detached work.
    ///
    /// Dropping the returned branch rolls it back. Call
    /// [`RsjsArtifactBranch::commit`] only once the branch HTML is ready to be
    /// published.
    pub fn rsjs_artifact_branch(&self) -> RsjsArtifactBranch {
        let checkpoint = self.rsjs_artifact_checkpoint();
        let page = Rc::clone(self.ensure_context_parts().rsjs_artifacts.ensure_page());
        RsjsArtifactBranch {
            checkpoint: Some(checkpoint),
            page,
        }
    }

    /// Snapshot contributions owned by one still-open checkpoint.
    #[doc(hidden)]
    pub fn rsjs_artifacts_since(&self, checkpoint: &RsjsArtifactCheckpoint) -> RsjsRenderArtifacts {
        self.ensure_context_parts()
            .rsjs_artifacts
            .page
            .get()
            .map_or_else(RsjsRenderArtifacts::default, |page| {
                page.borrow()
                    .snapshot_for_transaction(&checkpoint.transaction)
            })
    }

    /// Commit one artifact transaction into its parent.
    #[doc(hidden)]
    pub fn rsjs_artifact_commit(&self, mut checkpoint: RsjsArtifactCheckpoint) {
        checkpoint.commit_now();
    }

    /// Roll back an artifact transaction if its corresponding markup is still
    /// recallable; otherwise preserve it because the browser may already have
    /// received markup that depends on those artifacts.
    #[doc(hidden)]
    pub fn rsjs_artifact_rollback(&self, mut checkpoint: RsjsArtifactCheckpoint) {
        if self.stream_flush_generation() != checkpoint.flush_generation {
            checkpoint.commit_now();
            return;
        }
        checkpoint.rollback_now();
        if let Some(page) = self.ensure_context_parts().rsjs_artifacts.page.get() {
            page.borrow_mut().prune_rolled_back();
        }
    }

    fn rsjs_artifact_rollback_unconditionally(&self, mut checkpoint: RsjsArtifactCheckpoint) {
        checkpoint.rollback_now();
        if let Some(page) = self.ensure_context_parts().rsjs_artifacts.page.get() {
            page.borrow_mut().prune_rolled_back();
        }
    }

    /// Enter a typed rsjs shared-state provider scope.
    ///
    /// This is an internal bridge used by `rsjs::SharedStateProvider`. The
    /// protocol identity and encoded state are supplied by the SharedState
    /// derive so `proa_core` remains independent of the rsjs crate.
    #[doc(hidden)]
    pub fn rsjs_shared_state_enter<T: 'static, W: RsjsSharedStateWidePaths>(
        &mut self,
        type_key: &'static str,
        schema_hash: &'static str,
        rust_type: &'static str,
        wide_paths: W,
        value: Rc<T>,
        state_json: String,
    ) -> Result<RsjsSharedStateScopeGuard, crate::WriteError> {
        let transaction_id = self.rsjs_shared_state_transaction;
        let wide_paths_json = wide_paths.into_wide_paths_json();
        let shared = &self.ensure_context_parts_mut().rsjs_shared_state;
        let scope_id = {
            let mut page = shared.ensure_page().borrow_mut();
            if shared.published.get() || page.seed_table_published {
                // Keep the message truthful for both finalization outcomes:
                // a table script was emitted only if the shell registered at
                // least one seed; an empty registry finalizes with no script
                // in the response at all.
                let table_note = if !page.seed_table_emitted {
                    "no committed shell consumer made a provider seed live, so the response \
                     carries no seed table script at all"
                } else {
                    "the table script was already emitted with the shell"
                };
                return Err(crate::WriteError::render(format!(
                    "rsjs/shared-state-late-stream-provider: `{rust_type}` (`{type_key}`) was \
                     provided after the page shared-state seed table was finalized for this \
                     response ({table_note}), so this scope can never reach the client; move \
                     `rsjs::SharedStateProvider {{ ... }}` above the deferred/streaming \
                     boundary so its scope is present in the shell before islands hydrate"
                )));
            }
            let scope_id = page.next_scope_id;
            let next_scope_id = page.next_scope_id.checked_add(1).ok_or_else(|| {
                crate::WriteError::render(
                    "rsjs/shared-state-scope-overflow: provider scope id space exhausted",
                )
            })?;

            if let Some((registered_schema, registered_rust_type)) = page.schemas.get(type_key) {
                if *registered_schema != schema_hash || *registered_rust_type != rust_type {
                    return Err(crate::WriteError::render(format!(
                        "rsjs/shared-state-type-key-conflict: protocol key `{type_key}` was \
                         registered for `{registered_rust_type}` ({registered_schema}) and \
                         cannot also identify `{rust_type}` ({schema_hash}); choose a unique \
                         #[rsjs(type_key = \"...\")] or rebuild every consumer with one schema"
                    )));
                }
            } else {
                page.schemas.insert(type_key, (schema_hash, rust_type));
            }

            page.next_scope_id = next_scope_id;
            page.seeds.push(RsjsSharedStateSeed {
                artifact: Arc::new(RsjsSharedStateSeedArtifact {
                    scope_id,
                    type_key,
                    schema_hash,
                    rust_type,
                    wide_paths_json,
                    state_json,
                }),
                transaction_id,
            });
            scope_id
        };

        let value: Rc<dyn Any> = value;
        let active = shared.ensure_active();
        active.borrow_mut().push(RsjsSharedStateBinding {
            scope_id,
            type_key,
            schema_hash,
            rust_type,
            value,
            linked_occurrence: None,
        });
        Ok(RsjsSharedStateScopeGuard {
            active: Rc::clone(active),
            scope_id,
        })
    }

    /// Tag the current provider with its exact compiler-linked occurrence.
    ///
    /// Generated RSJS server lowering calls this only while rendering the
    /// `children` field of the provider that was just entered. A binding may
    /// carry at most one active linked tag; accepting nested replacement would
    /// make an out-of-order guard drop capable of restoring stale topology.
    #[doc(hidden)]
    pub fn rsjs_shared_state_tag_linked_provider(
        &self,
        occurrence: RsjsLinkedProviderOccurrenceKey,
    ) -> Result<RsjsLinkedProviderTagGuard, crate::WriteError> {
        let Some(active) = self.ensure_context_parts().rsjs_shared_state.active.get() else {
            return Err(crate::WriteError::render(format!(
                "rsjs/shared-state-linked-provider-tag-without-scope: linked provider \
                 occurrence {occurrence:?} has no active SharedStateProvider to tag; the \
                 compiler must wrap the `children` of the provider it just entered"
            )));
        };
        let mut bindings = active.borrow_mut();
        let Some(binding) = bindings.last_mut() else {
            return Err(crate::WriteError::render(format!(
                "rsjs/shared-state-linked-provider-tag-without-scope: linked provider \
                 occurrence {occurrence:?} has no active SharedStateProvider to tag; the \
                 compiler must wrap the `children` of the provider it just entered"
            )));
        };
        if let Some(existing) = binding.linked_occurrence {
            return Err(crate::WriteError::render(format!(
                "rsjs/shared-state-linked-provider-double-tag: provider scope {} is already \
                 tagged as {existing:?} and cannot also be tagged as {occurrence:?}; generated \
                 linked provider wrappers must be unique per provider occurrence",
                binding.scope_id
            )));
        }
        binding.linked_occurrence = Some(occurrence);
        let scope_id = binding.scope_id;
        drop(bindings);
        Ok(RsjsLinkedProviderTagGuard {
            active: Rc::clone(active),
            scope_id,
            occurrence,
        })
    }

    /// Acquire the nearest active provider snapshot for one shared-state type.
    #[doc(hidden)]
    pub fn rsjs_shared_state_acquire<T: 'static>(
        &self,
        type_key: &'static str,
        schema_hash: &'static str,
        rust_type: &'static str,
        island: &'static str,
    ) -> Result<(Rc<T>, u64), crate::WriteError> {
        let shared = &self.ensure_context_parts().rsjs_shared_state;
        let binding = shared
            .active
            .get()
            .and_then(|active| {
                active
                    .borrow()
                    .iter()
                    .rev()
                    .find(|binding| binding.type_key == type_key)
                    .cloned()
            })
            .ok_or_else(|| {
                crate::WriteError::render(format!(
                    "rsjs/shared-state-missing-provider: island `{island}` requires \
                     `{rust_type}` (`{type_key}`), but no matching provider is active; wrap \
                     the owning page region in `rsjs::SharedStateProvider {{ state: \
                     {rust_type} {{ ... }}, children: ... }}`"
                ))
            })?;
        self.rsjs_shared_state_finish_acquire(
            binding,
            type_key,
            schema_hash,
            rust_type,
            island,
            "nearest provider",
        )
    }

    /// Acquire only the exact provider occurrence selected by the final linker.
    ///
    /// This never falls back to the nearest provider with the same `type_key`.
    /// Nested and sibling providers of one schema therefore retain distinct
    /// response-local scopes even when ordinary lexical lookup would select a
    /// different binding.
    #[doc(hidden)]
    pub fn rsjs_shared_state_acquire_linked<T: 'static>(
        &self,
        occurrence: RsjsLinkedProviderOccurrenceKey,
        type_key: &'static str,
        schema_hash: &'static str,
        rust_type: &'static str,
        island: &'static str,
    ) -> Result<(Rc<T>, u64), crate::WriteError> {
        let shared = &self.ensure_context_parts().rsjs_shared_state;
        let binding = {
            let active = shared.active.get().map(|active| active.borrow());
            let mut matches = active
                .as_deref()
                .into_iter()
                .flat_map(|active| active.iter().rev())
                .filter(|binding| binding.linked_occurrence == Some(occurrence));
            let binding = matches.next().cloned();
            if matches.next().is_some() {
                return Err(crate::WriteError::render(format!(
                    "rsjs/shared-state-linked-provider-ambiguous: island `{island}` resolved \
                     linked provider occurrence {occurrence:?} more than once in the active \
                     provider stack; final provider topology must identify exactly one binding"
                )));
            }
            binding
        };
        let Some(binding) = binding else {
            return Err(crate::WriteError::render(format!(
                "rsjs/shared-state-linked-provider-missing: island `{island}` requires \
                 `{rust_type}` (`{type_key}`) from exact linked provider occurrence \
                 {occurrence:?}, but that occurrence is not active; this lookup does not fall \
                 back to another provider with the same protocol key"
            )));
        };
        self.rsjs_shared_state_finish_acquire(
            binding,
            type_key,
            schema_hash,
            rust_type,
            island,
            "exact linked provider",
        )
    }

    fn rsjs_shared_state_finish_acquire<T: 'static>(
        &self,
        binding: RsjsSharedStateBinding,
        type_key: &'static str,
        schema_hash: &'static str,
        rust_type: &'static str,
        island: &'static str,
        provider_description: &'static str,
    ) -> Result<(Rc<T>, u64), crate::WriteError> {
        if binding.type_key != type_key
            || binding.schema_hash != schema_hash
            || binding.rust_type != rust_type
        {
            return Err(crate::WriteError::render(format!(
                "rsjs/shared-state-schema-mismatch: island `{island}` expects `{rust_type}` \
                 (`{type_key}`, {schema_hash}), but the {provider_description} contains `{}` \
                 (`{}`, {}); rebuild the provider and every consuming island with the same \
                 schema",
                binding.rust_type, binding.type_key, binding.schema_hash
            )));
        }
        let scope_id = binding.scope_id;
        let value = binding.value.downcast::<T>().map_err(|_| {
            crate::WriteError::render(format!(
                "rsjs/shared-state-rust-type-mismatch: island `{island}` resolved protocol key \
                `{type_key}` but its provider snapshot could not be downcast to `{rust_type}`; \
                 ensure each type_key belongs to exactly one Rust type"
            ))
        })?;
        let seed = {
            let shared = &self.ensure_context_parts().rsjs_shared_state;
            let page = shared
                .page
                .get()
                .expect("an active provider must belong to a shared-state page")
                .borrow();
            Arc::clone(
                &page
                    .seeds
                    .iter()
                    .find(|seed| seed.artifact.scope_id == scope_id)
                    .expect("an active provider must retain its registered seed")
                    .artifact,
            )
        };
        let owner = self.current_rsjs_artifact_transaction();
        self.ensure_context_parts()
            .rsjs_artifacts
            .ensure_page()
            .borrow_mut()
            .record_shared_state_seed(seed, owner)?;
        Ok((value, scope_id))
    }

    /// Assemble the page shared-state seed table for this response, if any
    /// committed consumer acquired a registered provider scope during render.
    ///
    /// Returns the JSON payload for the inert
    /// `<script type="application/json" id="rsjs-shared-state-seed-table">`
    /// element the rsjs browser runtime reads before hydration, shaped as
    /// `{"abi_version":1,"stores":[{"scope_id":...,"type_key":...,
    /// "schema_hash":...,"rust_type":...,"state":...},...]}`. Stores whose
    /// schema contains wide-int fields (the i64 family, encoded as JSON
    /// strings on the wire) additionally carry `"wide_paths":[[...],...]`.
    /// Each path locates a value to decode to BigInt and `"*"` traverses Vec
    /// elements; the key is omitted when the schema has no wide values.
    /// Provider registration alone does not expose its value. Returns `None`
    /// when no committed consumer made a provider live, including when unused
    /// providers were present in the server shell.
    ///
    /// The framework response paths (sync, async, in-order streaming, and
    /// out-of-order streaming) call this automatically after the shell has
    /// rendered and embed the result before the rsjs bootstrap script. Call
    /// it yourself only when you bypass those paths and embed rendered HTML
    /// manually: render the page, call this method once, and write the
    /// returned JSON into a script element with the id above, after the
    /// rendered markup and before any rsjs bootstrap module runs.
    ///
    /// Calling this finalizes the table for the whole response. Any provider
    /// that enters afterwards, on this context or on one forked from it,
    /// fails with `rsjs/shared-state-late-stream-provider`, because a scope
    /// registered after finalization can never reach the client.
    ///
    /// SECURITY: every fragment (the protocol metadata and the state
    /// payload) is appended through the same inline-script-safe escaping as
    /// the query seed table, so the returned JSON cannot contain a literal
    /// `</script>` or the JS line separators U+2028/U+2029.
    pub fn rsjs_shared_state_seed_table_json(&self) -> Option<String> {
        let shared = &self.ensure_context_parts().rsjs_shared_state;
        shared.published.set(true);
        let seed_table_json = self
            .rsjs_artifacts_snapshot()
            .shared_state_seed_table_json();
        if let Some(page) = shared.page.get() {
            let mut page = page.borrow_mut();
            page.seed_table_published = true;
            page.seed_table_emitted = seed_table_json.is_some();
        }
        seed_table_json
    }

    /// Begin an ownership transaction for shared-state seeds registered by
    /// a subtree, so those exact seeds can be rolled back if the subtree is
    /// discarded.
    ///
    /// `ErrorBoundary` takes this checkpoint alongside its output-buffer
    /// checkpoint before rendering the child; if the child fails and its
    /// bytes are rewound, [`Self::rsjs_shared_state_rollback_seeds`] removes
    /// the seeds the discarded subtree registered so they do not ship to the
    /// client. Context forks inherit the transaction id, while forks made
    /// before this checkpoint keep their previous id; a failed branch can
    /// therefore never truncate a surviving sibling's seeds. Cold-parts
    /// safe: no side channel is materialized by taking a checkpoint.
    ///
    /// The checkpoint also records the stream flush generation, so
    /// [`Self::rsjs_shared_state_rollback_seeds`] can tell a genuine rewind
    /// from a rewind of a buffer whose earlier contents were already
    /// streamed to the client.
    #[doc(hidden)]
    pub fn rsjs_shared_state_seed_checkpoint(&mut self) -> RsjsSharedStateSeedCheckpoint {
        let parent_transaction_id = self.rsjs_shared_state_transaction;
        let transaction_id = NEXT_RSJS_SHARED_STATE_TRANSACTION_ID.fetch_add(1, Ordering::Relaxed);
        assert_ne!(
            transaction_id, 0,
            "rsjs shared-state transaction id space exhausted"
        );
        self.rsjs_shared_state_transaction = Some(transaction_id);
        RsjsSharedStateSeedCheckpoint {
            transaction_id,
            parent_transaction_id,
            flush_generation: self.stream_flush_generation(),
        }
    }

    /// Stream flush generation as seen from this context, without
    /// materializing the response side channel on a cold page.
    fn stream_flush_generation(&self) -> u64 {
        self.ensure_context_parts()
            .response
            .stream_flush_generation()
    }

    /// Whether markup written since `checkpoint` is still recallable, i.e.
    /// nothing has been streamed to the client since it was taken.
    ///
    /// Rewinding an output buffer only discards markup while the whole
    /// subtree is still buffered. Once a chunk has been flushed, the bytes
    /// are on the wire and the buffer offset recorded at the checkpoint
    /// describes a buffer that no longer exists, so a successful
    /// `rewind_to` says nothing about what the client already received.
    #[doc(hidden)]
    pub fn rsjs_shared_state_seeds_are_recallable(
        &self,
        checkpoint: &RsjsSharedStateSeedCheckpoint,
    ) -> bool {
        self.stream_flush_generation() == checkpoint.flush_generation
    }

    /// Commit a seed transaction into its parent boundary, if any.
    #[doc(hidden)]
    pub fn rsjs_shared_state_commit_seeds(&mut self, checkpoint: RsjsSharedStateSeedCheckpoint) {
        if let Some(page) = self.ensure_context_parts().rsjs_shared_state.page.get() {
            for seed in &mut page.borrow_mut().seeds {
                if seed.transaction_id == Some(checkpoint.transaction_id) {
                    seed.transaction_id = checkpoint.parent_transaction_id;
                }
            }
        }
        if self.rsjs_shared_state_transaction == Some(checkpoint.transaction_id) {
            self.rsjs_shared_state_transaction = checkpoint.parent_transaction_id;
        }
    }

    /// Discard every rsjs shared-state seed owned by `checkpoint`.
    ///
    /// Call this only when the output written since the matching
    /// [`Self::rsjs_shared_state_seed_checkpoint`] has been rewound (an
    /// `ErrorBoundary` recovering with its fallback): the removed seeds
    /// belong to markup that no longer exists, so shipping them would bloat
    /// the payload and install browser stores no island references. Scope
    /// ids intentionally skip. The protocol schema registry is rebuilt from
    /// surviving seeds so a schema introduced only by the dead branch cannot
    /// poison a later provider using that type key.
    ///
    /// If markup was streamed to the client after the checkpoint was taken
    /// (see [`Self::rsjs_shared_state_seeds_are_recallable`]) the subtree's
    /// bytes are unrecallable, so the seeds are committed to the parent
    /// transaction instead of discarded: an island the browser can already
    /// see must keep the store it hydrates from.
    #[doc(hidden)]
    pub fn rsjs_shared_state_rollback_seeds(&mut self, checkpoint: RsjsSharedStateSeedCheckpoint) {
        if !self.rsjs_shared_state_seeds_are_recallable(&checkpoint) {
            let stranded = self.rsjs_shared_state_seed_count_owned_by(checkpoint.transaction_id);
            if stranded > 0 {
                tracing::warn!(
                    seeds = stranded,
                    "shared-state seeds kept after a boundary recovery: part of the discarded \
                     subtree was already streamed to the client, so dropping its seeds would \
                     leave shipped islands with no store to hydrate from. The seeds ship with \
                     the response; move the failing work above the enclosing Suspense if the \
                     state must not reach the client on failure."
                );
            }
            self.rsjs_shared_state_commit_seeds(checkpoint);
            return;
        }
        if let Some(page) = self.ensure_context_parts().rsjs_shared_state.page.get() {
            let mut page = page.borrow_mut();
            page.seeds
                .retain(|seed| seed.transaction_id != Some(checkpoint.transaction_id));
            page.schemas = page
                .seeds
                .iter()
                .map(|seed| {
                    (
                        seed.artifact.type_key,
                        (seed.artifact.schema_hash, seed.artifact.rust_type),
                    )
                })
                .collect();
        }
        if self.rsjs_shared_state_transaction == Some(checkpoint.transaction_id) {
            self.rsjs_shared_state_transaction = checkpoint.parent_transaction_id;
        }
    }

    /// Number of registered seeds owned by `transaction_id`. Diagnostics
    /// only, so it stays on the cold path and never materializes the page.
    fn rsjs_shared_state_seed_count_owned_by(&self, transaction_id: u64) -> usize {
        self.ensure_context_parts()
            .rsjs_shared_state
            .page
            .get()
            .map_or(0, |page| {
                page.borrow()
                    .seeds
                    .iter()
                    .filter(|seed| seed.transaction_id == Some(transaction_id))
                    .count()
            })
    }

    /// Enter a new content mode, returning the previous mode to restore
    /// with [`WebContext::restore_mode`].
    ///
    /// The content mode selects the escaping applied to dynamic text:
    /// [`ContentMode::Html`] uses HTML text escaping, while
    /// [`ContentMode::Script`] / [`ContentMode::Style`] use the raw-text
    /// sentinel masking that keeps interpolated text from closing a
    /// `<script>` / `<style>` element early. The `html!` / `html_sync!`
    /// macros manage the mode automatically; runtime renderers that emit
    /// raw-text regions themselves (interpreted page programs, custom
    /// writers) call this pair directly. Always restore the returned
    /// previous mode when the raw-text region ends.
    pub fn enter_mode(&mut self, mode: ContentMode) -> ContentMode {
        std::mem::replace(&mut self.mode, mode)
    }

    /// Restore a previous content mode returned by
    /// [`WebContext::enter_mode`].
    pub fn restore_mode(&mut self, prev: ContentMode) {
        self.mode = prev;
    }

    /// The active content mode for escaping purposes.
    pub fn current_mode(&self) -> ContentMode {
        self.mode
    }

    /// Enter a content mode (`Script` / `Style`) for proc-macro generated
    /// render fragments, returning the previous mode to restore later.
    ///
    /// `html!` / `html_sync!` wrap the dynamic children of `<script>` and
    /// `<style>` elements with this so interpolated text is escaped with the
    /// raw-text sentinel masking (`</script`, `<script`, `<!--`) instead of
    /// plain HTML-text escaping. Without it, that masking would be unreachable
    /// on the macro path and a dynamic child of `<script>` could break out of
    /// the element.
    #[doc(hidden)]
    pub fn __proa_enter_mode(&mut self, mode: ContentMode) -> ContentMode {
        std::mem::replace(&mut self.mode, mode)
    }

    /// Restore a previous content mode entered via [`__proa_enter_mode`].
    ///
    /// [`__proa_enter_mode`]: Self::__proa_enter_mode
    #[doc(hidden)]
    pub fn __proa_restore_mode(&mut self, prev: ContentMode) {
        self.mode = prev;
    }

    /// Enter an escape dialect for proc-macro generated render fragments.
    #[doc(hidden)]
    pub fn __proa_enter_escape_dialect(&mut self, dialect: EscapeDialect) -> EscapeDialect {
        std::mem::replace(&mut self.escape_dialect, dialect)
    }

    /// Restore the previous escape dialect for proc-macro generated fragments.
    #[doc(hidden)]
    pub fn __proa_restore_escape_dialect(&mut self, prev: EscapeDialect) {
        self.escape_dialect = prev;
    }

    /// Return the active escape dialect.
    #[doc(hidden)]
    pub fn __proa_current_escape_dialect(&self) -> EscapeDialect {
        self.escape_dialect
    }
}

impl<I, H, L> WebContext<L, MeteredWriteBuf<I, H>>
where
    I: WriteBuf,
    H: OutputMeterHandle,
    L: DataLoader,
{
    /// Render one pre-measured semantic operation against the physical output.
    ///
    /// `reservation` has already debited this context's activation meter. The
    /// child therefore writes through a [`ReservedWriteBuf`] over the physical
    /// buffer beneath the outer meter, avoiding a second debit. Exact
    /// completion is checked only when `render` succeeds; an earlier write
    /// error is returned unchanged. Context state is restored on every return
    /// path and during unwinding.
    pub fn with_reserved_output<R>(
        &mut self,
        reservation: OutputReservation,
        render: impl FnOnce(
            &mut WebContext<L, ReservedWriteBuf<&mut I>>,
        ) -> Result<R, crate::WriteError>,
    ) -> Result<R, crate::WriteError> {
        let (metered, parent) = self.output_and_state();
        let reserved = ReservedWriteBuf::new(metered.inner_mut(), reservation);
        let mut scoped = ScopedWebOutput::new(parent, reserved);
        let result = render(scoped.child_mut());
        let reserved = scoped
            .restore()
            .expect("scoped reserved output is restored exactly once");

        match result {
            Ok(value) => {
                reserved.finish()?;
                Ok(value)
            }
            Err(error) => {
                let _ = reserved.into_inner_unchecked();
                Err(error)
            }
        }
    }
}

/// Append a caller-provided JSON fragment to `out`, rewriting any
/// characters that could break out of an inline `<script>` context.
///
/// The assembled seed table is embedded verbatim inside an inline
/// `<script type="application/json">` tag by the framework finalizers, so
/// a fragment containing a literal `</script>` would terminate that tag
/// early and turn attacker-influenced JSON into live markup. Rewriting
/// `<` and `>` to `\u003c` / `\u003e` is JSON-equivalent: in a valid
/// fragment those bytes can only appear inside string literals, where the
/// `\uXXXX` escape decodes to the same character. U+2028 and U+2029 are
/// rewritten to `\u2028` / `\u2029` as well because they are legal
/// unescaped in JSON strings but act as line terminators if the payload
/// ever lands in a JavaScript source context.
///
/// Fast path: a fragment with none of those bytes is appended as-is, with
/// no allocation beyond the destination string's own growth.
fn append_inline_script_safe_json(out: &mut String, fragment: &str) {
    // First byte of the UTF-8 encodings of U+2028 (E2 80 A8) and
    // U+2029 (E2 80 A9). Matching on it over-selects (other U+2xxx
    // chars share it), but false positives only divert to the slow
    // path, which copies non-dangerous chars through unchanged.
    const LS_PS_LEAD_BYTE: u8 = 0xE2;
    let needs_rewrite = fragment
        .bytes()
        .any(|b| b == b'<' || b == b'>' || b == LS_PS_LEAD_BYTE);
    if !needs_rewrite {
        out.push_str(fragment);
        return;
    }
    for ch in fragment.chars() {
        match ch {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            other => out.push(other),
        }
    }
}

fn append_inline_script_safe_json_string(out: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            control if control <= '\u{1f}' => {
                let byte = control as u8;
                out.push_str("\\u00");
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0x0f) as usize] as char);
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

fn query_seed_table_json_from_artifacts(seeds: &[RsjsQuerySeedArtifact]) -> Option<String> {
    if seeds.is_empty() {
        return None;
    }

    let mut json = String::from("{\"queries\":[");
    for (index, seed) in seeds.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str("{\"key\":");
        append_inline_script_safe_json(&mut json, &seed.key_json);
        json.push_str(",\"state\":");
        append_inline_script_safe_json(&mut json, &seed.state_json);
        // Preserve Rust's exact `String::cmp` input on the browser wire.
        // JavaScript's native string comparison uses UTF-16 code units, so
        // the runtime compares this raw serialized state as UTF-8 bytes.
        // Keeping the raw rank is also materially smaller than hex encoding
        // large dehydrated query payloads.
        json.push_str(",\"rank\":");
        append_inline_script_safe_json_string(&mut json, &seed.state_json);
        json.push('}');
    }
    json.push_str("]}");
    Some(json)
}

fn shared_state_seed_table_json_from_artifacts(
    seeds: &[Arc<RsjsSharedStateSeedArtifact>],
) -> Option<String> {
    if seeds.is_empty() {
        return None;
    }

    let mut json = String::from("{\"abi_version\":1,\"stores\":[");
    for (index, seed) in seeds.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str("{\"scope_id\":");
        json.push_str(&seed.scope_id.to_string());
        json.push_str(",\"type_key\":");
        append_inline_script_safe_json(
            &mut json,
            &serde_json::to_string(seed.type_key)
                .expect("serializing a static shared-state type key cannot fail"),
        );
        json.push_str(",\"schema_hash\":");
        append_inline_script_safe_json(
            &mut json,
            &serde_json::to_string(seed.schema_hash)
                .expect("serializing a static shared-state schema hash cannot fail"),
        );
        json.push_str(",\"rust_type\":");
        append_inline_script_safe_json(
            &mut json,
            &serde_json::to_string(seed.rust_type)
                .expect("serializing a static shared-state Rust type cannot fail"),
        );
        json.push_str(",\"state\":");
        append_inline_script_safe_json(&mut json, &seed.state_json);
        if seed.wide_paths_json.as_ref() != "[]" {
            json.push_str(",\"wide_paths\":");
            append_inline_script_safe_json(&mut json, seed.wide_paths_json.as_ref());
        }
        json.push('}');
    }
    json.push_str("]}");
    Some(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::RouteParam;
    use crate::{OutputMeter, WriteError};

    #[derive(Debug)]
    struct TestUser {
        id: u64,
    }

    #[test]
    fn request_context_exposes_headers_cookies_and_params() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::COOKIE,
            HeaderValue::from_static("theme=dark; session=abc123"),
        );
        headers.insert(
            http::header::USER_AGENT,
            HeaderValue::from_static("proa-test"),
        );
        let request = RequestContext::new()
            .with_method(Method::GET)
            .with_location(RequestLocation::from(
                "/products/42?tab=reviews".parse::<http::Uri>().unwrap(),
            ))
            .with_route_params(RouteParams::from_vec(vec![RouteParam::new("id", "42")]))
            .with_headers(headers)
            .with_client_ip("203.0.113.5");

        assert_eq!(request.method(), Some(&Method::GET));
        assert_eq!(request.location().unwrap().path(), "/products/42");
        assert_eq!(request.param("id"), Some("42"));
        assert_eq!(request.cookie("theme"), Some("dark"));
        assert_eq!(request.cookie("session"), Some("abc123"));
        assert_eq!(request.user_agent(), Some("proa-test"));
        assert_eq!(request.client_ip(), Some("203.0.113.5"));
    }

    #[test]
    fn locals_store_typed_request_values() {
        let mut locals = Locals::new();
        locals.insert(TestUser { id: 7 });

        let user = locals.get::<TestUser>().unwrap();
        assert_eq!(user.id, 7);
        assert!(locals.contains::<TestUser>());
        assert!(locals.remove::<TestUser>().is_some());
        assert!(!locals.contains::<TestUser>());
    }

    #[test]
    fn response_context_refuses_late_header_mutations_after_streaming_started() {
        let mut response = ResponseContext::new();

        assert!(response.set_status(StatusCode::CREATED));
        assert!(response.insert_header(
            HeaderName::from_static("x-before"),
            HeaderValue::from_static("1")
        ));

        response.mark_streaming_started();

        assert!(!response.can_mutate_headers());
        assert!(!response.set_status(StatusCode::ACCEPTED));
        assert!(!response.insert_header(
            HeaderName::from_static("x-after"),
            HeaderValue::from_static("1")
        ));
        assert!(!response.append_set_cookie(HeaderValue::from_static("late=1")));
        assert_eq!(response.status(), Some(StatusCode::CREATED));
        assert!(response.headers().get("x-after").is_none());
        assert!(response.headers().get(http::header::SET_COOKIE).is_none());
    }

    #[test]
    fn web_context_parts_exposes_composable_side_channels() {
        let mut locals = Locals::new();
        locals.insert(TestUser { id: 23 });
        let mut response = ResponseContext::new();
        assert!(response.set_status(StatusCode::CREATED));
        let mut head = HeadContext::new();
        head.set_title("Parts");
        let mut cache_trace = CacheTrace::new();
        cache_trace.depend_path("/products/42");
        let mut telemetry = RenderTelemetry::new();
        telemetry.set_request_id("req-1");
        let request = RequestContext::new().with_method(Method::POST);

        let parts = WebContextParts::new()
            .with_request(request)
            .with_response(response)
            .with_head(head)
            .with_cache_trace(cache_trace)
            .with_telemetry(telemetry)
            .with_locals(locals);

        assert_eq!(parts.request().method(), Some(&Method::POST));
        assert_eq!(parts.response().status(), Some(StatusCode::CREATED));
        assert_eq!(parts.head_state().title().as_deref(), Some("Parts"));
        assert_eq!(
            parts.cache_trace().dependencies(),
            vec![CacheDependency::Path("/products/42".to_owned())]
        );
        assert_eq!(parts.telemetry().request_id().as_deref(), Some("req-1"));
        assert_eq!(parts.locals().get::<TestUser>().unwrap().id, 23);
    }

    #[test]
    fn context_parts_fork_ordinary_side_channels_by_value() {
        let mut parent_out = Vec::new();
        let mut parent = WebContext::with_buffer(&mut parent_out);
        parent.locals_mut().insert(TestUser { id: 11 });
        parent.head_mut().set_title("Parent");
        parent.cache_trace_mut().depend_tag("product:42");
        assert!(parent.response_mut().set_status(StatusCode::ACCEPTED));

        let parts = parent.context_parts();
        let mut child_out = Vec::new();
        let mut child = WebContext::with_buffer(&mut child_out).with_context_parts(parts);

        assert_eq!(child.locals().get::<TestUser>().unwrap().id, 11);
        child
            .head_mut()
            .push_meta("description", "Child description");
        child
            .cache_trace_mut()
            .mark_dynamic("child used request headers");
        child.response_mut().append_header(
            HeaderName::from_static("x-child"),
            HeaderValue::from_static("1"),
        );

        assert_eq!(parent.head_state().title().as_deref(), Some("Parent"));
        assert!(parent.head_state().entries().is_empty());
        assert_eq!(parent.cache_trace().dependencies().len(), 1);
        assert!(parent.cache_trace().dynamic_reasons().is_empty());
        assert_eq!(parent.response().status(), Some(StatusCode::ACCEPTED));
        assert_eq!(
            parent
                .response()
                .headers()
                .get("x-child")
                .and_then(|value| value.to_str().ok()),
            None
        );
    }

    #[test]
    fn context_parts_preserve_csp_nonce_across_deferred_forks() {
        let nonce = CspNonce::new("cmVxdWVzdC1ub25jZQ==").unwrap();
        let mut root_out = Vec::new();
        let mut root = WebContext::with_buffer(&mut root_out);
        root.set_csp_nonce(nonce.clone());

        let parts = root.context_parts();
        assert_eq!(parts.csp_nonce(), Some(&nonce));

        let mut child_out = Vec::new();
        let child = WebContext::with_buffer(&mut child_out).with_context_parts(parts);
        assert_eq!(child.csp_nonce(), Some(&nonce));
    }

    #[test]
    fn scoped_output_reborrow_preserves_lazy_side_channels() {
        let mut parent_out = Vec::new();
        let mut child_out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut parent_out);
        assert!(cx
            .ensure_context_parts()
            .rsjs_shared_state
            .page
            .get()
            .is_none());
        assert!(cx
            .ensure_context_parts()
            .rsjs_shared_state
            .active
            .get()
            .is_none());

        cx.with_output(&mut child_out, |child| {
            assert!(child
                .ensure_context_parts()
                .rsjs_shared_state
                .page
                .get()
                .is_none());
            assert!(child
                .ensure_context_parts()
                .rsjs_shared_state
                .active
                .get()
                .is_none());
            child.out.extend_static(b"child")?;
            Ok::<_, crate::WriteError>(())
        })
        .unwrap();

        // Touching an unrelated channel must keep the rsjs shared-state
        // registry cold: pages without providers pay zero allocations for it.
        cx.head_mut().set_title("still cold");
        let parts = cx.ensure_context_parts();
        assert!(parts.rsjs_shared_state.page.get().is_none());
        assert!(parts.rsjs_shared_state.active.get().is_none());
        assert!(!parts.rsjs_shared_state.published.get());

        drop(cx);
        assert!(parent_out.is_empty());
        assert_eq!(child_out, b"child");
    }

    #[test]
    fn scoped_output_reborrow_restores_mutations_after_an_error() {
        let mut parent_out = Vec::new();
        let mut child_out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut parent_out);
        cx.head_mut().set_title("before");
        cx.rsjs_island_used("before");

        let result = cx.with_output(&mut child_out, |child| {
            child.enter_mode(ContentMode::Style);
            child.__proa_enter_escape_dialect(EscapeDialect::Xml);
            child.set_render_mode(RenderMode::ChunkedOutOfOrder);
            child.head_mut().set_title("after");
            child.rsjs_island_used("after");
            Err::<(), _>(crate::WriteError::OutOfCapacity)
        });

        assert_eq!(result, Err(crate::WriteError::OutOfCapacity));
        assert_eq!(cx.current_mode(), ContentMode::Style);
        assert_eq!(cx.__proa_current_escape_dialect(), EscapeDialect::Xml);
        assert_eq!(cx.render_mode(), RenderMode::ChunkedOutOfOrder);
        assert_eq!(cx.head_state().title().as_deref(), Some("after"));
        let islands = cx.rsjs_islands_used();
        assert!(islands.contains("before"));
        assert!(islands.contains("after"));
    }

    #[test]
    fn scoped_output_reborrow_restores_state_during_unwind() {
        let mut parent_out = Vec::new();
        let mut child_out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut parent_out);

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cx.with_output(&mut child_out, |child| {
                child.enter_mode(ContentMode::Script);
                child.head_mut().set_title("survived");
                panic!("test unwind");
            });
        }));

        assert!(panic.is_err());
        assert_eq!(cx.current_mode(), ContentMode::Script);
        assert_eq!(cx.head_state().title().as_deref(), Some("survived"));
    }

    #[test]
    fn self_metered_output_reborrow_is_compile_safe_and_restores_state() {
        let meter = OutputMeter::new(8);
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        cx.head_mut().set_title("before");

        let value = cx
            .with_metered_output(meter.borrow(), |child| {
                child.enter_mode(ContentMode::Style);
                child.__proa_enter_escape_dialect(EscapeDialect::Xml);
                child.set_render_mode(RenderMode::Agent);
                child.head_mut().set_title("after");
                child.rsjs_island_used("metered");
                child.out.extend_static(b"metered")?;
                Ok::<_, WriteError>(17)
            })
            .unwrap();

        assert_eq!(value, 17);
        assert_eq!(meter.consumed(), 7);
        assert_eq!(cx.current_mode(), ContentMode::Style);
        assert_eq!(cx.__proa_current_escape_dialect(), EscapeDialect::Xml);
        assert_eq!(cx.render_mode(), RenderMode::Agent);
        assert_eq!(cx.head_state().title().as_deref(), Some("after"));
        assert!(cx.rsjs_islands_used().contains("metered"));
        drop(cx);
        assert_eq!(out, b"metered");
    }

    #[test]
    fn self_metered_output_preserves_structured_limit_failure_and_unwind_state() {
        let meter = OutputMeter::new(2);
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        let error = cx
            .with_metered_output(meter.borrow(), |child| {
                child.enter_mode(ContentMode::Script);
                child.out.extend_static(b"three")
            })
            .unwrap_err();
        assert_eq!(
            error,
            WriteError::OutputLimitExceeded {
                requested: 5,
                remaining: 2,
            }
        );
        assert_eq!(cx.current_mode(), ContentMode::Script);
        assert!(cx.out.is_empty());

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cx.with_metered_output(meter.borrow(), |child| {
                child.__proa_enter_escape_dialect(EscapeDialect::Xml);
                child.head_mut().set_title("survived");
                panic!("test metered unwind");
            });
        }));
        assert!(panic.is_err());
        assert_eq!(cx.__proa_current_escape_dialect(), EscapeDialect::Xml);
        assert_eq!(cx.head_state().title().as_deref(), Some("survived"));
    }

    #[tokio::test]
    async fn async_self_metered_output_keeps_child_borrow_across_await() {
        let meter = OutputMeter::new(8);
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        let value = cx
            .with_metered_output_async(meter.borrow(), async |child| {
                child.enter_mode(ContentMode::Style);
                child.head_mut().set_title("before await");
                tokio::task::yield_now().await;
                child.__proa_enter_escape_dialect(EscapeDialect::Xml);
                child.rsjs_island_used("async-metered");
                child.out.extend_static(b"async")?;
                Ok::<_, WriteError>(29)
            })
            .await
            .unwrap();

        assert_eq!(value, 29);
        assert_eq!(meter.consumed(), 5);
        assert_eq!(cx.current_mode(), ContentMode::Style);
        assert_eq!(cx.__proa_current_escape_dialect(), EscapeDialect::Xml);
        assert_eq!(cx.head_state().title().as_deref(), Some("before await"));
        assert!(cx.rsjs_islands_used().contains("async-metered"));
        drop(cx);
        assert_eq!(out, b"async");
    }

    #[tokio::test]
    async fn cancelling_async_metered_scope_restores_child_state() {
        let meter = OutputMeter::new(8);
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        let result = tokio::time::timeout(
            Duration::from_millis(1),
            cx.with_metered_output_async(meter.borrow(), async |child| {
                child.enter_mode(ContentMode::Script);
                child.set_render_mode(RenderMode::Agent);
                child.head_mut().set_title("cancelled");
                std::future::pending::<()>().await;
            }),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(cx.current_mode(), ContentMode::Script);
        assert_eq!(cx.render_mode(), RenderMode::Agent);
        assert_eq!(cx.head_state().title().as_deref(), Some("cancelled"));
    }

    #[test]
    fn reserved_output_bypasses_outer_meter_and_validates_exact_completion() {
        let meter = OutputMeter::new(16);
        let metered = MeteredWriteBuf::new(Vec::new(), meter.borrow());
        let mut cx = WebContext::with_buffer(metered);
        let reservation = cx.out.reserve_exact(5).unwrap();

        let value = cx
            .with_reserved_output(reservation, |child| {
                child.enter_mode(ContentMode::Style);
                child.head_mut().set_title("reserved");
                child.out.extend_static(b"hello")?;
                Ok(23)
            })
            .unwrap();

        assert_eq!(value, 23);
        assert_eq!(
            meter.consumed(),
            5,
            "reserved bytes must not be double-debited"
        );
        assert_eq!(cx.current_mode(), ContentMode::Style);
        assert_eq!(cx.head_state().title().as_deref(), Some("reserved"));
        assert_eq!(cx.output().as_slice(), b"hello");
    }

    #[test]
    fn reserved_output_reports_target_contract_errors_precisely() {
        let meter = OutputMeter::new(16);
        let metered = MeteredWriteBuf::new(Vec::new(), meter.borrow());
        let mut cx = WebContext::with_buffer(metered);

        let incomplete = cx.out.reserve_exact(4).unwrap();
        let error = cx
            .with_reserved_output(incomplete, |child| {
                child.out.extend_static(b"ok")?;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(
            error,
            WriteError::OutputReservationIncomplete {
                reserved: 4,
                consumed: 2,
                remaining: 2,
            }
        );

        let exceeded = cx.out.reserve_exact(1).unwrap();
        let error = cx
            .with_reserved_output(exceeded, |child| {
                child.out.extend_static(b"no")?;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(
            error,
            WriteError::OutputReservationExceeded {
                requested: 2,
                reserved: 1,
                consumed: 0,
                remaining: 1,
            }
        );
        assert_eq!(cx.output().as_slice(), b"ok");
    }

    #[test]
    fn reserved_output_preserves_underlying_error_and_restores_during_unwind() {
        let meter = OutputMeter::new(8);
        let mut backing = [0u8; 0];
        let cursor = crate::stack::WriteCursor::new(&mut backing);
        let metered = MeteredWriteBuf::new(cursor, meter.borrow());
        let mut cx = WebContext::with_buffer(metered);
        let reservation = cx.out.reserve_exact(2).unwrap();

        let error = cx
            .with_reserved_output(reservation, |child| {
                child.enter_mode(ContentMode::Style);
                child.out.push(b'x')?;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error, WriteError::OutOfCapacity);
        assert_eq!(cx.current_mode(), ContentMode::Style);

        let reservation = cx.out.reserve_exact(2).unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = cx.with_reserved_output(reservation, |child| -> Result<(), WriteError> {
                child.__proa_enter_escape_dialect(EscapeDialect::Xml);
                child.set_render_mode(RenderMode::Agent);
                panic!("test reserved unwind");
            });
        }));
        assert!(panic.is_err());
        assert_eq!(cx.__proa_current_escape_dialect(), EscapeDialect::Xml);
        assert_eq!(cx.render_mode(), RenderMode::Agent);
    }

    #[test]
    fn query_seed_table_keeps_clean_fragments_verbatim() {
        let mut out: Vec<u8> = Vec::new();
        let cx = WebContext::with_buffer(&mut out);
        cx.rsjs_query_seed_json(r#"["stock","AAPL"]"#, r#"{"data":{"value":172.5}}"#);

        let json = cx.rsjs_query_seed_table_json().unwrap();
        assert_eq!(
            json,
            r#"{"queries":[{"key":["stock","AAPL"],"state":{"data":{"value":172.5}},"rank":"{\"data\":{\"value\":172.5}}"}]}"#
        );
    }

    #[test]
    fn artifact_snapshot_excludes_open_transaction_until_commit() {
        let mut out = Vec::new();
        let cx = WebContext::with_buffer(&mut out);
        let checkpoint = cx.rsjs_artifact_checkpoint();
        cx.rsjs_island_used("open-island");
        cx.rsjs_query_seed_json(r#"["open"]"#, r#"{"value":1}"#);

        assert!(cx.rsjs_artifacts_snapshot().is_empty());
        let pending = cx.rsjs_artifacts_since(&checkpoint);
        assert_eq!(
            pending.rendered_islands().collect::<Vec<_>>(),
            ["open-island"]
        );
        assert_eq!(pending.query_seeds().len(), 1);

        cx.rsjs_artifact_commit(checkpoint);
        let committed = cx.rsjs_artifacts_snapshot();
        assert_eq!(
            committed.rendered_islands().collect::<Vec<_>>(),
            ["open-island"]
        );
        assert_eq!(committed.query_seeds().len(), 1);
    }

    #[test]
    fn react_artifacts_share_transaction_commit_and_rollback() {
        let mut out = Vec::new();
        let cx = WebContext::with_buffer(&mut out);

        cx.react_island_used("Shell");
        let committed = cx.rsjs_artifact_checkpoint();
        cx.react_island_used("CommittedBoundary");
        cx.rsjs_artifact_commit(committed);

        let rolled_back = cx.rsjs_artifact_checkpoint();
        cx.react_island_used("DiscardedBoundary");
        cx.rsjs_artifact_rollback(rolled_back);

        assert_eq!(
            cx.render_artifacts_snapshot().react_islands,
            ["CommittedBoundary".to_owned(), "Shell".to_owned()]
        );
    }

    #[test]
    fn detached_contexts_share_ids_only_with_their_own_request() {
        let mut first_out = Vec::new();
        let mut first = WebContext::with_buffer(&mut first_out);
        let first_parts = first.context_parts();
        let mut first_child_out = Vec::new();
        let first_child =
            WebContext::with_buffer(&mut first_child_out).with_context_parts(first_parts);

        let mut second_out = Vec::new();
        let second = WebContext::with_buffer(&mut second_out);

        assert_eq!(first.next_react_island_id(), 0);
        assert_eq!(second.next_react_island_id(), 0);
        assert_eq!(first_child.next_react_island_id(), 1);
        assert_eq!(second.next_react_island_id(), 1);
    }

    #[test]
    fn artifact_rollback_discards_only_the_failed_owner() {
        let mut root_out = Vec::new();
        let mut root = WebContext::with_buffer(&mut root_out);
        root.rsjs_island_used("shared");
        root.rsjs_query_seed_json(r#"["survivor"]"#, r#"{"value":1}"#);

        let sibling_parts = root.context_parts();
        let mut sibling_out = Vec::new();
        let sibling = WebContext::with_buffer(&mut sibling_out).with_context_parts(sibling_parts);

        let checkpoint = root.rsjs_artifact_checkpoint();
        root.rsjs_island_used("shared");
        root.rsjs_island_used("doomed");
        root.rsjs_query_seed_json(r#"["doomed"]"#, r#"{"value":2}"#);
        sibling.rsjs_island_used("sibling");
        sibling.rsjs_query_seed_json(r#"["sibling"]"#, r#"{"value":3}"#);
        root.rsjs_artifact_rollback(checkpoint);

        let snapshot = root.rsjs_artifacts_snapshot();
        assert_eq!(
            snapshot.rendered_islands().collect::<Vec<_>>(),
            ["shared", "sibling"]
        );
        let seed_keys: Vec<_> = snapshot
            .query_seeds()
            .iter()
            .map(RsjsQuerySeedArtifact::key_json)
            .collect();
        assert_eq!(seed_keys, [r#"["sibling"]"#, r#"["survivor"]"#]);
    }

    #[test]
    fn nested_artifact_commit_stays_owned_by_outer_transaction() {
        let mut out = Vec::new();
        let cx = WebContext::with_buffer(&mut out);
        let outer = cx.rsjs_artifact_checkpoint();
        let inner = cx.rsjs_artifact_checkpoint();
        cx.rsjs_island_used("nested");
        cx.rsjs_query_seed_json(r#"["nested"]"#, r#"{"value":1}"#);

        cx.rsjs_artifact_commit(inner);
        assert!(cx.rsjs_artifacts_snapshot().is_empty());
        cx.rsjs_artifact_rollback(outer);
        assert!(cx.rsjs_artifacts_snapshot().is_empty());
    }

    #[test]
    fn detached_fork_inherits_transaction_outcome_for_late_writes() {
        let mut root_out = Vec::new();
        let mut root = WebContext::with_buffer(&mut root_out);

        let doomed = root.rsjs_artifact_checkpoint();
        let doomed_parts = root.context_parts();
        let mut doomed_out = Vec::new();
        let doomed_fork = WebContext::with_buffer(&mut doomed_out).with_context_parts(doomed_parts);
        root.rsjs_artifact_rollback(doomed);
        doomed_fork.rsjs_island_used("late-doomed");
        doomed_fork.rsjs_query_seed_json(r#"["late-doomed"]"#, "{}");
        let nested_doomed = doomed_fork.rsjs_artifact_checkpoint();
        doomed_fork.rsjs_island_used("nested-late-doomed");
        assert!(
            doomed_fork.rsjs_artifacts_since(&nested_doomed).is_empty(),
            "an open child beneath a rolled-back inherited owner is already doomed"
        );
        doomed_fork.rsjs_artifact_commit(nested_doomed);
        assert!(root.rsjs_artifacts_snapshot().is_empty());

        let committed = root.rsjs_artifact_checkpoint();
        let committed_parts = root.context_parts();
        let mut committed_out = Vec::new();
        let committed_fork =
            WebContext::with_buffer(&mut committed_out).with_context_parts(committed_parts);
        root.rsjs_artifact_commit(committed);
        committed_fork.rsjs_island_used("late-committed");
        committed_fork.rsjs_query_seed_json(r#"["late-committed"]"#, "{}");
        let nested_committed = committed_fork.rsjs_artifact_checkpoint();
        committed_fork.rsjs_island_used("nested-late-committed");
        committed_fork.rsjs_artifact_commit(nested_committed);

        let snapshot = root.rsjs_artifacts_snapshot();
        assert_eq!(
            snapshot.rendered_islands().collect::<Vec<_>>(),
            ["late-committed", "nested-late-committed"]
        );
        assert_eq!(
            snapshot.query_seeds()[0].key_json(),
            r#"["late-committed"]"#
        );
    }

    #[test]
    fn direct_detached_constructor_inherits_active_transactions() {
        let mut root_out = Vec::new();
        let mut root = WebContext::with_buffer(&mut root_out);
        let artifact_checkpoint = root.rsjs_artifact_checkpoint();
        let artifact_transaction = root
            .current_rsjs_artifact_transaction()
            .expect("artifact checkpoint should install a transaction");
        let shared_state_checkpoint = root.rsjs_shared_state_seed_checkpoint();
        let parts = root.context_parts();

        let mut child_out = Vec::new();
        let child = WebContext::with_loader_buffer_and_context_parts(
            &mut child_out,
            Arc::new(NoLoader),
            parts,
        );

        let child_artifact_transaction = child
            .current_rsjs_artifact_transaction()
            .expect("detached child should inherit the artifact transaction");
        assert!(Rc::ptr_eq(
            &child_artifact_transaction,
            &artifact_transaction
        ));
        assert_eq!(
            child.rsjs_shared_state_transaction,
            Some(shared_state_checkpoint.transaction_id)
        );

        drop(child);
        root.rsjs_shared_state_rollback_seeds(shared_state_checkpoint);
        root.rsjs_artifact_rollback(artifact_checkpoint);
    }

    #[test]
    fn dropping_artifact_branch_rolls_back_speculative_contributions() {
        let mut out = Vec::new();
        let cx = WebContext::with_buffer(&mut out);
        {
            let branch = cx.rsjs_artifact_branch();
            cx.rsjs_island_used("cancelled");
            cx.rsjs_query_seed_json(r#"["cancelled"]"#, "{}");
            assert_eq!(
                branch.snapshot().rendered_islands().collect::<Vec<_>>(),
                ["cancelled"]
            );
        }

        assert!(cx.rsjs_artifacts_snapshot().is_empty());
        cx.rsjs_island_used("survivor");
        assert_eq!(
            cx.rsjs_artifacts_snapshot()
                .rendered_islands()
                .collect::<Vec<_>>(),
            ["survivor"]
        );
    }

    #[test]
    fn artifact_snapshot_delta_and_replay_are_deterministic() {
        let mut source_out = Vec::new();
        let source = WebContext::with_buffer(&mut source_out);
        source.rsjs_island_used("zeta");
        source.rsjs_island_used("alpha");
        source.rsjs_query_seed_json(r#"["zeta"]"#, r#"{"value":2}"#);
        source.rsjs_query_seed_json(r#"["alpha"]"#, r#"{"value":1}"#);
        let full = source.rsjs_artifacts_snapshot();
        assert_eq!(
            full.rendered_islands().collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert_eq!(full.query_seeds()[0].key_json(), r#"["alpha"]"#);

        let mut target_out = Vec::new();
        let target = WebContext::with_buffer(&mut target_out);
        target.rsjs_island_used("alpha");
        target.rsjs_query_seed_json(r#"["alpha"]"#, r#"{"value":1}"#);
        let baseline = target.rsjs_artifacts_snapshot();
        target.replay_rsjs_artifacts(&full).unwrap();
        target.replay_rsjs_artifacts(&full).unwrap();

        assert_eq!(target.rsjs_artifacts_snapshot(), full);
        let delta = target.rsjs_artifacts_delta_since(&baseline);
        assert_eq!(delta.rendered_islands().collect::<Vec<_>>(), ["zeta"]);
        assert_eq!(delta.query_seeds()[0].key_json(), r#"["zeta"]"#);
    }

    #[test]
    fn query_seed_conflicts_follow_documented_deterministic_policy() {
        let mut same_owner_out = Vec::new();
        let same_owner = WebContext::with_buffer(&mut same_owner_out);
        same_owner.rsjs_query_seed_json(r#"["key"]"#, r#"{"value":"z"}"#);
        same_owner.rsjs_query_seed_json(r#"["key"]"#, r#"{"value":"a"}"#);
        assert_eq!(
            same_owner.rsjs_artifacts_snapshot().query_seeds()[0].state_json(),
            r#"{"value":"z"}"#,
            "one owner preserves historical first-key-wins behavior"
        );

        let mut independent_out = Vec::new();
        let independent = WebContext::with_buffer(&mut independent_out);
        let first = independent.rsjs_artifact_checkpoint();
        independent.rsjs_query_seed_json(r#"["key"]"#, r#"{"value":"z"}"#);
        independent.rsjs_artifact_commit(first);
        let completion_order_baseline = independent.rsjs_artifacts_snapshot();
        let second = independent.rsjs_artifact_checkpoint();
        independent.rsjs_query_seed_json(r#"["key"]"#, r#"{"value":"a"}"#);
        independent.rsjs_artifact_commit(second);
        let lexical_winner = independent.rsjs_artifacts_snapshot();
        assert_eq!(
            lexical_winner.query_seeds()[0].state_json(),
            r#"{"value":"a"}"#,
            "independent owners choose the lexical state, not completion order"
        );
        let correction = lexical_winner.delta_since(&completion_order_baseline);
        assert_eq!(
            correction.query_seeds()[0].state_json(),
            r#"{"value":"a"}"#,
            "a later-completing lexical winner must remain visible as a replacement delta"
        );
    }

    #[test]
    fn artifact_snapshot_does_not_materialize_artifact_page() {
        let mut out = Vec::new();
        let cx = WebContext::with_buffer(&mut out);
        assert!(cx.rsjs_artifacts_snapshot().is_empty());
        assert!(cx
            .ensure_context_parts()
            .rsjs_artifacts
            .page
            .get()
            .is_none());
    }

    #[test]
    fn query_seed_table_cannot_emit_a_literal_script_close() {
        let mut out: Vec<u8> = Vec::new();
        let cx = WebContext::with_buffer(&mut out);
        cx.rsjs_query_seed_json(
            r#""</script><script>alert(1)</script>""#,
            r#""<!--</ScRiPt>-->""#,
        );

        let json = cx.rsjs_query_seed_table_json().unwrap();
        assert!(
            !json.to_ascii_lowercase().contains("</script"),
            "assembled seed table leaked a script close: {json}"
        );
        assert!(!json.contains('<') && !json.contains('>'));

        // The rewrite must stay JSON-equivalent: decoding gives back the
        // original string values.
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["queries"][0]["key"],
            serde_json::Value::String("</script><script>alert(1)</script>".to_owned())
        );
        assert_eq!(
            parsed["queries"][0]["state"],
            serde_json::Value::String("<!--</ScRiPt>-->".to_owned())
        );
    }

    #[test]
    fn query_seed_table_escapes_js_line_separators() {
        let mut out: Vec<u8> = Vec::new();
        let cx = WebContext::with_buffer(&mut out);
        cx.rsjs_query_seed_json("\"a\u{2028}b\"", "\"c\u{2029}d\"");

        let json = cx.rsjs_query_seed_table_json().unwrap();
        assert!(!json.contains('\u{2028}') && !json.contains('\u{2029}'));
        assert!(json.contains("\\u2028") && json.contains("\\u2029"));

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["queries"][0]["key"],
            serde_json::Value::String("a\u{2028}b".to_owned())
        );
        assert_eq!(
            parsed["queries"][0]["state"],
            serde_json::Value::String("c\u{2029}d".to_owned())
        );
    }

    #[test]
    fn inline_script_safe_append_passes_other_multibyte_chars_through() {
        // Shares the 0xE2 lead byte with U+2028/U+2029 but must not be
        // altered by the slow path.
        let mut json = String::new();
        append_inline_script_safe_json(&mut json, "\"em\u{2014}dash \u{201c}quoted\u{201d}\"");
        assert_eq!(json, "\"em\u{2014}dash \u{201c}quoted\u{201d}\"");
    }

    #[test]
    fn shared_state_seed_table_cannot_emit_a_literal_script_close() {
        let mut out: Vec<u8> = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let _scope = cx
            .rsjs_shared_state_enter(
                "acme.</script><script>alert(1)</script>",
                "sha256:</script>",
                "Evil</ScRiPt>State",
                &[],
                Rc::new(1u8),
                "\"<!--</ScRiPt>-->\u{2028}\"".to_owned(),
            )
            .unwrap();
        cx.rsjs_shared_state_acquire::<u8>(
            "acme.</script><script>alert(1)</script>",
            "sha256:</script>",
            "Evil</ScRiPt>State",
            "EscapingProbe",
        )
        .unwrap();

        let json = cx.rsjs_shared_state_seed_table_json().unwrap();
        assert!(
            !json.to_ascii_lowercase().contains("</script"),
            "assembled shared-state seed table leaked a script close: {json}"
        );
        assert!(!json.contains('<') && !json.contains('>'));
        assert!(!json.contains('\u{2028}') && !json.contains('\u{2029}'));

        // The rewrite must stay JSON-equivalent: decoding gives back the
        // original metadata and state values.
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let store = &parsed["stores"][0];
        assert_eq!(
            store["type_key"],
            serde_json::Value::String("acme.</script><script>alert(1)</script>".to_owned())
        );
        assert_eq!(
            store["schema_hash"],
            serde_json::Value::String("sha256:</script>".to_owned())
        );
        assert_eq!(
            store["rust_type"],
            serde_json::Value::String("Evil</ScRiPt>State".to_owned())
        );
        assert_eq!(
            store["state"],
            serde_json::Value::String("<!--</ScRiPt>-->\u{2028}".to_owned())
        );
    }

    // Stores with wide-int fields must ship recursive paths so the browser
    // can decode nested values and Vec elements to BigInt.
    #[test]
    fn shared_state_seed_table_carries_wide_field_metadata() {
        let mut out: Vec<u8> = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let _wide_scope = cx
            .rsjs_shared_state_enter(
                "test.cart",
                "sha256:cart",
                "CartState",
                &["count", "total_cents"],
                Rc::new(1u8),
                "{\"count\":\"8\",\"total_cents\":\"12050\",\"label\":\"summer\"}".to_owned(),
            )
            .unwrap();
        cx.rsjs_shared_state_acquire::<u8>("test.cart", "sha256:cart", "CartState", "WideProbe")
            .unwrap();
        let _narrow_scope = cx
            .rsjs_shared_state_enter(
                "test.theme",
                "sha256:theme",
                "ThemeState",
                &[],
                Rc::new(2u8),
                "{\"button_bg\":\"green\"}".to_owned(),
            )
            .unwrap();
        cx.rsjs_shared_state_acquire::<u8>(
            "test.theme",
            "sha256:theme",
            "ThemeState",
            "NarrowProbe",
        )
        .unwrap();

        let json = cx.rsjs_shared_state_seed_table_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let stores = parsed["stores"].as_array().unwrap();
        assert_eq!(
            stores[0]["wide_paths"],
            serde_json::json!([["count"], ["total_cents"]]),
            "wide-int paths must reach the seed table: {json}"
        );
        assert!(
            stores[1].get("wide_paths").is_none(),
            "stores without wide-int fields must omit wide_paths: {json}"
        );
    }

    #[test]
    fn unused_shared_state_provider_does_not_publish_its_value() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let _scope = cx
            .rsjs_shared_state_enter(
                "test.unused",
                "sha256:unused",
                "UnusedState",
                &[],
                Rc::new(1u8),
                "{\"private_until_acquired\":true}".to_owned(),
            )
            .unwrap();

        assert!(
            cx.rsjs_shared_state_seed_table_json().is_none(),
            "provider registration is topology; without a committed consumer its value stays private"
        );
        let error = cx
            .rsjs_shared_state_enter(
                "test.too-late",
                "sha256:too-late",
                "TooLateState",
                &[],
                Rc::new(2u8),
                "{}".to_owned(),
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("no committed shell consumer made a provider seed live"),
            "{error}"
        );
    }

    #[test]
    fn shared_state_liveness_selects_the_exact_nested_provider_scope() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let outer = cx
            .rsjs_shared_state_enter(
                "test.nested",
                "sha256:nested",
                "NestedState",
                &[],
                Rc::new(1u8),
                "{\"value\":\"outer\"}".to_owned(),
            )
            .unwrap();
        let inner = cx
            .rsjs_shared_state_enter(
                "test.nested",
                "sha256:nested",
                "NestedState",
                &[],
                Rc::new(2u8),
                "{\"value\":\"inner\"}".to_owned(),
            )
            .unwrap();

        let (_, scope_id) = cx
            .rsjs_shared_state_acquire::<u8>(
                "test.nested",
                "sha256:nested",
                "NestedState",
                "NestedConsumer",
            )
            .unwrap();
        assert_eq!(scope_id, 1);
        let table = cx.rsjs_shared_state_seed_table_json().unwrap();
        assert!(table.contains("\"scope_id\":1"), "{table}");
        assert!(table.contains("\"value\":\"inner\""), "{table}");
        assert!(!table.contains("\"scope_id\":0"), "{table}");
        assert!(!table.contains("\"value\":\"outer\""), "{table}");

        drop(inner);
        drop(outer);
    }

    #[test]
    fn linked_shared_state_acquire_selects_exact_inner_and_outer_occurrences() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let outer_key = RsjsLinkedProviderOccurrenceKey::new([0x11; 32], 4, 7);
        let inner_key = RsjsLinkedProviderOccurrenceKey::new([0x11; 32], 4, 8);
        assert_eq!(outer_key.scope_id(), [0x11; 32]);
        assert_eq!(outer_key.host_candidate_id(), 4);
        assert_eq!(outer_key.provider_site_id(), 7);

        let _outer_scope = cx
            .rsjs_shared_state_enter(
                "test.linked-nested",
                "sha256:linked-nested",
                "LinkedNestedState",
                &[],
                Rc::new(10u8),
                "{\"value\":\"outer\"}".to_owned(),
            )
            .unwrap();
        let _outer_tag = cx.rsjs_shared_state_tag_linked_provider(outer_key).unwrap();
        let _inner_scope = cx
            .rsjs_shared_state_enter(
                "test.linked-nested",
                "sha256:linked-nested",
                "LinkedNestedState",
                &[],
                Rc::new(20u8),
                "{\"value\":\"inner\"}".to_owned(),
            )
            .unwrap();
        let _inner_tag = cx.rsjs_shared_state_tag_linked_provider(inner_key).unwrap();

        let (inner, inner_scope) = cx
            .rsjs_shared_state_acquire_linked::<u8>(
                inner_key,
                "test.linked-nested",
                "sha256:linked-nested",
                "LinkedNestedState",
                "InnerLinkedConsumer",
            )
            .unwrap();
        let (outer, outer_scope) = cx
            .rsjs_shared_state_acquire_linked::<u8>(
                outer_key,
                "test.linked-nested",
                "sha256:linked-nested",
                "LinkedNestedState",
                "OuterLinkedConsumer",
            )
            .unwrap();
        let (nearest, nearest_scope) = cx
            .rsjs_shared_state_acquire::<u8>(
                "test.linked-nested",
                "sha256:linked-nested",
                "LinkedNestedState",
                "NearestConsumer",
            )
            .unwrap();

        assert_eq!((*inner, inner_scope), (20, 1));
        assert_eq!((*outer, outer_scope), (10, 0));
        assert_eq!((*nearest, nearest_scope), (20, 1));
        let table = cx.rsjs_shared_state_seed_table_json().unwrap();
        assert!(table.contains("\"scope_id\":0"), "{table}");
        assert!(table.contains("\"scope_id\":1"), "{table}");
    }

    #[test]
    fn missing_linked_provider_never_falls_back_and_tag_guard_cleans_up() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let actual = RsjsLinkedProviderOccurrenceKey::new([0x22; 32], 5, 1);
        let absent = RsjsLinkedProviderOccurrenceKey::new([0x22; 32], 5, 2);
        let _scope = cx
            .rsjs_shared_state_enter(
                "test.linked-missing",
                "sha256:linked-missing",
                "LinkedMissingState",
                &[],
                Rc::new(30u8),
                "{\"value\":30}".to_owned(),
            )
            .unwrap();
        let tag = cx.rsjs_shared_state_tag_linked_provider(actual).unwrap();

        let error = cx
            .rsjs_shared_state_acquire_linked::<u8>(
                absent,
                "test.linked-missing",
                "sha256:linked-missing",
                "LinkedMissingState",
                "MissingLinkedConsumer",
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("rsjs/shared-state-linked-provider-missing"),
            "{error}"
        );
        assert_eq!(
            *cx.rsjs_shared_state_acquire::<u8>(
                "test.linked-missing",
                "sha256:linked-missing",
                "LinkedMissingState",
                "ProvisionalConsumer",
            )
            .unwrap()
            .0,
            30,
            "the old provisional nearest-key path remains available"
        );

        drop(tag);
        let error = cx
            .rsjs_shared_state_acquire_linked::<u8>(
                actual,
                "test.linked-missing",
                "sha256:linked-missing",
                "LinkedMissingState",
                "CleanedUpLinkedConsumer",
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("rsjs/shared-state-linked-provider-missing"),
            "{error}"
        );
    }

    #[test]
    fn linked_shared_state_acquire_rejects_schema_mismatch() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let occurrence = RsjsLinkedProviderOccurrenceKey::new([0x33; 32], 6, 2);
        let _scope = cx
            .rsjs_shared_state_enter(
                "test.linked-schema",
                "sha256:linked-schema",
                "LinkedSchemaState",
                &[],
                Rc::new(40u8),
                "{\"value\":40}".to_owned(),
            )
            .unwrap();
        let _tag = cx
            .rsjs_shared_state_tag_linked_provider(occurrence)
            .unwrap();

        let error = cx
            .rsjs_shared_state_acquire_linked::<u8>(
                occurrence,
                "test.linked-schema",
                "sha256:stale-schema",
                "LinkedSchemaState",
                "StaleLinkedConsumer",
            )
            .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("rsjs/shared-state-schema-mismatch"),
            "{message}"
        );
        assert!(message.contains("exact linked provider"), "{message}");
    }

    #[test]
    fn linked_provider_tag_is_copied_into_context_forks() {
        let mut root_out = Vec::new();
        let mut root = WebContext::with_buffer(&mut root_out);
        let occurrence = RsjsLinkedProviderOccurrenceKey::new([0x44; 32], 7, 3);
        let _scope = root
            .rsjs_shared_state_enter(
                "test.linked-fork",
                "sha256:linked-fork",
                "LinkedForkState",
                &[],
                Rc::new(50u8),
                "{\"value\":50}".to_owned(),
            )
            .unwrap();
        let tag = root
            .rsjs_shared_state_tag_linked_provider(occurrence)
            .unwrap();
        let parts = root.context_parts();
        drop(tag);

        let mut branch_out = Vec::new();
        let branch = WebContext::with_buffer(&mut branch_out).with_context_parts(parts);
        let (value, scope_id) = branch
            .rsjs_shared_state_acquire_linked::<u8>(
                occurrence,
                "test.linked-fork",
                "sha256:linked-fork",
                "LinkedForkState",
                "ForkedLinkedConsumer",
            )
            .unwrap();
        assert_eq!((*value, scope_id), (50, 0));

        assert!(
            root.rsjs_shared_state_acquire_linked::<u8>(
                occurrence,
                "test.linked-fork",
                "sha256:linked-fork",
                "LinkedForkState",
                "RootAfterGuard",
            )
            .is_err(),
            "dropping the root guard must clean only the original active-stack snapshot"
        );
    }

    #[test]
    fn shared_state_acquisition_follows_artifact_commit_and_rollback() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let _scope = cx
            .rsjs_shared_state_enter(
                "test.transactional",
                "sha256:transactional",
                "TransactionalState",
                &[],
                Rc::new(1u8),
                "{\"value\":1}".to_owned(),
            )
            .unwrap();

        let rolled_back = cx.rsjs_artifact_checkpoint();
        cx.rsjs_shared_state_acquire::<u8>(
            "test.transactional",
            "sha256:transactional",
            "TransactionalState",
            "RolledBackConsumer",
        )
        .unwrap();
        assert_eq!(
            cx.rsjs_artifacts_since(&rolled_back)
                .shared_state_seeds()
                .count(),
            1
        );
        cx.rsjs_artifact_rollback(rolled_back);
        assert!(
            cx.rsjs_artifacts_snapshot()
                .shared_state_seeds()
                .next()
                .is_none(),
            "a discarded consumer must not keep its provider seed live"
        );

        let committed = cx.rsjs_artifact_checkpoint();
        cx.rsjs_shared_state_acquire::<u8>(
            "test.transactional",
            "sha256:transactional",
            "TransactionalState",
            "CommittedConsumer",
        )
        .unwrap();
        cx.rsjs_artifact_commit(committed);
        let table = cx.rsjs_shared_state_seed_table_json().unwrap();
        assert!(table.contains("test.transactional"), "{table}");
    }

    #[test]
    fn cached_shared_state_consumer_rebinds_to_current_seed_transactionally() {
        let cached = {
            let mut out = Vec::new();
            let mut source = WebContext::with_buffer(&mut out);
            let _scope = source
                .rsjs_shared_state_enter(
                    "test.cached",
                    "sha256:cached",
                    "CachedState",
                    &[],
                    Rc::new(1u8),
                    "{\"value\":\"cached\"}".to_owned(),
                )
                .unwrap();
            source
                .rsjs_shared_state_acquire::<u8>(
                    "test.cached",
                    "sha256:cached",
                    "CachedState",
                    "CachedConsumer",
                )
                .unwrap();
            source.rsjs_island_used("cached-shared-consumer");
            source.rsjs_query_seed_json(r#"["cached"]"#, r#"{"value":"cached"}"#);
            source.rsjs_artifacts_snapshot()
        };

        let mut out = Vec::new();
        let mut target = WebContext::with_buffer(&mut out);
        let _scope = target
            .rsjs_shared_state_enter(
                "test.cached",
                "sha256:cached",
                "CachedState",
                &[],
                Rc::new(2u8),
                "{\"value\":\"current\"}".to_owned(),
            )
            .unwrap();

        let discarded = target.rsjs_artifact_checkpoint();
        target.replay_rsjs_artifacts(&cached).unwrap();
        let pending = target.rsjs_artifacts_since(&discarded);
        let pending_table = pending.shared_state_seed_table_json().unwrap();
        assert!(pending_table.contains("\"value\":\"current\""));
        assert!(!pending_table.contains("\"value\":\"cached\""));
        target.rsjs_artifact_rollback(discarded);
        assert!(target.rsjs_artifacts_snapshot().is_empty());

        let committed = target.rsjs_artifact_checkpoint();
        target.replay_rsjs_artifacts(&cached).unwrap();
        target.rsjs_artifact_commit(committed);
        let table = target.rsjs_shared_state_seed_table_json().unwrap();
        assert!(table.contains("\"value\":\"current\""), "{table}");
        assert!(!table.contains("\"value\":\"cached\""), "{table}");
    }

    #[test]
    fn cached_shared_state_consumer_rejects_missing_or_mismatched_scope() {
        let cached = {
            let mut out = Vec::new();
            let mut source = WebContext::with_buffer(&mut out);
            let _scope = source
                .rsjs_shared_state_enter(
                    "test.cached",
                    "sha256:cached",
                    "CachedState",
                    &[],
                    Rc::new(1u8),
                    "{}".to_owned(),
                )
                .unwrap();
            source
                .rsjs_shared_state_acquire::<u8>(
                    "test.cached",
                    "sha256:cached",
                    "CachedState",
                    "CachedConsumer",
                )
                .unwrap();
            source.rsjs_island_used("cached-shared-consumer");
            source.rsjs_query_seed_json(r#"["cached"]"#, "{}");
            source.rsjs_artifacts_snapshot()
        };

        let mut missing_out = Vec::new();
        let missing = WebContext::with_buffer(&mut missing_out);
        let error = missing.replay_rsjs_artifacts(&cached).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("rsjs/shared-state-cache-scope-missing"),
            "{error}"
        );
        assert!(
            missing.rsjs_artifacts_snapshot().is_empty(),
            "a failed cache replay must be atomic"
        );

        let mut mismatch_out = Vec::new();
        let mut mismatch = WebContext::with_buffer(&mut mismatch_out);
        let _scope = mismatch
            .rsjs_shared_state_enter(
                "test.other",
                "sha256:other",
                "OtherState",
                &[],
                Rc::new(2u8),
                "{}".to_owned(),
            )
            .unwrap();
        let error = mismatch.replay_rsjs_artifacts(&cached).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("rsjs/shared-state-cache-schema-mismatch"),
            "{error}"
        );
        assert!(
            mismatch.rsjs_artifacts_snapshot().is_empty(),
            "a schema-mismatched cache replay must not leak earlier artifact kinds"
        );
    }

    #[test]
    fn shared_state_channel_allocates_lazily_and_only_on_use() {
        let mut out: Vec<u8> = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        // A provider-less acquire must fail without materializing the
        // registry or the stack.
        let err = cx
            .rsjs_shared_state_acquire::<u8>("test.cold", "sha256:cold", "Cold", "ColdIsland")
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("rsjs/shared-state-missing-provider"));
        let parts = cx.ensure_context_parts();
        assert!(parts.rsjs_shared_state.page.get().is_none());
        assert!(parts.rsjs_shared_state.active.get().is_none());

        // Forking a context with an untouched (or empty) provider stack must
        // not allocate a fresh stack either.
        let fork = cx.context_parts();
        assert!(fork.rsjs_shared_state.active.get().is_none());

        // Entering the first provider is what materializes both cells.
        let _scope = cx
            .rsjs_shared_state_enter(
                "test.cold",
                "sha256:cold",
                "Cold",
                &[],
                Rc::new(1u8),
                "{}".to_owned(),
            )
            .unwrap();
        let parts = cx.ensure_context_parts();
        assert!(parts.rsjs_shared_state.page.get().is_some());
        assert!(parts.rsjs_shared_state.active.get().is_some());
    }

    #[test]
    fn shared_state_forks_share_page_registry_but_not_active_stack() {
        let mut root_out: Vec<u8> = Vec::new();
        let mut root = WebContext::with_buffer(&mut root_out);
        let _root_scope = root
            .rsjs_shared_state_enter(
                "test.fork",
                "sha256:fork",
                "ForkState",
                &[],
                Rc::new(41u32),
                "{\"v\":41}".to_owned(),
            )
            .unwrap();

        // Two concurrent boundary forks of the same request context.
        let parts_a = root.context_parts();
        let parts_b = root.context_parts();
        let mut out_a: Vec<u8> = Vec::new();
        let mut branch_a = WebContext::with_buffer(&mut out_a).with_context_parts(parts_a);
        let mut out_b: Vec<u8> = Vec::new();
        let branch_b = WebContext::with_buffer(&mut out_b).with_context_parts(parts_b);

        // Branch A enters a nested provider for the same protocol key.
        let _nested_scope = branch_a
            .rsjs_shared_state_enter(
                "test.fork",
                "sha256:fork",
                "ForkState",
                &[],
                Rc::new(99u32),
                "{\"v\":99}".to_owned(),
            )
            .unwrap();

        // Branch A resolves its own nested provider.
        let (value_a, scope_a) = branch_a
            .rsjs_shared_state_acquire::<u32>("test.fork", "sha256:fork", "ForkState", "IslandA")
            .unwrap();
        assert_eq!(*value_a, 99);

        // The sibling's nearest-provider resolution is unaffected: the
        // active stack is forked by value, not shared.
        let (value_b, scope_b) = branch_b
            .rsjs_shared_state_acquire::<u32>("test.fork", "sha256:fork", "ForkState", "IslandB")
            .unwrap();
        assert_eq!(*value_b, 41);
        assert_eq!(scope_b, 0);

        // Scope-id allocation is page-level and shared: the nested provider
        // minted the next id from the same counter, not a colliding one.
        assert_eq!(scope_a, 1);

        // The seed registry is page-level and shared: the root table carries
        // both branches' seeds.
        let table = root.rsjs_shared_state_seed_table_json().unwrap();
        assert!(table.contains("\"scope_id\":0"));
        assert!(table.contains("\"scope_id\":1"));
        assert!(table.contains("{\"v\":41}"));
        assert!(table.contains("{\"v\":99}"));
    }

    #[test]
    fn late_provider_diagnostic_is_accurate_when_no_table_was_emitted() {
        let mut out: Vec<u8> = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        // The shell registered no providers: finalization emits nothing.
        assert!(cx.rsjs_shared_state_seed_table_json().is_none());

        let err = cx
            .rsjs_shared_state_enter(
                "test.late",
                "sha256:late",
                "Late",
                &[],
                Rc::new(1u8),
                "{}".to_owned(),
            )
            .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("rsjs/shared-state-late-stream-provider"));
        assert!(message.contains("above the deferred/streaming boundary"));
        assert!(
            message.contains("no seed table script"),
            "diagnostic must say no table was emitted: {message}"
        );
        assert!(
            !message.contains("already emitted"),
            "diagnostic must not claim a table was emitted when none was: {message}"
        );
    }

    #[test]
    fn late_provider_diagnostic_is_accurate_when_a_table_was_emitted() {
        let mut out: Vec<u8> = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        {
            let _scope = cx
                .rsjs_shared_state_enter(
                    "test.late",
                    "sha256:late",
                    "Late",
                    &[],
                    Rc::new(1u8),
                    "{}".to_owned(),
                )
                .unwrap();
            cx.rsjs_shared_state_acquire::<u8>("test.late", "sha256:late", "Late", "ShellConsumer")
                .unwrap();
        }
        assert!(cx.rsjs_shared_state_seed_table_json().is_some());

        let err = cx
            .rsjs_shared_state_enter(
                "test.late",
                "sha256:late",
                "Late",
                &[],
                Rc::new(2u8),
                "{}".to_owned(),
            )
            .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("rsjs/shared-state-late-stream-provider"));
        assert!(message.contains("above the deferred/streaming boundary"));
        assert!(
            message.contains("already emitted with the shell"),
            "diagnostic must acknowledge the emitted table: {message}"
        );
    }

    #[test]
    fn late_provider_is_still_rejected_on_a_fork_made_after_cold_finalization() {
        let mut out: Vec<u8> = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        assert!(cx.rsjs_shared_state_seed_table_json().is_none());

        // A context forked after finalization must inherit the latch even
        // though the page registry was never materialized.
        let parts = cx.context_parts();
        let mut fork_out: Vec<u8> = Vec::new();
        let mut fork = WebContext::with_buffer(&mut fork_out).with_context_parts(parts);
        let err = fork
            .rsjs_shared_state_enter(
                "test.late",
                "sha256:late",
                "Late",
                &[],
                Rc::new(1u8),
                "{}".to_owned(),
            )
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("rsjs/shared-state-late-stream-provider"));
    }

    #[test]
    fn seed_rollback_discards_only_seeds_registered_after_the_checkpoint() {
        let mut out: Vec<u8> = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let _outer = cx
            .rsjs_shared_state_enter(
                "test.outer",
                "sha256:outer",
                "Outer",
                &[],
                Rc::new(1u8),
                "{\"outer\":true}".to_owned(),
            )
            .unwrap();
        cx.rsjs_shared_state_acquire::<u8>("test.outer", "sha256:outer", "Outer", "OuterConsumer")
            .unwrap();

        let checkpoint = cx.rsjs_shared_state_seed_checkpoint();
        {
            let _doomed = cx
                .rsjs_shared_state_enter(
                    "test.doomed",
                    "sha256:doomed",
                    "Doomed",
                    &[],
                    Rc::new(2u8),
                    "{\"doomed\":true}".to_owned(),
                )
                .unwrap();
        }
        cx.rsjs_shared_state_rollback_seeds(checkpoint);

        let table = cx.rsjs_shared_state_seed_table_json().unwrap();
        assert!(table.contains("test.outer"));
        assert!(
            !table.contains("test.doomed"),
            "rolled-back subtree seed must not ship: {table}"
        );
    }

    #[test]
    fn seed_rollback_keeps_a_surviving_sibling_registered_after_checkpoint() {
        let mut root_out: Vec<u8> = Vec::new();
        let mut root = WebContext::with_buffer(&mut root_out);

        // Fork the sibling before the boundary begins. Both contexts share
        // the page registry, but only `root` enters the boundary transaction.
        let sibling_parts = root.context_parts();
        let mut sibling_out: Vec<u8> = Vec::new();
        let mut sibling =
            WebContext::with_buffer(&mut sibling_out).with_context_parts(sibling_parts);

        let checkpoint = root.rsjs_shared_state_seed_checkpoint();
        {
            let _doomed = root
                .rsjs_shared_state_enter(
                    "test.branch",
                    "sha256:doomed",
                    "DoomedBranch",
                    &[],
                    Rc::new(1u8),
                    "{\"branch\":\"doomed\"}".to_owned(),
                )
                .unwrap();
        }
        {
            let _survivor = sibling
                .rsjs_shared_state_enter(
                    "test.sibling",
                    "sha256:survivor",
                    "SurvivingSibling",
                    &[],
                    Rc::new(2u8),
                    "{\"branch\":\"survivor\"}".to_owned(),
                )
                .unwrap();
            sibling
                .rsjs_shared_state_acquire::<u8>(
                    "test.sibling",
                    "sha256:survivor",
                    "SurvivingSibling",
                    "SiblingConsumer",
                )
                .unwrap();
        }

        root.rsjs_shared_state_rollback_seeds(checkpoint);
        let table = root.rsjs_shared_state_seed_table_json().unwrap();
        assert!(
            !table.contains("test.branch"),
            "dead branch leaked: {table}"
        );
        assert!(
            table.contains("test.sibling"),
            "rollback erased a concurrently registered sibling: {table}"
        );
    }

    #[test]
    fn seed_rollback_removes_schema_owned_only_by_the_dead_branch() {
        let mut out: Vec<u8> = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let checkpoint = cx.rsjs_shared_state_seed_checkpoint();
        {
            let _doomed = cx
                .rsjs_shared_state_enter(
                    "test.reused-key",
                    "sha256:old",
                    "OldType",
                    &[],
                    Rc::new(1u8),
                    "{}".to_owned(),
                )
                .unwrap();
        }
        cx.rsjs_shared_state_rollback_seeds(checkpoint);

        let _replacement = cx
            .rsjs_shared_state_enter(
                "test.reused-key",
                "sha256:new",
                "NewType",
                &[],
                Rc::new(2u8),
                "{}".to_owned(),
            )
            .expect("a rewound branch must not poison the type-key registry");
    }

    #[test]
    fn committed_nested_seed_remains_owned_by_the_parent_boundary() {
        let mut out: Vec<u8> = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let outer = cx.rsjs_shared_state_seed_checkpoint();
        let inner = cx.rsjs_shared_state_seed_checkpoint();
        {
            let _scope = cx
                .rsjs_shared_state_enter(
                    "test.nested-transaction",
                    "sha256:nested",
                    "NestedState",
                    &[],
                    Rc::new(1u8),
                    "{}".to_owned(),
                )
                .unwrap();
        }
        cx.rsjs_shared_state_commit_seeds(inner);
        cx.rsjs_shared_state_rollback_seeds(outer);
        assert!(
            cx.rsjs_shared_state_seed_table_json().is_none(),
            "an outer rewind must discard seeds from a committed inner boundary"
        );
    }

    #[test]
    fn seed_checkpoint_and_rollback_do_not_materialize_cold_parts_channels() {
        let mut out: Vec<u8> = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let checkpoint = cx.rsjs_shared_state_seed_checkpoint();
        cx.rsjs_shared_state_rollback_seeds(checkpoint);
        assert!(cx
            .ensure_context_parts()
            .rsjs_shared_state
            .page
            .get()
            .is_none());
        assert!(cx
            .ensure_context_parts()
            .rsjs_shared_state
            .active
            .get()
            .is_none());
    }
}
