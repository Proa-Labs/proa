// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Standalone HTML document rendering on the existing WebContext.

use std::sync::Mutex;

use bytes::Bytes;
use proa_core::{HeadConfig, WebContext, WebRenderSync, WriteBuf};
use proa_react::ReactBootstrapConfig;

use crate::managed_response::BufferedHead;
use crate::response::finish_sync_html_response;
use crate::{FrameworkBody, FrameworkError, FrameworkRequestContext, IslandManifest};

// One private root-lifecycle entry in the context's existing extension storage.
// No additional context type or state is imposed on ordinary fragment renders.
struct DocumentSession(Mutex<DocumentState>);

enum DocumentState {
    Ready(HeadConfig),
    Rendering,
    Rendered {
        request: FrameworkRequestContext,
        head_buffer: Option<BufferedHead>,
    },
    Failed,
}

/// Complete-document rendering methods for the existing [`WebContext`].
///
/// Import this trait to use `cx.render(document)?` followed by `cx.finish()?`.
/// The root render runs immediately, including layout/Page metadata hooks, and
/// accepts borrowed or non-`Send` components. Ordinary component-body rendering
/// through `WebRenderSync::render` keeps its existing fragment semantics.
///
/// A document needs one `ComponentRole::PAGE` and one `cx.head()` outlet.
/// External scripts and generated RSJS assets remain references, not embedded
/// files. Use the route pipeline for documents requiring a React island
/// manifest or request-specific configuration.
pub trait WebContextDocumentExt: Sized {
    /// Optionally set metadata defaults and URL policy before rendering.
    fn set_document_head(&mut self, head: HeadConfig) -> Result<(), FrameworkError>;

    /// Render one complete document with default or explicitly configured head.
    ///
    /// The context must be in static mode with no prior output or managed-head
    /// driver. A failed or duplicate render invalidates it; `finish` cannot
    /// return partial HTML.
    fn render(&mut self, document: impl WebRenderSync) -> Result<(), FrameworkError>;

    /// Consume a successfully rendered context and return finalized UTF-8 HTML.
    ///
    /// Finalization appends generated RSJS bootstrap before the closing body
    /// tag. The bytes are accepted by [`std::fs::write`]. This performs no file
    /// or network I/O. Forked contexts cannot finish another context's document.
    fn finish(self) -> Result<Bytes, FrameworkError>;
}

impl WebContextDocumentExt for WebContext {
    fn set_document_head(&mut self, head: HeadConfig) -> Result<(), FrameworkError> {
        if let Some(session) = self.locals().get::<DocumentSession>() {
            let mut state = session.0.lock().unwrap();
            if matches!(*state, DocumentState::Ready(_)) {
                *state = DocumentState::Ready(head);
                return Ok(());
            }
            *state = DocumentState::Failed;
            return Err(FrameworkError::Invariant(
                "document configuration must precede rendering",
            ));
        }
        self.locals_mut()
            .insert(DocumentSession(Mutex::new(DocumentState::Ready(head))));
        Ok(())
    }

    fn render(&mut self, document: impl WebRenderSync) -> Result<(), FrameworkError> {
        if self.locals().get::<DocumentSession>().is_none() {
            self.set_document_head(HeadConfig::default())?;
        }
        let session = self
            .locals()
            .get::<DocumentSession>()
            .ok_or(FrameworkError::Invariant(
                "document lifecycle was not initialized",
            ))?;
        let head = {
            let mut state = session.0.lock().unwrap();
            match std::mem::replace(&mut *state, DocumentState::Failed) {
                DocumentState::Ready(head) => {
                    *state = DocumentState::Rendering;
                    head
                }
                _ => {
                    return Err(FrameworkError::Invariant(
                        "a WebContext can render only one complete document",
                    ))
                }
            }
        };
        let result = (|| {
            if self.render_mode() != proa_core::RenderMode::Static
                || !self.output().is_empty()
                || self.head_state().document().is_some()
            {
                return Err(FrameworkError::Invariant(
                    "document rendering requires static mode, fresh output, and no managed-head driver",
                ));
            }
            let request = FrameworkRequestContext {
                request: self.request().clone(),
                csp_nonce: self.csp_nonce().cloned(),
                head: Some(head.into()),
                ..Default::default()
            };
            let head_buffer = crate::managed_response::setup_local(self, &request)?;
            proa_core::head_dispatch::sync(document, self)?;
            self.require_managed_head()?.final_check()?;
            Ok(DocumentState::Rendered {
                request,
                head_buffer,
            })
        })();
        let mut state = session.0.lock().unwrap();
        match result {
            Ok(rendered) if matches!(*state, DocumentState::Rendering) => {
                *state = rendered;
                Ok(())
            }
            Ok(_) => Err(FrameworkError::Invariant("document render was invalidated")),
            Err(error) => {
                *state = DocumentState::Failed;
                Err(error)
            }
        }
    }

    fn finish(mut self) -> Result<Bytes, FrameworkError> {
        let session =
            self.locals_mut()
                .remove::<DocumentSession>()
                .ok_or(FrameworkError::Invariant(
                    "a document must render successfully before finish",
                ))?;
        let session = match std::sync::Arc::try_unwrap(session) {
            Ok(session) => session,
            Err(shared) => {
                *shared.0.lock().unwrap() = DocumentState::Failed;
                return Err(FrameworkError::Invariant(
                    "a forked WebContext cannot finish a shared document lifecycle",
                ));
            }
        };
        let DocumentState::Rendered {
            request,
            head_buffer,
        } = session.0.into_inner().unwrap()
        else {
            return Err(FrameworkError::Invariant(
                "a document must render successfully before finish",
            ));
        };
        let manifest = IslandManifest {
            islands: Default::default(),
            assets: Default::default(),
            import_map: Default::default(),
            ssr_bundle: None,
        };
        let response = finish_sync_html_response(
            self,
            head_buffer,
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &request,
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
/// This convenience function uses [`WebContextDocumentExt`] on [`WebContext`].
/// Both entry points accept borrowed or non-`Send` roots and preserve metadata
/// and generated bootstrap output.
pub fn render_sync_html_document(
    component: impl WebRenderSync,
    head: HeadConfig,
) -> Result<Bytes, FrameworkError> {
    let mut cx = WebContext::new();
    cx.set_document_head(head)?;
    cx.render(component)?;
    cx.finish()
}
