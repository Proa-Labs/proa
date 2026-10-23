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

/// Structured Open Graph declaration. Fields merge individually; images replace
/// the preceding list when at least one image is supplied.
#[derive(Clone, Debug, Default)]
pub struct OpenGraph {
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
    title: Option<Text>,
    title_format: Option<Text>,
    inherited_template: Option<Text>,
    descendant_template: Option<Text>,
    description: Option<Text>,
    canonical: Option<Text>,
    og_title: Option<Text>,
    og_description: Option<Text>,
    og_site_name: Option<Text>,
    og_determiner: Option<Text>,
    og_ty: Option<Text>,
    og_url: Option<Text>,
    og_locale: Option<Text>,
    images: Option<Vec<OpenGraphImage>>,
}

/// Request-local managed metadata. Obtain a borrow-free render handle with
/// `head()`; declare values through `WebContext::metadata`.
/// No registry is allocated until the first nonempty declaration.
#[derive(Debug, Default)]
pub struct DocumentMetadata {
    state: Option<Box<State>>,
    closed: bool,
    denied: bool,
    rejections: u32,
}

impl DocumentMetadata {
    /// Obtaining this handle does not snapshot or close metadata. Rendering it
    /// reads the active context and emits immediately into its current writer.
    pub fn head(&self) -> Head {
        // A concrete, hidden handle supports every loader/writer combination.
        Head
    }

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

    #[track_caller]
    fn reject(&mut self, reason: &'static str) {
        self.rejections = self.rejections.saturating_add(1);
        static DIAGNOSTICS: AtomicUsize = AtomicUsize::new(0);
        let count = DIAGNOSTICS.fetch_add(1, Ordering::Relaxed);
        if cfg!(debug_assertions) || count < 8 || count.is_power_of_two() {
            let caller = std::panic::Location::caller();
            tracing::warn!(
                reason,
                file = caller.file(),
                line = caller.line(),
                "managed metadata operation rejected"
            );
        }
    }
    #[track_caller]
    pub(crate) fn declare(&mut self, patch: Metadata) {
        if self.denied {
            self.reject("deferred branches and fallbacks cannot change document metadata");
            return;
        }
        if self.closed {
            self.reject("metadata was already emitted; declare before rendering the head");
            return;
        }
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
            return;
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
        og_title.apply(&mut state.og_title);
        og_description.apply(&mut state.og_description);
        site_name.apply(&mut state.og_site_name);
        determiner.apply(&mut state.og_determiner);
        ty.apply(&mut state.og_ty);
        url.apply(&mut state.og_url);
        locale.apply(&mut state.og_locale);
        images.apply(&mut state.images);
    }

    /// Framework integration only: begin the next registered route scope.
    /// Ordinary components and scoped writers never call this.
    #[doc(hidden)]
    pub fn __next_route_scope(&mut self) {
        if let Some(state) = self.state.as_mut() {
            state.inherited_template = state.descendant_template.clone();
        }
    }

    pub(crate) fn emit<B: WriteBuf>(
        &mut self,
        out: &mut B,
        dialect: EscapeDialect,
    ) -> Result<(), WriteError> {
        if self.denied {
            self.reject("deferred branches and fallbacks cannot emit document metadata");
            return Ok(());
        }
        if self.closed {
            self.reject("managed metadata head was already emitted");
            return Ok(());
        }
        self.closed = true;
        let Some(state) = self.state.as_ref() else {
            return Ok(());
        };
        if let Some(title) = &state.title {
            out.extend_static(b"<title>")?;
            if let Some(template) = &state.title_format {
                let mut parts = template.split("%s").peekable();
                while let Some(part) = parts.next() {
                    escape_text_for_dialect(out, part, ContentMode::Html, dialect)?;
                    if parts.peek().is_some() {
                        escape_text_for_dialect(out, title, ContentMode::Html, dialect)?;
                    }
                }
            } else {
                escape_text_for_dialect(out, title, ContentMode::Html, dialect)?;
            }
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
        if let Some(value) = &state.og_title {
            meta(
                out,
                dialect,
                b"<meta property=\"og:title\" content=\"",
                value,
            )?;
        }
        if let Some(value) = &state.og_description {
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
        if let Some(value) = &state.og_url {
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
                number(
                    out,
                    dialect,
                    b"<meta property=\"og:image:width\" content=\"",
                    value,
                )?;
            }
            if let Some(value) = image.height {
                number(
                    out,
                    dialect,
                    b"<meta property=\"og:image:height\" content=\"",
                    value,
                )?;
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
fn number<B: WriteBuf>(
    out: &mut B,
    _dialect: EscapeDialect,
    prefix: &'static [u8],
    value: u32,
) -> Result<(), WriteError> {
    let mut digits = [0u8; 10];
    let mut index = digits.len();
    let mut remaining = value;
    loop {
        index -= 1;
        digits[index] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    out.extend_static(prefix)?;
    out.extend(&digits[index..])?;
    out.extend_static(b"\"/>")
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct Head;
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Head {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let dialect = cx.__proa_current_escape_dialect();
        cx.metadata.emit(&mut cx.out, dialect)
    }
}
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for Head {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        let dialect = cx.__proa_current_escape_dialect();
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            cx.metadata.emit(&mut cx.out, dialect),
        )
    }
}
impl<L: DataLoader, B: WriteBuf + Send> WebRenderSendSync<L, B> for Head {
    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        let dialect = cx.__proa_current_escape_dialect();
        cx.metadata.emit(&mut cx.out, dialect)
    }
}
impl<L: DataLoader, B: WriteBuf + Send> WebRenderSend<L, B> for Head {
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>> + Send> {
        let dialect = cx.__proa_current_escape_dialect();
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            cx.metadata.emit(&mut cx.out, dialect),
        )
    }
}

pub(super) trait MetadataAccess {
    fn document_metadata(&mut self) -> &mut DocumentMetadata;
}
impl<L: DataLoader, B: WriteBuf> MetadataAccess for WebContext<L, B> {
    fn document_metadata(&mut self) -> &mut DocumentMetadata {
        &mut self.metadata
    }
}
impl<L: DataLoader, B: WriteBuf + Send> MetadataAccess for SendWebContext<L, B> {
    fn document_metadata(&mut self) -> &mut DocumentMetadata {
        &mut self.metadata
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
