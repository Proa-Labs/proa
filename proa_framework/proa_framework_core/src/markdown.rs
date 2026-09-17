// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Buffered Markdown responses share request setup with HTML while preserving
//! the page allocations and omitting HTML bootstrap and document finalization.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use http::{header, HeaderValue};
use proa_core::{DataLoader, MdRender, MdRenderSync, NoLoader, WebContext, WriteBuf, WriteError};

use crate::response::{install_request_context, merge_response_context};
use crate::{FrameworkBody, FrameworkRequestContext, FrameworkResponse};

trait DynMdRender<L: DataLoader> {
    fn render<'cx>(
        self: Box<Self>,
        cx: &'cx mut WebContext<L>,
    ) -> Pin<Box<dyn Future<Output = Result<(), WriteError>> + 'cx>>;
}

impl<L: DataLoader + 'static, M: MdRender<L> + 'static> DynMdRender<L> for M {
    fn render<'cx>(
        self: Box<Self>,
        cx: &'cx mut WebContext<L>,
    ) -> Pin<Box<dyn Future<Output = Result<(), WriteError>> + 'cx>> {
        Box::pin(async move { (*self).render_md(cx).resolve().await })
    }
}

/// An unopened Markdown render plan that can cross a handler's `Send` boundary.
///
/// Only the root and loader are transferred. Finalization creates the context
/// and drives its local future on the host's render executor, just like HTML.
pub struct MarkdownResponse<L: DataLoader = NoLoader> {
    component: Box<dyn DynMdRender<L> + Send>,
    loader: Arc<L>,
}

impl<L: DataLoader + 'static> MarkdownResponse<L> {
    pub fn with_loader<M: MdRender<L> + Send + 'static>(component: M, loader: Arc<L>) -> Self {
        Self {
            component: Box::new(component),
            loader,
        }
    }

    /// Render locally with the same request metadata used by HTML finalization.
    /// No `Send` requirement is imposed on the active component future.
    pub async fn finalize(
        self,
        request: &FrameworkRequestContext,
    ) -> Result<FrameworkResponse, WriteError> {
        let mut cx = WebContext::with_loader(self.loader).with_render_mode(request.render_mode);
        install_request_context(&mut cx, request);
        self.component.render(&mut cx).await?;
        Ok(finish_markdown(cx))
    }
}

/// Render a synchronous Markdown root directly into the default owned buffer.
pub fn render_sync_markdown_response<M: MdRenderSync>(
    component: M,
    request: &FrameworkRequestContext,
) -> Result<FrameworkResponse, WriteError> {
    let mut cx = WebContext::new().with_render_mode(request.render_mode);
    install_request_context(&mut cx, request);
    component.render_md(&mut cx)?;
    Ok(finish_markdown(cx))
}

fn finish_markdown<L: DataLoader>(cx: WebContext<L>) -> FrameworkResponse {
    let len = cx.out.len();
    let mut response = FrameworkResponse::plain_text(http::StatusCode::OK, Bytes::new());
    merge_response_context(&mut response, cx.response());
    let out = cx.into_output();
    response.body = if out.page_count() == 1 {
        FrameworkBody::Full(Bytes::from(out.into_vec()))
    } else {
        FrameworkBody::BufferedPages(out)
    };
    response.headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    response
        .headers
        .insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    response
}

use http::header::CONTENT_TYPE;
