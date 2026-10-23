// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Declare document metadata before rendering `cx.metadata.head()`.
//! Manual head HTML is independent of this state.

use crate::mode::ContentMode;
use crate::shared::escape::{
    escape_attr_value_for_dialect, escape_text_for_dialect, EscapeDialect,
};
use crate::{
    DataLoader, RenderOutcome, SendWebContext, WebContext, WebRender, WebRenderSend,
    WebRenderSendSync, WebRenderSync, WriteBuf, WriteError,
};
use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};

type Text = Cow<'static, str>;

#[derive(Clone, Debug, Default)]
enum Field<T> {
    #[default]
    Inherit,
    Set(T),
    Clear,
}
impl<T> Field<T> {
    fn apply(self, target: &mut Option<T>) {
        match self {
            Self::Inherit => {}
            Self::Set(value) => *target = Some(value),
            Self::Clear => *target = None,
        }
    }
}

/// A declaration of managed document metadata. Omitted fields inherit.
/// Dynamic text must be owned (`String`); static literals stay borrowed.
#[derive(Clone, Debug, Default)]
pub struct Metadata {
    title: Field<Text>,
    title_template: Field<Text>,
    absolute_title: bool,
    description: Field<Text>,
    canonical: Field<Text>,
    open_graph: OpenGraph,
}

impl Metadata {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn absolute_title(mut self, value: impl Into<Text>) -> Self {
        self.title = Field::Set(value.into());
        self.absolute_title = true;
        self
    }
    pub fn open_graph(mut self, declare: impl FnOnce(&mut OpenGraph)) -> Self {
        self.open_graph.enabled = true;
        declare(&mut self.open_graph);
        self
    }
    pub fn title(mut self, value: impl Into<Text>) -> Self {
        self.title = Field::Set(value.into());
        self.absolute_title = false;
        self
    }
    pub fn clear_title(mut self) -> Self {
        self.title = Field::Clear;
        self
    }
    pub fn title_template(mut self, value: impl Into<Text>) -> Self {
        self.title_template = Field::Set(value.into());
        self
    }
    pub fn clear_title_template(mut self) -> Self {
        self.title_template = Field::Clear;
        self
    }
    pub fn description(mut self, value: impl Into<Text>) -> Self {
        self.description = Field::Set(value.into());
        self
    }
    pub fn clear_description(mut self) -> Self {
        self.description = Field::Clear;
        self
    }
    pub fn canonical(mut self, value: impl Into<Text>) -> Self {
        self.canonical = Field::Set(value.into());
        self
    }
    pub fn clear_canonical(mut self) -> Self {
        self.canonical = Field::Clear;
        self
    }
}

/// Structured Open Graph declaration. Fields merge individually; image lists
/// replace the preceding list. Omitted title, description, and URL fall back to
/// the final document values at emission. Explicit clears suppress fallback.
#[derive(Clone, Debug, Default)]
pub struct OpenGraph {
    enabled: bool,
    title: Field<Text>,
    description: Field<Text>,
    site_name: Field<Text>,
    determiner: Field<Text>,
    ty: Field<Text>,
    url: Field<Text>,
    locale: Field<Text>,
    images: Field<Vec<OpenGraphImage>>,
}
impl OpenGraph {
    pub fn title(&mut self, value: impl Into<Text>) -> &mut Self {
        self.title = Field::Set(value.into());
        self
    }
    pub fn clear_title(&mut self) -> &mut Self {
        self.title = Field::Clear;
        self
    }
    pub fn description(&mut self, value: impl Into<Text>) -> &mut Self {
        self.description = Field::Set(value.into());
        self
    }
    pub fn clear_description(&mut self) -> &mut Self {
        self.description = Field::Clear;
        self
    }
    pub fn site_name(&mut self, value: impl Into<Text>) -> &mut Self {
        self.site_name = Field::Set(value.into());
        self
    }
    pub fn clear_site_name(&mut self) -> &mut Self {
        self.site_name = Field::Clear;
        self
    }
    pub fn determiner(&mut self, value: impl Into<Text>) -> &mut Self {
        self.determiner = Field::Set(value.into());
        self
    }
    pub fn clear_determiner(&mut self) -> &mut Self {
        self.determiner = Field::Clear;
        self
    }
    pub fn ty(&mut self, value: impl Into<Text>) -> &mut Self {
        self.ty = Field::Set(value.into());
        self
    }
    pub fn clear_type(&mut self) -> &mut Self {
        self.ty = Field::Clear;
        self
    }
    pub fn url(&mut self, value: impl Into<Text>) -> &mut Self {
        self.url = Field::Set(value.into());
        self
    }
    pub fn clear_url(&mut self) -> &mut Self {
        self.url = Field::Clear;
        self
    }
    pub fn locale(&mut self, value: impl Into<Text>) -> &mut Self {
        self.locale = Field::Set(value.into());
        self
    }
    pub fn clear_locale(&mut self) -> &mut Self {
        self.locale = Field::Clear;
        self
    }
    /// Replace the complete inherited or preceding image list.
    pub fn images<I, T>(&mut self, images: I) -> &mut Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OpenGraphImage>,
    {
        self.images = Field::Set(images.into_iter().map(Into::into).collect());
        self
    }
    /// Add one image to this declaration. The declaration's list replaces the
    /// previous list when applied; separate declarations do not append.
    pub fn image(&mut self, image: impl Into<OpenGraphImage>) -> &mut Self {
        if !matches!(self.images, Field::Set(_)) {
            self.images = Field::Set(Vec::new());
        }
        if let Field::Set(images) = &mut self.images {
            images.push(image.into());
        }
        self
    }
    pub fn clear_images(&mut self) -> &mut Self {
        self.images = Field::Clear;
        self
    }
}

/// One image and its associated Open Graph subproperties.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenGraphImage {
    url: Text,
    secure_url: Option<Text>,
    mime_type: Option<Text>,
    width: Option<u32>,
    height: Option<u32>,
    alt: Option<Text>,
}
impl OpenGraphImage {
    pub fn new(url: impl Into<Text>) -> Self {
        Self {
            url: url.into(),
            secure_url: None,
            mime_type: None,
            width: None,
            height: None,
            alt: None,
        }
    }
    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }
    pub fn with_alt(mut self, alt: impl Into<Text>) -> Self {
        self.alt = Some(alt.into());
        self
    }
    pub fn with_secure_url(mut self, url: impl Into<Text>) -> Self {
        self.secure_url = Some(url.into());
        self
    }
    pub fn with_mime_type(mut self, mime_type: impl Into<Text>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }
}
impl From<&'static str> for OpenGraphImage {
    fn from(url: &'static str) -> Self {
        Self::new(url)
    }
}
impl From<String> for OpenGraphImage {
    fn from(url: String) -> Self {
        Self::new(url)
    }
}
impl From<Text> for OpenGraphImage {
    fn from(url: Text) -> Self {
        Self::new(url)
    }
}

#[derive(Debug, Default)]
struct State {
    open_graph: bool,
    title: Option<Text>,
    title_format: Option<Text>,
    inherited_template: Option<Text>,
    descendant_template: Option<Text>,
    description: Option<Text>,
    canonical: Option<Text>,
    og_title: Field<Text>,
    og_description: Field<Text>,
    og_site_name: Option<Text>,
    og_determiner: Option<Text>,
    og_ty: Option<Text>,
    og_url: Field<Text>,
    og_locale: Option<Text>,
    images: Option<Vec<OpenGraphImage>>,
}

/// Stateless access to the managed head render command. All mutable state is
/// private to the active context; replacing this façade cannot reset it.
#[derive(Clone, Copy, Debug, Default)]
pub struct DocumentMetadata;
impl DocumentMetadata {
    /// Rendering the returned command emits the active context's metadata and
    /// closes declarations. Obtaining it takes no snapshot and retains no borrow.
    pub fn head(&self) -> Head {
        Head
    }
}

/// A rejected declaration or head operation. Errors never include metadata values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MetadataError {
    #[error("metadata was already emitted; declare before rendering the head")]
    Closed,
    #[error("deferred branches and fallbacks cannot change or emit document metadata")]
    Ineligible,
    #[error("managed metadata head was already emitted")]
    DuplicateHead,
    #[error("metadata title template must contain exactly one %s placeholder")]
    InvalidTitleTemplate,
    #[error("{field} must be an absolute public HTTP(S) URL without credentials, whitespace, controls, or backslashes")]
    InvalidUrl { field: &'static str },
}

/// Private request-local managed metadata, shared across scoped writers.
/// No registry is allocated until the first nonempty declaration.
#[derive(Debug, Default)]
pub(super) struct MetadataState {
    state: Option<Box<State>>,
    closed: bool,
    denied: bool,
    title_only: bool,
    rejections: u32,
}

impl MetadataState {
    /// Number of rejected declarations/emissions in this context, saturating at
    /// `u32::MAX`. Intended for strict render tests and development diagnostics.
    pub fn rejections(&self) -> usize {
        self.rejections as usize
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    #[doc(hidden)]
    pub fn denied() -> Self {
        Self {
            denied: true,
            ..Self::default()
        }
    }

    /// Switch an open context into the existing navigation adapter's title-only
    /// mode without resetting its declarations or reopening an emitted head.
    pub(super) fn enable_title_only(&mut self) -> Result<(), MetadataError> {
        if self.denied {
            return Err(self.reject(MetadataError::Ineligible));
        }
        if self.closed {
            return Err(self.reject(MetadataError::Closed));
        }
        self.title_only = true;
        Ok(())
    }

    /// The document title as the head would render it, with the inherited
    /// title template applied.
    pub fn resolved_title(&self) -> Option<String> {
        let state = self.state.as_ref()?;
        let title = state.title.as_deref()?;
        let Some(template) = state.title_format.as_deref() else {
            return Some(title.to_owned());
        };
        Some(template.replace("%s", title))
    }

    #[track_caller]
    fn reject(&mut self, error: MetadataError) -> MetadataError {
        self.rejections = self.rejections.saturating_add(1);
        static DIAGNOSTICS: AtomicUsize = AtomicUsize::new(0);
        let count = DIAGNOSTICS.fetch_add(1, Ordering::Relaxed);
        if cfg!(debug_assertions) || count < 8 || count.is_power_of_two() {
            let caller = std::panic::Location::caller();
            tracing::warn!(
                error = %error,
                file = caller.file(),
                line = caller.line(),
                "managed metadata operation rejected"
            );
        }
        error
    }
    #[track_caller]
    pub(super) fn declare(&mut self, patch: Metadata) -> Result<(), MetadataError> {
        if self.denied {
            return Err(self.reject(MetadataError::Ineligible));
        }
        if self.closed {
            return Err(self.reject(MetadataError::Closed));
        }
        if let Err(error) = patch.validate() {
            return Err(self.reject(error));
        }
        // Validate the complete patch before mutating any field.
        // An empty declaration does not allocate.
        let Metadata {
            title,
            title_template,
            absolute_title,
            description,
            canonical,
            open_graph,
        } = patch;
        let OpenGraph {
            enabled,
            title: og_title,
            description: og_description,
            site_name,
            determiner,
            ty,
            url,
            locale,
            images,
        } = open_graph;
        if self.state.is_none()
            && !enabled
            && matches!(
                (
                    &title,
                    &title_template,
                    &description,
                    &canonical,
                    &og_title,
                    &og_description,
                    &site_name,
                    &determiner,
                    &ty,
                    &url,
                    &locale,
                    &images
                ),
                (
                    Field::Inherit,
                    Field::Inherit,
                    Field::Inherit,
                    Field::Inherit,
                    Field::Inherit,
                    Field::Inherit,
                    Field::Inherit,
                    Field::Inherit,
                    Field::Inherit,
                    Field::Inherit,
                    Field::Inherit,
                    Field::Inherit
                )
            )
        {
            return Ok(());
        }
        // proa-perf-ok: lazy per-document typed state; no per-component allocation
        let state = self.state.get_or_insert_with(|| Box::new(State::default()));
        if !matches!(title, Field::Inherit) {
            title.apply(&mut state.title);
            state.title_format = if absolute_title {
                None
            } else {
                state.inherited_template.clone()
            };
        }
        title_template.apply(&mut state.descendant_template);
        description.apply(&mut state.description);
        canonical.apply(&mut state.canonical);
        state.open_graph |= enabled;
        state.og_title.override_with(og_title);
        state.og_description.override_with(og_description);
        site_name.apply(&mut state.og_site_name);
        determiner.apply(&mut state.og_determiner);
        ty.apply(&mut state.og_ty);
        state.og_url.override_with(url);
        locale.apply(&mut state.og_locale);
        images.apply(&mut state.images);
        Ok(())
    }

    /// Framework integration only: begin the next registered route scope.
    /// Ordinary components and scoped writers never call this.
    #[doc(hidden)]
    pub(super) fn next_route_scope(&mut self) -> Result<(), MetadataError> {
        if self.denied {
            return Err(self.reject(MetadataError::Ineligible));
        }
        if self.closed {
            return Err(self.reject(MetadataError::Closed));
        }
        if let Some(state) = self.state.as_mut() {
            state.inherited_template = state.descendant_template.clone();
        }
        Ok(())
    }

    pub(crate) fn emit<B: WriteBuf>(
        &mut self,
        out: &mut B,
        dialect: EscapeDialect,
    ) -> Result<(), WriteError> {
        if self.denied {
            return Err(self.reject(MetadataError::Ineligible).into());
        }
        if self.closed {
            return Err(self.reject(MetadataError::DuplicateHead).into());
        }
        self.closed = true;
        if self.title_only {
            return Ok(());
        }
        let Some(state) = self.state.as_ref() else {
            return Ok(());
        };
        if let Some(title) = &state.title {
            out.extend_static(b"<title>")?;
            write_title(out, title, state.title_format.as_deref(), false, dialect)?;
            out.extend_static(b"</title>")?;
        }
        if let Some(description) = &state.description {
            meta(
                out,
                dialect,
                b"<meta name=\"description\" content=\"",
                description,
            )?;
        }
        if let Some(canonical) = &state.canonical {
            out.extend_static(b"<link rel=\"canonical\" href=\"")?;
            escape_attr_value_for_dialect(out, canonical, dialect)?;
            out.extend_static(b"\"/>")?;
        }
        if !state.open_graph {
            return Ok(());
        }
        match &state.og_title {
            Field::Set(value) => meta(
                out,
                dialect,
                b"<meta property=\"og:title\" content=\"",
                value,
            )?,
            Field::Clear => {}
            Field::Inherit => {
                if let Some(title) = &state.title {
                    out.extend_static(b"<meta property=\"og:title\" content=\"")?;
                    write_title(out, title, state.title_format.as_deref(), true, dialect)?;
                    out.extend_static(b"\"/>")?;
                }
            }
        }
        if let Some(value) = state
            .og_description
            .or_fallback(state.description.as_deref())
        {
            meta(
                out,
                dialect,
                b"<meta property=\"og:description\" content=\"",
                value,
            )?;
        }
        if let Some(value) = &state.og_site_name {
            meta(
                out,
                dialect,
                b"<meta property=\"og:site_name\" content=\"",
                value,
            )?;
        }
        if let Some(value) = &state.og_determiner {
            meta(
                out,
                dialect,
                b"<meta property=\"og:determiner\" content=\"",
                value,
            )?;
        }
        if let Some(value) = &state.og_ty {
            meta(
                out,
                dialect,
                b"<meta property=\"og:type\" content=\"",
                value,
            )?;
        }
        if let Some(value) = state.og_url.or_fallback(state.canonical.as_deref()) {
            meta(out, dialect, b"<meta property=\"og:url\" content=\"", value)?;
        }
        if let Some(value) = &state.og_locale {
            meta(
                out,
                dialect,
                b"<meta property=\"og:locale\" content=\"",
                value,
            )?;
        }
        for image in state.images.iter().flatten() {
            meta(
                out,
                dialect,
                b"<meta property=\"og:image\" content=\"",
                &image.url,
            )?;
            if let Some(value) = &image.secure_url {
                meta(
                    out,
                    dialect,
                    b"<meta property=\"og:image:secure_url\" content=\"",
                    value,
                )?;
            }
            if let Some(value) = &image.mime_type {
                meta(
                    out,
                    dialect,
                    b"<meta property=\"og:image:type\" content=\"",
                    value,
                )?;
            }
            if let Some(value) = image.width {
                number(out, b"<meta property=\"og:image:width\" content=\"", value)?;
            }
            if let Some(value) = image.height {
                number(out, b"<meta property=\"og:image:height\" content=\"", value)?;
            }
            if let Some(value) = &image.alt {
                meta(
                    out,
                    dialect,
                    b"<meta property=\"og:image:alt\" content=\"",
                    value,
                )?;
            }
        }
        Ok(())
    }
}

fn meta<B: WriteBuf>(
    out: &mut B,
    dialect: EscapeDialect,
    prefix: &'static [u8],
    value: &str,
) -> Result<(), WriteError> {
    out.extend_static(prefix)?;
    escape_attr_value_for_dialect(out, value, dialect)?;
    out.extend_static(b"\"/>")
}
fn number<B: WriteBuf>(out: &mut B, prefix: &'static [u8], value: u32) -> Result<(), WriteError> {
    out.extend_static(prefix)?;
    out.extend_with(10, |buf| {
        itoa_lut::write_u32(value, buf.try_into().unwrap())
    })?;
    out.extend_static(b"\"/>")
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct Head;
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Head {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let dialect = cx.__proa_current_escape_dialect();
        cx.metadata_state.emit(&mut cx.out, dialect)
    }
}
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for Head {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        let dialect = cx.__proa_current_escape_dialect();
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            cx.metadata_state.emit(&mut cx.out, dialect),
        )
    }
}
impl<L: DataLoader, B: WriteBuf + Send> WebRenderSendSync<L, B> for Head {
    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        let dialect = cx.__proa_current_escape_dialect();
        cx.metadata_state.emit(&mut cx.out, dialect)
    }
}
impl<L: DataLoader, B: WriteBuf + Send> WebRenderSend<L, B> for Head {
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>> + Send> {
        let dialect = cx.__proa_current_escape_dialect();
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            cx.metadata_state.emit(&mut cx.out, dialect),
        )
    }
}

pub(super) trait MetadataAccess {
    fn document_metadata(&mut self) -> &mut MetadataState;
}
impl<L: DataLoader, B: WriteBuf> MetadataAccess for WebContext<L, B> {
    fn document_metadata(&mut self) -> &mut MetadataState {
        &mut self.metadata_state
    }
}
impl<L: DataLoader, B: WriteBuf + Send> MetadataAccess for SendWebContext<L, B> {
    fn document_metadata(&mut self) -> &mut MetadataState {
        &mut self.metadata_state
    }
}
struct DeniedScope<'a, C: MetadataAccess> {
    cx: &'a mut C,
    saved_denied: bool,
}
impl<C: MetadataAccess> Drop for DeniedScope<'_, C> {
    fn drop(&mut self) {
        self.cx.document_metadata().denied = self.saved_denied;
    }
}
pub(super) fn without_metadata<C: MetadataAccess, R>(
    cx: &mut C,
    render: impl FnOnce(&mut C) -> R,
) -> R {
    let saved_denied = std::mem::replace(&mut cx.document_metadata().denied, true);
    let scope = DeniedScope { cx, saved_denied };
    render(scope.cx)
}
pub(super) async fn without_metadata_async<C: MetadataAccess, R>(
    cx: &mut C,
    render: impl AsyncFnOnce(&mut C) -> R,
) -> R {
    let saved_denied = std::mem::replace(&mut cx.document_metadata().denied, true);
    let scope = DeniedScope { cx, saved_denied };
    render(scope.cx).await
}

impl<T> Field<T> {
    fn override_with(&mut self, incoming: Self) {
        if !matches!(incoming, Self::Inherit) {
            *self = incoming;
        }
    }
}
impl Field<Text> {
    fn or_fallback<'a>(&'a self, fallback: Option<&'a str>) -> Option<&'a str> {
        match self {
            Self::Inherit => fallback,
            Self::Set(value) => Some(value),
            Self::Clear => None,
        }
    }
}
fn write_title<B: WriteBuf>(
    out: &mut B,
    title: &str,
    template: Option<&str>,
    attribute: bool,
    dialect: EscapeDialect,
) -> Result<(), WriteError> {
    let mut write = |value: &str| {
        if attribute {
            escape_attr_value_for_dialect(out, value, dialect)
        } else {
            escape_text_for_dialect(out, value, ContentMode::Html, dialect)
        }
    };
    if let Some((before, after)) = template.and_then(|value| value.split_once("%s")) {
        write(before)?;
        write(title)?;
        write(after)
    } else {
        write(title)
    }
}
impl Metadata {
    fn validate(&self) -> Result<(), MetadataError> {
        if let Field::Set(template) = &self.title_template {
            if template.matches("%s").count() != 1 {
                return Err(MetadataError::InvalidTitleTemplate);
            }
        }
        for (field, value) in [
            ("canonical", &self.canonical),
            ("og:url", &self.open_graph.url),
        ] {
            if let Field::Set(url) = value {
                validate_url(url, field)?;
            }
        }
        if let Field::Set(images) = &self.open_graph.images {
            for image in images {
                validate_url(&image.url, "og:image")?;
                if let Some(url) = &image.secure_url {
                    validate_url(url, "og:image:secure_url")?;
                }
            }
        }
        Ok(())
    }
}
// A narrow allocation-free URL policy, without fetching, normalization, or
// implicit origin resolution. Public metadata never accepts credentials.
fn validate_url(url: &str, field: &'static str) -> Result<(), MetadataError> {
    let valid = || -> Option<()> {
        let (scheme, rest) = url.split_once("://")?;
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return None;
        }
        if url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '\\' | '"' | '<' | '>'))
        {
            return None;
        }
        let authority = rest.split(['/', '?', '#']).next()?;
        if authority.is_empty() || authority.contains('@') {
            return None;
        }
        let (host, port) = if let Some(ipv6) = authority.strip_prefix('[') {
            let (address, tail) = ipv6.split_once(']')?;
            address.parse::<std::net::Ipv6Addr>().ok()?;
            (
                address,
                if tail.is_empty() {
                    None
                } else {
                    Some(tail.strip_prefix(':')?)
                },
            )
        } else {
            let (host, port) = authority
                .split_once(':')
                .map_or((authority, None), |(h, p)| (h, Some(p)));
            if host.is_empty()
                || !host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
            {
                return None;
            }
            (host, port)
        };
        if host.is_empty() {
            return None;
        }
        if let Some(port) = port {
            if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            port.parse::<u16>().ok()?;
        }
        Some(())
    };
    valid().ok_or(MetadataError::InvalidUrl { field })
}
