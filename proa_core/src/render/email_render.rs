// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Email rendering trait.
//!
//! Email HTML is a different rendering target than web HTML: inline
//! styles, table layouts, no JavaScript. Components that target email
//! implement [`EmailRender`] separately from [`super::web_render::WebRender`].

use crate::email::EmailContext;

/// Trait for components that can render to HTML for email clients.
pub trait EmailRender {
    fn render(self, cx: &mut EmailContext<'_>);
}
