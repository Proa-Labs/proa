// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Request-aware metadata resolution. Static providers opt into precomputation;
//! all other providers run against the request's loader before rendering begins.

use crate::manifest::IslandManifest;
use proa_core::text::{text, StaticRaw};
use proa_core::{
    DataLoader, DynDataLoader, LoadError, LoadKey, LoadRequest, RequestContext, WebContext,
    WebRenderSync, WriteBuf,
};
use std::{borrow::Cow, future::Future, pin::Pin, sync::Arc};

pub type MetadataRef = &'static dyn DynMetadata;
pub type MetadataText = Cow<'static, str>;

/// Override, explicitly clear, or inherit a field. Lists replace on `Set`;
/// extend a parent's list explicitly in a generated patch when needed.
#[derive(Clone, Debug, Default)]
pub enum CascadeValue<T> {
    Set(T),
    Tombstone,
    #[default]
    Unset,
}

impl<T> CascadeValue<T> {
    pub fn resolve(self, parent_value: Option<T>) -> Option<T> {
        match self {
            Self::Set(value) => Some(value),
            Self::Tombstone => None,
            Self::Unset => parent_value,
        }
    }
}

/// Validated HTTP(S) base URL. Explicit bases include their public mount path.
#[derive(Clone, Debug)]
pub struct MetadataBase(Arc<url::Url>);

impl MetadataBase {
    pub fn new(value: &str) -> Result<Self, MetadataError> {
        let mut url = url::Url::parse(value).map_err(|_| MetadataError::InvalidBase)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(MetadataError::InvalidBase);
        }
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        Ok(Self(Arc::new(url)))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn from_request(request: &RequestContext) -> Result<Option<Self>, MetadataError> {
        if let Some(base) = request.extensions().get::<Self>() {
            return Ok(Some(base.clone()));
        }
        // Only an absolute request URI carries a known scheme. Do not guess
        // HTTPS from Host or trust arbitrary forwarding headers.
        let Some(uri) = request.location().map(|location| location.uri()) else {
            return Ok(None);
        };
        match (uri.scheme_str(), uri.authority()) {
            (Some(scheme), Some(authority)) => Self::new(&format!(
                "{scheme}://{authority}{}/",
                request.base_path().as_str()
            ))
            .map(Some),
            _ => Ok(None),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error(
        "metadata base must be an absolute HTTP(S) URL without credentials, query, or fragment"
    )]
    InvalidBase,
    #[error("relative metadata URL requires a metadata base or an absolute request URI")]
    MissingBase,
    #[error("metadata URL must resolve to HTTP(S)")]
    InvalidUrl,
    #[error("metadata generation suspended; use an async route response")]
    AsyncInSyncResponse,
    #[error("local metadata future requires a local async route response")]
    LocalInSendResponse,
    #[error("metadata data loading failed: {0}")]
    Load(#[from] LoadError),
    #[error("metadata generation failed: {0}")]
    Generate(#[from] anyhow::Error),
}

/// Read-only request and loader access for metadata generation.
///
/// The framework borrows these inputs from the page's render context. Authors
/// receive `&dyn MetadataContext`; they do not need lifetime parameters or a
/// separate request/loader instance.
pub trait MetadataContext: Sync {
    fn request(&self) -> &RequestContext;
    fn loader(&self) -> &dyn DynDataLoader;
}

struct MetadataInputs<'a> {
    request: &'a RequestContext,
    loader: &'a dyn DynDataLoader,
}

impl MetadataContext for MetadataInputs<'_> {
    fn request(&self) -> &RequestContext {
        self.request
    }
    fn loader(&self) -> &dyn DynDataLoader {
        self.loader
    }
}

impl dyn MetadataContext + '_ {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.request().param(name)
    }

    /// Decoded query pairs, preserving duplicates and empty values.
    pub fn search_params(&self) -> url::form_urlencoded::Parse<'_> {
        url::form_urlencoded::parse(
            self.request()
                .location()
                .and_then(|location| location.query())
                .unwrap_or("")
                .as_bytes(),
        )
    }

    pub fn metadata_base(&self) -> Result<Option<MetadataBase>, MetadataError> {
        MetadataBase::from_request(self.request())
    }

    pub async fn load_bytes(&self, key: LoadKey) -> Result<bytes::Bytes, LoadError> {
        self.load_bytes_request(LoadRequest::new(key)).await
    }

    pub async fn load_bytes_request(
        &self,
        request: LoadRequest,
    ) -> Result<bytes::Bytes, LoadError> {
        self.loader().load_boxed(&request).await?.into_bytes()
    }

    pub async fn load_json<T: serde::de::DeserializeOwned>(
        &self,
        key: LoadKey,
    ) -> Result<T, LoadError> {
        self.load_json_request(LoadRequest::new(key)).await
    }

    pub async fn load_json_request<T: serde::de::DeserializeOwned>(
        &self,
        request: LoadRequest,
    ) -> Result<T, LoadError> {
        let bytes = self.load_bytes_request(request).await?;
        serde_json::from_slice(&bytes).map_err(LoadError::from)
    }

    pub async fn load_typed<T: Send + Sync + 'static>(
        &self,
        key: LoadKey,
    ) -> Result<Arc<T>, LoadError> {
        self.load_typed_request(LoadRequest::new(key)).await
    }

    pub async fn load_typed_request<T: Send + Sync + 'static>(
        &self,
        request: LoadRequest,
    ) -> Result<Arc<T>, LoadError> {
        self.loader().load_boxed(&request).await?.downcast::<T>()
    }
}

/// Author request-aware metadata with a normal `async fn` implementation.
///
/// ```
/// use proa_core::LoadKey;
/// use proa_framework_core::{
///     CascadeValue, Metadata, MetadataContext, MetadataError, MetadataPatch,
///     ResolvedMetadata,
/// };
///
/// struct PostMetadata;
/// impl Metadata for PostMetadata {
///     async fn generate_metadata(
///         &self,
///         cx: &dyn MetadataContext,
///         _parent: &ResolvedMetadata,
///     ) -> Result<MetadataPatch, MetadataError> {
///         let slug = cx.param("slug").unwrap_or_default();
///         let title = cx.load_typed::<String>(LoadKey::named("post", slug)).await?;
///         Ok(MetadataPatch {
///             title: CascadeValue::Set(title.as_str().to_owned().into()),
///             ..Default::default()
///         })
///     }
/// }
/// ```
///
/// The return-position `Send` bound lets authors write `async fn` while the
/// framework retains the proof needed by Send response streams. Use
/// [`LocalMetadata`] for a generator that awaits request-local futures.
pub trait Metadata: Send + Sync + 'static {
    fn static_metadata(&self) -> Option<MetadataPatch> {
        None
    }

    /// Optional immediate hook; the default async generator calls it.
    fn metadata(&self, _cx: &dyn MetadataContext, _parent: &ResolvedMetadata) -> MetadataPatch {
        self.static_metadata()
            .unwrap_or_else(|| legacy_metadata_patch(self))
    }

    fn generate_metadata(
        &self,
        cx: &dyn MetadataContext,
        parent: &ResolvedMetadata,
    ) -> impl Future<Output = Result<MetadataPatch, MetadataError>> + Send {
        async move { Ok(self.metadata(cx, parent)) }
    }

    // Compatibility for existing static providers.
    fn title(&self) -> CascadeValue<&'static str> {
        CascadeValue::Unset
    }
    fn absolute_title(&self) -> bool {
        false
    }
    fn description(&self) -> CascadeValue<&'static str> {
        CascadeValue::Unset
    }
    fn open_graph(&self) -> CascadeOpenGraph {
        CascadeOpenGraph::default()
    }
}

/// Metadata generator whose future may retain request-local, non-Send state.
/// Register it through [`local_metadata`]. Ordinary generators implement
/// [`Metadata`] and can serve both local and Send responses.
#[allow(async_fn_in_trait)] // Local futures intentionally have no Send bound.
pub trait LocalMetadata: Send + Sync + 'static {
    fn static_metadata(&self) -> Option<MetadataPatch> {
        None
    }

    async fn generate_metadata(
        &self,
        cx: &dyn MetadataContext,
        parent: &ResolvedMetadata,
    ) -> Result<MetadataPatch, MetadataError>;
}

/// Explicit local-future registration adapter. It does not allocate or spawn.
pub struct LocalMetadataProvider<T>(T);

pub const fn local_metadata<T: LocalMetadata>(provider: T) -> LocalMetadataProvider<T> {
    LocalMetadataProvider(provider)
}

/// Object-safe storage adapter for heterogeneous route providers.
/// Application authors implement [`Metadata`] or [`LocalMetadata`]; future
/// erasure and named lifetimes stay at this framework boundary.
pub trait DynMetadata: Send + Sync + 'static {
    fn static_metadata_dyn(&self) -> Option<MetadataPatch>;
    fn legacy_metadata_dyn(&self) -> MetadataPatch;
    fn generate_metadata_dyn<'a>(
        &'a self,
        cx: &'a dyn MetadataContext,
        parent: &'a ResolvedMetadata,
    ) -> DynMetadataFuture<'a>;
}

impl<T: Metadata> DynMetadata for T {
    fn static_metadata_dyn(&self) -> Option<MetadataPatch> {
        self.static_metadata()
    }
    fn legacy_metadata_dyn(&self) -> MetadataPatch {
        legacy_metadata_patch(self)
    }
    fn generate_metadata_dyn<'a>(
        &'a self,
        cx: &'a dyn MetadataContext,
        parent: &'a ResolvedMetadata,
    ) -> DynMetadataFuture<'a> {
        DynMetadataFuture::Send(Box::pin(self.generate_metadata(cx, parent)))
    }
}

impl<T: LocalMetadata> DynMetadata for LocalMetadataProvider<T> {
    fn static_metadata_dyn(&self) -> Option<MetadataPatch> {
        self.0.static_metadata()
    }
    fn legacy_metadata_dyn(&self) -> MetadataPatch {
        MetadataPatch::default()
    }
    fn generate_metadata_dyn<'a>(
        &'a self,
        cx: &'a dyn MetadataContext,
        parent: &'a ResolvedMetadata,
    ) -> DynMetadataFuture<'a> {
        DynMetadataFuture::Local(Box::pin(self.0.generate_metadata(cx, parent)))
    }
}

/// Erased future used only by the route storage adapter.
#[doc(hidden)]
pub enum DynMetadataFuture<'a> {
    Send(Pin<Box<dyn Future<Output = Result<MetadataPatch, MetadataError>> + Send + 'a>>),
    Local(Pin<Box<dyn Future<Output = Result<MetadataPatch, MetadataError>> + 'a>>),
}

impl<'a> DynMetadataFuture<'a> {
    async fn resolve(self) -> Result<MetadataPatch, MetadataError> {
        match self {
            Self::Send(future) => future.await,
            Self::Local(future) => future.await,
        }
    }

    fn into_send(
        self,
    ) -> Result<
        Pin<Box<dyn Future<Output = Result<MetadataPatch, MetadataError>> + Send + 'a>>,
        MetadataError,
    > {
        match self {
            Self::Send(future) => Ok(future),
            Self::Local(_) => Err(MetadataError::LocalInSendResponse),
        }
    }
}

fn legacy_metadata_patch<M: Metadata + ?Sized>(provider: &M) -> MetadataPatch {
    MetadataPatch {
        title: provider.title().map(Cow::Borrowed),
        absolute_title: provider.absolute_title(),
        description: provider.description().map(Cow::Borrowed),
        open_graph: provider.open_graph().into(),
        ..Default::default()
    }
}

impl<T> CascadeValue<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> CascadeValue<U> {
        match self {
            Self::Set(value) => CascadeValue::Set(f(value)),
            Self::Tombstone => CascadeValue::Tombstone,
            Self::Unset => CascadeValue::Unset,
        }
    }
}

pub struct TitleOnly {
    pub title: &'static str,
}
impl TitleOnly {
    pub const fn new(title: &'static str) -> Self {
        Self { title }
    }
}
impl Metadata for TitleOnly {
    fn title(&self) -> CascadeValue<&'static str> {
        CascadeValue::Set(self.title)
    }
    fn static_metadata(&self) -> Option<MetadataPatch> {
        Some(MetadataPatch {
            title: CascadeValue::Set(self.title.into()),
            ..Default::default()
        })
    }
}

#[derive(Clone, Default)]
pub struct MetadataPatch {
    pub title: CascadeValue<MetadataText>,
    pub absolute_title: bool,
    pub description: CascadeValue<MetadataText>,
    pub canonical: CascadeValue<MetadataText>,
    pub metadata_base: CascadeValue<MetadataBase>,
    pub open_graph: OpenGraphPatch,
}

#[derive(Clone, Default)]
pub struct OpenGraphPatch {
    pub title: CascadeValue<MetadataText>,
    pub description: CascadeValue<MetadataText>,
    pub site_name: CascadeValue<MetadataText>,
    pub determiner: CascadeValue<MetadataText>,
    pub r#type: CascadeValue<MetadataText>,
    pub url: CascadeValue<MetadataText>,
    pub locale: CascadeValue<MetadataText>,
    pub locale_alternates: CascadeValue<Cow<'static, [MetadataText]>>,
    pub images: CascadeValue<Cow<'static, [ResolvedOpenGraphImage]>>,
    pub audio: CascadeValue<Cow<'static, [ResolvedOpenGraphMedia]>>,
    pub video: CascadeValue<Cow<'static, [ResolvedOpenGraphMedia]>>,
}

impl From<CascadeOpenGraph> for OpenGraphPatch {
    fn from(og: CascadeOpenGraph) -> Self {
        Self {
            title: og.title.map(Cow::Borrowed),
            description: og.description.map(Cow::Borrowed),
            site_name: og.site_name.map(Cow::Borrowed),
            determiner: og.determiner.map(Cow::Borrowed),
            r#type: og.r#type.map(Cow::Borrowed),
            url: og.url.map(Cow::Borrowed),
            locale: og.locale.map(Cow::Borrowed),
            locale_alternates: og
                .locale_alternates
                .map(|values| Cow::Owned(values.iter().map(|v| Cow::Borrowed(*v)).collect())),
            images: og.images.map(|values| {
                Cow::Owned(
                    values
                        .iter()
                        .filter_map(ResolvedOpenGraphImage::from_image)
                        .collect(),
                )
            }),
            audio: og.audio.map(|values| {
                Cow::Owned(
                    values
                        .iter()
                        .filter_map(ResolvedOpenGraphMedia::from_media)
                        .collect(),
                )
            }),
            video: og.video.map(|values| {
                Cow::Owned(
                    values
                        .iter()
                        .filter_map(ResolvedOpenGraphMedia::from_media)
                        .collect(),
                )
            }),
        }
    }
}

#[derive(Clone, Default)]
pub struct ResolvedMetadata {
    pub title: Option<MetadataText>,
    pub description: Option<MetadataText>,
    pub canonical: Option<MetadataText>,
    pub metadata_base: Option<MetadataBase>,
    pub open_graph: ResolvedOpenGraph,
}

/// Read the resolved request snapshot from any local render context. The
/// snapshot travels with the context into deferred streams and boundaries.
pub fn metadata_from_context<L: DataLoader, B: WriteBuf>(
    cx: &WebContext<L, B>,
) -> Option<&ResolvedMetadata> {
    cx.request()
        .extensions()
        .get::<Arc<ResolvedMetadata>>()
        .map(AsRef::as_ref)
}

#[derive(Clone, Default)]
pub struct ResolvedOpenGraph {
    pub title: Option<MetadataText>,
    pub description: Option<MetadataText>,
    pub site_name: Option<MetadataText>,
    pub determiner: Option<MetadataText>,
    pub r#type: Option<MetadataText>,
    pub url: Option<MetadataText>,
    pub locale: Option<MetadataText>,
    pub locale_alternates: Cow<'static, [MetadataText]>,
    pub images: Cow<'static, [ResolvedOpenGraphImage]>,
    pub audio: Cow<'static, [ResolvedOpenGraphMedia]>,
    pub video: Cow<'static, [ResolvedOpenGraphMedia]>,
}
pub type ComputedOpenGraph = ResolvedOpenGraph;

#[derive(Clone)]
pub struct ResolvedOpenGraphImage {
    pub url: MetadataText,
    pub secure_url: Option<MetadataText>,
    pub mime_type: Option<MetadataText>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub alt: Option<MetadataText>,
}
impl ResolvedOpenGraphImage {
    pub const fn new(url: &'static str) -> Self {
        Self {
            url: Cow::Borrowed(url),
            secure_url: None,
            mime_type: None,
            width: None,
            height: None,
            alt: None,
        }
    }
    fn from_image(image: &OpenGraphImage) -> Option<Self> {
        Some(Self {
            url: Cow::Borrowed(image.url.or(image.path)?),
            secure_url: image.secure_url.map(Cow::Borrowed),
            mime_type: image.mime_type.map(Cow::Borrowed),
            width: image.width,
            height: image.height,
            alt: image.alt.map(Cow::Borrowed),
        })
    }
}

#[derive(Clone)]
pub struct ResolvedOpenGraphMedia {
    pub url: MetadataText,
    pub secure_url: Option<MetadataText>,
    pub mime_type: Option<MetadataText>,
}
impl ResolvedOpenGraphMedia {
    pub const fn new(url: &'static str) -> Self {
        Self {
            url: Cow::Borrowed(url),
            secure_url: None,
            mime_type: None,
        }
    }
    fn from_media(media: &OpenGraphMedia) -> Option<Self> {
        Some(Self {
            url: Cow::Borrowed(media.url?),
            secure_url: media.secure_url.map(Cow::Borrowed),
            mime_type: media.mime_type.map(Cow::Borrowed),
        })
    }
}

/// One cascade engine backs static precomputation and per-request resolution.
#[derive(Clone, Default)]
struct MetadataState {
    resolved: ResolvedMetadata,
    title_template: Option<MetadataText>,
    og_title_template: Option<MetadataText>,
    og_description_template: Option<MetadataText>,
    raw_title: Option<MetadataText>,
}

fn apply_text(
    field: &mut Option<MetadataText>,
    template: &mut Option<MetadataText>,
    value: CascadeValue<MetadataText>,
) {
    match value {
        CascadeValue::Set(value) if value.contains("%s") => *template = Some(value),
        CascadeValue::Set(value) => {
            *field = Some(match template {
                Some(template) => Cow::Owned(template.replace("%s", &value)),
                None => value,
            });
        }
        CascadeValue::Tombstone => {
            *field = None;
            *template = None;
        }
        CascadeValue::Unset => {}
    }
}

impl MetadataState {
    fn apply(&mut self, patch: MetadataPatch) {
        match &patch.title {
            CascadeValue::Set(value) if patch.absolute_title => {
                self.resolved.title = Some(value.clone());
                self.title_template = None;
                self.raw_title = None;
            }
            CascadeValue::Set(value) => {
                if !value.contains("%s") {
                    self.raw_title = Some(value.clone());
                }
                apply_text(
                    &mut self.resolved.title,
                    &mut self.title_template,
                    patch.title,
                );
            }
            CascadeValue::Tombstone => {
                self.resolved.title = None;
                self.title_template = None;
                self.raw_title = None;
            }
            CascadeValue::Unset => {}
        }
        self.resolved.description = patch.description.resolve(self.resolved.description.take());
        self.resolved.canonical = patch.canonical.resolve(self.resolved.canonical.take());
        self.resolved.metadata_base = patch
            .metadata_base
            .resolve(self.resolved.metadata_base.take());
        let og = &mut self.resolved.open_graph;
        apply_text(
            &mut og.title,
            &mut self.og_title_template,
            patch.open_graph.title,
        );
        apply_text(
            &mut og.description,
            &mut self.og_description_template,
            patch.open_graph.description,
        );
        og.site_name = patch.open_graph.site_name.resolve(og.site_name.take());
        og.determiner = patch.open_graph.determiner.resolve(og.determiner.take());
        og.r#type = patch.open_graph.r#type.resolve(og.r#type.take());
        og.url = patch.open_graph.url.resolve(og.url.take());
        og.locale = patch.open_graph.locale.resolve(og.locale.take());
        macro_rules! list {
            ($field:ident) => {
                match patch.open_graph.$field {
                    CascadeValue::Set(value) => og.$field = value,
                    CascadeValue::Tombstone => og.$field = Cow::Borrowed(&[]),
                    CascadeValue::Unset => {}
                }
            };
        }
        list!(locale_alternates);
        list!(images);
        list!(audio);
        list!(video);
    }

    fn snapshot(&self) -> ResolvedMetadata {
        let mut value = self.resolved.clone();
        if value.open_graph.title.is_none() {
            if let (Some(template), Some(title)) = (&self.og_title_template, &self.raw_title) {
                value.open_graph.title = Some(Cow::Owned(template.replace("%s", title)));
            }
        }
        if value.open_graph.description.is_none() {
            if let (Some(template), Some(description)) =
                (&self.og_description_template, &value.description)
            {
                value.open_graph.description =
                    Some(Cow::Owned(template.replace("%s", description)));
            }
        }
        value
    }
}

/// Precomputed request-independent prefix plus the remaining providers. Never
/// evaluates providers after the first dynamic provider at startup.
#[derive(Clone, Default)]
pub struct ComputedRouteMeta {
    state: Arc<MetadataState>,
    snapshot: Arc<ResolvedMetadata>,
    pending: Arc<[MetadataRef]>,
}

impl std::ops::Deref for ComputedRouteMeta {
    type Target = ResolvedMetadata;
    fn deref(&self) -> &Self::Target {
        &self.snapshot
    }
}

impl ComputedRouteMeta {
    pub fn cascade(parent: &Self, local_providers: &[MetadataRef]) -> Self {
        let mut plan = parent.clone();
        let mut pending = parent.pending.to_vec();
        let mut changed = false;
        for provider in local_providers {
            if pending.is_empty() {
                if let Some(patch) = provider.static_metadata_dyn() {
                    Arc::make_mut(&mut plan.state).apply(patch);
                    changed = true;
                    continue;
                }
            }
            pending.push(*provider);
        }
        if changed {
            plan.snapshot = Arc::new(plan.state.snapshot());
        }
        plan.pending = pending.into();
        plan
    }

    pub fn is_static(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn resolve_sync(
        &self,
        request: &RequestContext,
        loader: &dyn DynDataLoader,
    ) -> Result<ResolvedMetadata, MetadataError> {
        let cx = MetadataInputs { request, loader };
        if self.is_static() {
            return finish_urls(self.snapshot.as_ref().clone(), cx.request);
        }
        let mut state = self.state.as_ref().clone();
        for provider in self.pending.iter() {
            let parent = state.snapshot();
            use futures::FutureExt;
            let result = provider
                .generate_metadata_dyn(&cx, &parent)
                .resolve()
                .now_or_never()
                .ok_or(MetadataError::AsyncInSyncResponse)?;
            state.apply(result?);
        }
        finish_urls(state.snapshot(), cx.request)
    }

    pub async fn resolve(
        &self,
        request: &RequestContext,
        loader: &dyn DynDataLoader,
    ) -> Result<ResolvedMetadata, MetadataError> {
        let cx = MetadataInputs { request, loader };
        if self.is_static() {
            return finish_urls(self.snapshot.as_ref().clone(), cx.request);
        }
        let mut state = self.state.as_ref().clone();
        for provider in self.pending.iter() {
            let parent = state.snapshot();
            state.apply(
                provider
                    .generate_metadata_dyn(&cx, &parent)
                    .resolve()
                    .await?,
            );
        }
        finish_urls(state.snapshot(), cx.request)
    }

    /// Resolve for a Send response. Reject local continuations before rendering.
    pub async fn resolve_send(
        &self,
        request: &RequestContext,
        loader: &dyn DynDataLoader,
    ) -> Result<ResolvedMetadata, MetadataError> {
        let cx = MetadataInputs { request, loader };
        if self.is_static() {
            return finish_urls(self.snapshot.as_ref().clone(), cx.request);
        }
        let mut state = self.state.as_ref().clone();
        for provider in self.pending.iter() {
            let parent = state.snapshot();
            let outcome = provider.generate_metadata_dyn(&cx, &parent).into_send()?;
            state.apply(outcome.await?);
        }
        finish_urls(state.snapshot(), cx.request)
    }
}

/// Compatibility helper for request-independent metadata. For request-aware
/// providers, call the plan's resolve/resolve_sync with the real request.
pub fn resolved_metadata_from_computed(computed: &ComputedRouteMeta) -> ResolvedMetadata {
    computed.snapshot.as_ref().clone()
}

/// Compatibility resolver for legacy getters and explicitly static patches.
/// Generated providers are never called with invented request inputs. Use
/// `ComputedRouteMeta::resolve` with the real request for generated metadata.
/// URL references are left unnormalized by this request-free helper.
pub fn resolve_metadata_stack(metadata: &[MetadataRef]) -> Option<ResolvedMetadata> {
    if metadata.is_empty() {
        return None;
    }
    let mut state = MetadataState::default();
    for provider in metadata {
        state.apply(
            provider
                .static_metadata_dyn()
                .unwrap_or_else(|| provider.legacy_metadata_dyn()),
        );
    }
    Some(state.snapshot())
}

fn resolve_url(value: &mut MetadataText, base: Option<&MetadataBase>) -> Result<(), MetadataError> {
    if let Ok(url) = url::Url::parse(value) {
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(MetadataError::InvalidUrl);
        }
        return Ok(());
    }
    let base = base.ok_or(MetadataError::MissingBase)?;
    // Root-relative metadata is application-local. Explicit bases supply the
    // mount prefix; inferred bases include the request's BasePath.
    let path = if value.starts_with('/') && !value.starts_with("//") {
        value.trim_start_matches('/')
    } else {
        value.as_ref()
    };
    let url = base.0.join(path).map_err(|_| MetadataError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(MetadataError::InvalidUrl);
    }
    *value = Cow::Owned(url.into());
    Ok(())
}

fn finish_urls(
    mut value: ResolvedMetadata,
    request: &RequestContext,
) -> Result<ResolvedMetadata, MetadataError> {
    let base = match &value.metadata_base {
        Some(base) => Some(base.clone()),
        None => MetadataBase::from_request(request)?,
    };
    if let Some(url) = &mut value.canonical {
        resolve_url(url, base.as_ref())?;
    }
    if let Some(url) = &mut value.open_graph.url {
        resolve_url(url, base.as_ref())?;
    }
    // Keep borrowed static lists borrowed when all their URLs are absolute.
    fn relative(value: &str) -> bool {
        url::Url::parse(value).is_err()
    }
    if value
        .open_graph
        .images
        .iter()
        .any(|image| relative(&image.url) || image.secure_url.as_deref().is_some_and(relative))
    {
        for image in value.open_graph.images.to_mut() {
            resolve_url(&mut image.url, base.as_ref())?;
            if let Some(url) = &mut image.secure_url {
                resolve_url(url, base.as_ref())?;
            }
        }
    } else {
        for image in value.open_graph.images.iter() {
            validate_absolute_url(&image.url)?;
            if let Some(url) = &image.secure_url {
                validate_absolute_url(url)?;
            }
        }
    }
    for media in [&mut value.open_graph.audio, &mut value.open_graph.video] {
        if media
            .iter()
            .any(|item| relative(&item.url) || item.secure_url.as_deref().is_some_and(relative))
        {
            for item in media.to_mut() {
                resolve_url(&mut item.url, base.as_ref())?;
                if let Some(url) = &mut item.secure_url {
                    resolve_url(url, base.as_ref())?;
                }
            }
        } else {
            for item in media.iter() {
                validate_absolute_url(&item.url)?;
                if let Some(url) = &item.secure_url {
                    validate_absolute_url(url)?;
                }
            }
        }
    }
    value.metadata_base = base;
    Ok(value)
}

fn validate_absolute_url(value: &str) -> Result<(), MetadataError> {
    let url = url::Url::parse(value).map_err(|_| MetadataError::InvalidUrl)?;
    if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
        Ok(())
    } else {
        Err(MetadataError::InvalidUrl)
    }
}

/// Resolve once before rendering; the request owns the result for the head,
/// client payload and deferred boundaries. No Tokio/task-local dependency.
pub(crate) fn prepare_metadata_sync<L: DataLoader>(
    request: &mut RequestContext,
    loader: &L,
) -> Result<(), MetadataError> {
    if let Some(plan) = request.extensions().get::<ComputedRouteMeta>() {
        let resolved = plan.resolve_sync(request, loader)?;
        request.extensions_mut().insert(Arc::new(resolved));
    }
    Ok(())
}

pub(crate) async fn prepare_metadata<L: DataLoader>(
    request: &mut RequestContext,
    loader: &L,
) -> Result<(), MetadataError> {
    if let Some(plan) = request.extensions().get::<ComputedRouteMeta>() {
        let resolved = plan.resolve(request, loader).await?;
        request.extensions_mut().insert(Arc::new(resolved));
    }
    Ok(())
}

pub(crate) async fn prepare_metadata_send<L: DataLoader>(
    request: &mut RequestContext,
    loader: &L,
) -> Result<(), MetadataError> {
    if let Some(plan) = request.extensions().get::<ComputedRouteMeta>() {
        let resolved = plan.resolve_send(request, loader).await?;
        request.extensions_mut().insert(Arc::new(resolved));
    }
    Ok(())
}

/// Cascade specification for Open Graph metadata.
#[derive(Clone, Default)]
pub struct CascadeOpenGraph {
    pub title: CascadeValue<&'static str>,
    pub description: CascadeValue<&'static str>,
    pub site_name: CascadeValue<&'static str>,
    pub determiner: CascadeValue<&'static str>,
    pub r#type: CascadeValue<&'static str>,
    pub url: CascadeValue<&'static str>,
    pub locale: CascadeValue<&'static str>,
    pub locale_alternates: CascadeValue<&'static [&'static str]>,
    pub images: CascadeValue<&'static [OpenGraphImage]>,
    pub audio: CascadeValue<&'static [OpenGraphMedia]>,
    pub video: CascadeValue<&'static [OpenGraphMedia]>,
}

/// Static Open Graph descriptor returned by metadata providers.
#[derive(Clone, Default)]
pub struct OpenGraph {
    pub title: Option<&'static str>,
    pub description: Option<&'static str>,
    pub site_name: Option<&'static str>,
    pub determiner: Option<&'static str>,
    pub r#type: Option<&'static str>,
    pub url: Option<&'static str>,
    pub locale: Option<&'static str>,
    pub locale_alternates: &'static [&'static str],
    pub images: &'static [OpenGraphImage],
    pub audio: &'static [OpenGraphMedia],
    pub video: &'static [OpenGraphMedia],
}

/// Static Open Graph image descriptor.
#[derive(Clone)]
pub struct OpenGraphImage {
    pub url: Option<&'static str>,
    pub secure_url: Option<&'static str>,
    pub path: Option<&'static str>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub alt: Option<&'static str>,
    pub mime_type: Option<&'static str>,
}

impl OpenGraphImage {
    pub const fn new(url: &'static str) -> Self {
        Self {
            url: Some(url),
            secure_url: None,
            path: None,
            width: None,
            height: None,
            alt: None,
            mime_type: None,
        }
    }

    pub const fn with_secure_url(mut self, secure_url: &'static str) -> Self {
        self.secure_url = Some(secure_url);
        self
    }

    pub const fn with_path(mut self, path: &'static str) -> Self {
        self.path = Some(path);
        self
    }

    pub const fn with_width(mut self, width: u32) -> Self {
        self.width = Some(width);
        self
    }

    pub const fn with_height(mut self, height: u32) -> Self {
        self.height = Some(height);
        self
    }

    pub const fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    pub const fn with_alt(mut self, alt: &'static str) -> Self {
        self.alt = Some(alt);
        self
    }

    pub const fn with_mime(mut self, mime_type: &'static str) -> Self {
        self.mime_type = Some(mime_type);
        self
    }
}

/// Static Open Graph media descriptor (audio/video).
#[derive(Clone)]
pub struct OpenGraphMedia {
    pub url: Option<&'static str>,
    pub secure_url: Option<&'static str>,
    pub mime_type: Option<&'static str>,
}

impl OpenGraphMedia {
    pub const fn new(url: &'static str) -> Self {
        Self {
            url: Some(url),
            secure_url: None,
            mime_type: None,
        }
    }

    pub const fn with_secure_url(mut self, secure_url: &'static str) -> Self {
        self.secure_url = Some(secure_url);
        self
    }

    pub const fn with_mime(mut self, mime_type: &'static str) -> Self {
        self.mime_type = Some(mime_type);
        self
    }
}

pub fn css_asset_path(manifest: &IslandManifest) -> Option<&str> {
    manifest.assets.css.as_ref().map(|css| css.path.as_ref())
}

/// Write a string as an HTML attribute value, escaping `& < > " '` for the
/// active dialect.
///
/// The metadata head builds `<meta>` / `<link>` tags by hand with `StaticRaw`
/// scaffolding, so any value placed inside `"..."` must use attribute escaping,
/// which neutralizes the closing quote. Plain text escaping (`text()`) does NOT
/// escape `"` and would let a value carrying a quote break out of the attribute
/// and inject further attributes (for example an `http-equiv="refresh"`
/// redirect). All request-generated values use this same escaping path.
fn write_attr_value<L: DataLoader, B: WriteBuf>(cx: &mut WebContext<L, B>, value: &str) {
    let dialect = cx.__proa_current_escape_dialect();
    proa_core::shared::escape::escape_attr_value_for_dialect(&mut cx.out, value, dialect).unwrap();
}

pub fn render_css_link<L: DataLoader, B: WriteBuf>(cx: &mut WebContext<L, B>, css_path: &str) {
    StaticRaw("<link rel=\"stylesheet\" href=\"")
        .render(cx)
        .unwrap();
    write_attr_value(cx, css_path);
    StaticRaw("\"/>").render(cx).unwrap();
}

pub fn render_metadata_head<L: DataLoader, B: WriteBuf>(
    cx: &mut WebContext<L, B>,
    metadata: &ResolvedMetadata,
) {
    if let Some(title) = &metadata.title {
        StaticRaw("<title>").render(cx).unwrap();
        text(title.as_ref()).render(cx).unwrap();
        StaticRaw("</title>").render(cx).unwrap();
    }

    if let Some(description) = &metadata.description {
        render_named_meta(cx, "description", description);
    }

    if let Some(url) = &metadata.canonical {
        StaticRaw("<link rel=\"canonical\" href=\"")
            .render(cx)
            .unwrap();
        write_attr_value(cx, url);
        StaticRaw("\"/>").render(cx).unwrap();
    }

    let og = &metadata.open_graph;

    if let Some(title) = &og.title {
        render_property_meta(cx, "og:title", title.as_ref());
    }

    if let Some(description) = &og.description {
        render_property_meta(cx, "og:description", description.as_ref());
    }

    if let Some(site_name) = &og.site_name {
        render_property_meta(cx, "og:site_name", site_name.as_ref());
    }

    if let Some(determiner) = &og.determiner {
        render_property_meta(cx, "og:determiner", determiner);
    }

    if let Some(kind) = &og.r#type {
        render_property_meta(cx, "og:type", kind);
    }

    if let Some(url) = &og.url {
        render_property_meta(cx, "og:url", url);
    }

    if let Some(locale) = &og.locale {
        render_property_meta(cx, "og:locale", locale);
    }

    for alternate in og.locale_alternates.iter() {
        render_property_meta(cx, "og:locale:alternate", alternate);
    }

    for image in og.images.iter() {
        render_property_meta(cx, "og:image", image.url.as_ref());
        if let Some(secure_url) = image.secure_url.as_ref() {
            render_property_meta(cx, "og:image:secure_url", secure_url.as_ref());
        }
        if let Some(width) = image.width {
            render_numeric_property_meta(cx, "og:image:width", width);
        }
        if let Some(height) = image.height {
            render_numeric_property_meta(cx, "og:image:height", height);
        }
        if let Some(alt) = &image.alt {
            render_property_meta(cx, "og:image:alt", alt);
        }
        if let Some(mime) = &image.mime_type {
            render_property_meta(cx, "og:image:type", mime);
        }
    }

    for media in og.audio.iter() {
        render_property_meta(cx, "og:audio", media.url.as_ref());
        if let Some(secure_url) = media.secure_url.as_ref() {
            render_property_meta(cx, "og:audio:secure_url", secure_url.as_ref());
        }
        if let Some(mime) = &media.mime_type {
            render_property_meta(cx, "og:audio:type", mime);
        }
    }

    for media in og.video.iter() {
        render_property_meta(cx, "og:video", media.url.as_ref());
        if let Some(secure_url) = media.secure_url.as_ref() {
            render_property_meta(cx, "og:video:secure_url", secure_url.as_ref());
        }
        if let Some(mime) = &media.mime_type {
            render_property_meta(cx, "og:video:type", mime);
        }
    }
}

fn render_named_meta<L: DataLoader, B: WriteBuf>(
    cx: &mut WebContext<L, B>,
    name: &str,
    content: &str,
) {
    StaticRaw("<meta name=\"").render(cx).unwrap();
    write_attr_value(cx, name);
    StaticRaw("\" content=\"").render(cx).unwrap();
    write_attr_value(cx, content);
    StaticRaw("\"/>").render(cx).unwrap();
}

fn render_property_meta<L: DataLoader, B: WriteBuf>(
    cx: &mut WebContext<L, B>,
    property: &str,
    content: &str,
) {
    StaticRaw("<meta property=\"").render(cx).unwrap();
    write_attr_value(cx, property);
    StaticRaw("\" content=\"").render(cx).unwrap();
    write_attr_value(cx, content);
    StaticRaw("\"/>").render(cx).unwrap();
}

fn render_numeric_property_meta<L: DataLoader, B: WriteBuf>(
    cx: &mut WebContext<L, B>,
    property: &str,
    content: u32,
) {
    StaticRaw("<meta property=\"").render(cx).unwrap();
    write_attr_value(cx, property);
    StaticRaw("\" content=\"").render(cx).unwrap();
    content.render(cx).unwrap();
    StaticRaw("\"/>").render(cx).unwrap();
}

/// Send-context companion; metadata rendering itself only writes escaped output.
pub fn render_metadata_head_send<L: DataLoader, B: WriteBuf + Send>(
    cx: &mut proa_core::SendWebContext<L, B>,
    metadata: &ResolvedMetadata,
) {
    cx.__proa_with_stateless_context(|local| render_metadata_head(local, metadata));
}

#[cfg(test)]
mod tests;
