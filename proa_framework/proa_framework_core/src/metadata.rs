// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Registered route setup. Metadata is declared and emitted by the live render
//! context, never resolved from a registration-time provider snapshot.

use crate::manifest::IslandManifest;
use proa_core::{
    DataLoader, DynLoaderHandle, SendWebContext, WebContext, WebRenderSendSync, WebRenderSync,
    WriteBuf, WriteError,
};
pub use proa_core::{Metadata, OpenGraph, OpenGraphImage};
use std::sync::Arc;

pub type LayoutRef = &'static dyn Layout;

/// Request-time defaults for a registered route layout. The framework runs
/// setup outermost to innermost before the page, using its live context.
/// Ordinary HTML wrappers do not establish a route scope.
///
/// Setup is synchronous; pages can await data on their normal render path
/// before declaring their own metadata and emitting the head.
pub trait Layout: Send + Sync + 'static {
    fn setup(&self, cx: &mut WebContext<DynLoaderHandle>) -> Result<(), WriteError>;

    /// Implement this callback when using a compiler-proved Send response.
    /// The default reports unsupported setup rather than silently dropping it.
    fn setup_send(&self, _cx: &mut SendWebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        Err(WriteError::Render(
            "this layout must implement setup_send for Send responses".into(),
        ))
    }
}

/// Ordered registered setup callbacks, collected without executing them.
/// Runtime metadata can only be discovered by rendering with a real request.
#[derive(Clone, Default)]
pub struct RouteSetupPlan {
    layouts: Option<Arc<[LayoutRef]>>,
    page_title: Option<&'static str>,
}
impl RouteSetupPlan {
    pub fn cascade(parent: &Self, layouts: &[LayoutRef]) -> Self {
        if layouts.is_empty() {
            return parent.clone();
        }
        let mut chain = parent.layouts.as_deref().unwrap_or_default().to_vec();
        chain.extend_from_slice(layouts);
        Self {
            layouts: Some(chain.into()),
            page_title: None,
        }
    }
    pub fn new(layouts: &[LayoutRef], page_title: Option<&'static str>) -> Self {
        Self {
            layouts: (!layouts.is_empty()).then(|| layouts.into()),
            page_title,
        }
    }
    pub fn with_page_title(mut self, title: Option<&'static str>) -> Self {
        self.page_title = title;
        self
    }
    pub fn is_empty(&self) -> bool {
        self.layouts.is_none() && self.page_title.is_none()
    }
    pub fn layout_count(&self) -> usize {
        self.layouts.as_deref().map_or(0, <[_]>::len)
    }
    pub fn page_title(&self) -> Option<&'static str> {
        self.page_title
    }
}

pub(crate) fn prepare_layouts<L: DataLoader + 'static>(
    cx: &mut WebContext<L>,
) -> Result<(), WriteError> {
    let Some(plan) = cx.request().extensions().get::<RouteSetupPlan>().cloned() else {
        return Ok(());
    };
    if plan.layouts.is_none() {
        if let Some(title) = plan.page_title {
            cx.metadata(Metadata::new().title(title));
        }
        return Ok(());
    }
    cx.__with_erased_loader(|cx| {
        for layout in plan.layouts.as_deref().unwrap_or_default() {
            layout.setup(cx)?;
            cx.metadata.__next_route_scope();
        }
        if let Some(title) = plan.page_title {
            cx.metadata(Metadata::new().title(title));
        }
        Ok(())
    })
}
pub(crate) fn prepare_layouts_send<L: DataLoader + 'static>(
    cx: &mut SendWebContext<L>,
) -> Result<(), WriteError> {
    let Some(plan) = cx.request().extensions().get::<RouteSetupPlan>().cloned() else {
        return Ok(());
    };
    if plan.layouts.is_none() {
        if let Some(title) = plan.page_title {
            cx.metadata(Metadata::new().title(title));
        }
        return Ok(());
    }
    cx.__with_erased_loader(|cx| {
        for layout in plan.layouts.as_deref().unwrap_or_default() {
            layout.setup_send(cx)?;
            cx.metadata.__next_route_scope();
        }
        if let Some(title) = plan.page_title {
            cx.metadata(Metadata::new().title(title));
        }
        Ok(())
    })
}

pub fn css_asset_path(manifest: &IslandManifest) -> Option<&str> {
    manifest.assets.css.as_ref().map(|css| css.path.as_ref())
}
pub fn render_css_link<L: DataLoader, B: WriteBuf>(cx: &mut WebContext<L, B>, path: &str) {
    use proa_core::text::StaticRaw;
    StaticRaw("<link rel=\"stylesheet\" href=\"")
        .render(cx)
        .unwrap();
    let dialect = cx.__proa_current_escape_dialect();
    proa_core::shared::escape::escape_attr_value_for_dialect(&mut cx.out, path, dialect).unwrap();
    StaticRaw("\"/>").render(cx).unwrap();
}
/// Immediate compatibility emitter; using it and `head()` counts as two emissions.
pub fn render_metadata_head<L: DataLoader, B: WriteBuf>(
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    cx.metadata.head().render(cx)
}
pub fn render_metadata_head_send<L: DataLoader, B: WriteBuf + Send>(
    cx: &mut SendWebContext<L, B>,
) -> Result<(), WriteError> {
    cx.metadata.head().render_send_sync(cx)
}
