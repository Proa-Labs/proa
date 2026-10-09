// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

/// Compiler-owned string argument for `text!` conditional branches.
/// Borrowed signal text stays borrowed; authored string transforms stay owned.
pub struct TextString<'a>(pub Cow<'a, str>);

impl std::fmt::Display for TextString<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.0.as_ref(), f)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TextString<'_> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(self.0.as_ref(), cx)
    }
}

impl proa_core::shared::attr_value::AttrValue for TextString<'_> {
    fn should_render(&self) -> bool {
        true
    }

    fn is_boolean_attr(&self) -> bool {
        false
    }

    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        proa_core::shared::attr_value::AttrValue::render_attr_value(&self.0.as_ref(), cx)
    }
}
