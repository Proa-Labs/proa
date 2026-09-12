// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{ctx::Ctx, WebRenderSync, WriteError};

/// Server-rendered content to be placed inside a client island (donut pattern).
///
/// `Children` wraps renderable content that gets serialized as HTML inside
/// the island's container div, rather than being passed as JSON props.
///
/// # Example
///
/// ```ignore
/// AccordionProps {
///     title: "FAQ".into(),
///     children: html! { <p>"Server content"</p> }.into(),
/// }
/// ```
type ChildrenRenderer = Box<dyn FnOnce(&mut Ctx) -> Result<(), WriteError> + Send>;

#[derive(Default)]
pub struct Children(Option<ChildrenRenderer>);

impl<T: WebRenderSync + Send + 'static> From<T> for Children {
    fn from(renderable: T) -> Self {
        Self(Some(Box::new(move |cx: &mut Ctx| renderable.render(cx))))
    }
}

impl Children {
    /// Render the children content into the context.
    /// If no children were provided, this is a no-op.
    pub fn render(self, cx: &mut Ctx) -> Result<(), WriteError> {
        if let Some(f) = self.0 {
            f(cx)?;
        }
        Ok(())
    }

    /// Returns true if no children content was provided.
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }
}
