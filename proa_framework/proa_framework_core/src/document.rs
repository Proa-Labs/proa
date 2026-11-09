// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Standalone HTML document rendering without an HTTP adapter.

use bytes::Bytes;
use proa_core::{HeadConfig, WebContext, WebRenderSync};
use proa_react::ReactBootstrapConfig;

use crate::managed_response::BufferedHead;
use crate::response::{finish_sync_html_response, install_request_context};
use crate::{FrameworkBody, FrameworkError, FrameworkRequestContext, IslandManifest};

#[derive(Clone, Copy)]
enum DocumentState {
    Ready,
    Rendered,
    Failed,
}

/// A local synchronous context for one complete managed-head HTML document.
///
/// Create the context, call [`render`](Self::render) with your composed layout
/// and Page, then consume it with [`finish`](Self::finish). Rendering runs
/// immediately and accepts borrowed or non-`Send` components. The context owns
/// metadata preparation and output finalization; no server, async runtime, or
/// application-owned response machinery is required.
///
/// The document needs one `ComponentRole::PAGE` and one `cx.head()` outlet.
/// External scripts and generated RSJS assets remain references, not embedded
/// files. Documents requiring a React island manifest or request-specific
/// configuration should use the route pipeline instead.
#[must_use = "render a document and finish the context to obtain its HTML"]
pub struct HtmlDocumentContext {
    context: WebContext,
    request: FrameworkRequestContext,
    head_buffer: Option<BufferedHead>,
    state: DocumentState,
}

impl Default for HtmlDocumentContext {
    fn default() -> Self {
        Self::with_head(HeadConfig::default())
    }
}

impl HtmlDocumentContext {
    /// Create a document context with the default managed-head configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a context with inherited metadata defaults and trusted URL policy.
    pub fn with_head(head: HeadConfig) -> Self {
        Self {
            context: WebContext::new(),
            request: FrameworkRequestContext {
                head: Some(head.into()),
                ..Default::default()
            },
            head_buffer: None,
            state: DocumentState::Ready,
        }
    }

    /// Render one complete document, including its layout and Page head hooks.
    ///
    /// This is the root lifecycle entry point, unlike calling the component's
    /// `WebRenderSync::render` body directly. A second render or a failed render
    /// invalidates the context; [`finish`](Self::finish) cannot return partial HTML.
    pub fn render(&mut self, document: impl WebRenderSync) -> Result<(), FrameworkError> {
        let previous = std::mem::replace(&mut self.state, DocumentState::Failed);
        if !matches!(previous, DocumentState::Ready) {
            return Err(FrameworkError::Invariant(
                "an HTML document context can render only one document",
            ));
        }
        install_request_context(&mut self.context, &self.request);
        self.head_buffer = crate::managed_response::setup_local(&mut self.context, &self.request)?;
        proa_core::head_dispatch::sync(document, &mut self.context)?;
        self.context.require_managed_head()?.final_check()?;
        self.state = DocumentState::Rendered;
        Ok(())
    }

    /// Finish the document and return UTF-8 bytes accepted by [`std::fs::write`].
    ///
    /// Finalization validates the complete output and appends any generated
    /// RSJS bootstrap before the closing body tag. It performs no file or network
    /// I/O and never returns output from an absent or failed render.
    pub fn finish(self) -> Result<Bytes, FrameworkError> {
        if !matches!(self.state, DocumentState::Rendered) {
            return Err(FrameworkError::Invariant(
                "an HTML document must render successfully before finish",
            ));
        }
        let manifest = IslandManifest {
            islands: Default::default(),
            assets: Default::default(),
            import_map: Default::default(),
            ssr_bundle: None,
        };
        let response = finish_sync_html_response(
            self.context,
            self.head_buffer,
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &self.request,
        )?;
        match response.body {
            FrameworkBody::Full(bytes) => Ok(bytes),
            FrameworkBody::BufferedPages(buffer) => Ok(Bytes::from(buffer.into_vec())),
            FrameworkBody::Managed(_) | FrameworkBody::Stream(_) => Err(FrameworkError::Invariant(
                "synchronous HTML document produced a streaming body",
            )),
        }
    }
}

/// Render a synchronous managed-head document to UTF-8 HTML bytes in one call.
///
/// This convenience wrapper uses [`HtmlDocumentContext`]. Use the context
/// directly for the create/render/finish flow. Both APIs accept borrowed or
/// non-`Send` roots and preserve the same metadata and bootstrap lifecycle.
pub fn render_sync_html_document(
    component: impl WebRenderSync,
    head: HeadConfig,
) -> Result<Bytes, FrameworkError> {
    let mut cx = HtmlDocumentContext::with_head(head);
    cx.render(component)?;
    cx.finish()
}
