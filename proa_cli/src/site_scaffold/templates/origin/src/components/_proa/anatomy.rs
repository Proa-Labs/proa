// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Stable multipart-component anatomy and allocation-free part identifiers.
//!
//! Recipes own presentation, while an anatomy names the DOM contract shared by
//! static rendering, the delegated runtime, rsjs islands, tests, and external
//! styles. Components should expose the same scope and part names on every
//! render target.

use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

/// The public DOM anatomy for a multipart component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anatomy {
    scope: &'static str,
    parts: &'static [&'static str],
}

impl Anatomy {
    /// Define a component scope and its complete set of public parts.
    pub const fn new(scope: &'static str, parts: &'static [&'static str]) -> Self {
        Self { scope, parts }
    }

    /// Stable value rendered through `data-scope`.
    pub const fn scope(self) -> &'static str {
        self.scope
    }

    /// Stable values accepted by `data-part`.
    pub const fn parts(self) -> &'static [&'static str] {
        self.parts
    }

    /// Return whether a part belongs to this component's public anatomy.
    pub fn contains(self, part: &str) -> bool {
        self.parts.contains(&part)
    }

    /// Resolve a named part without allocating.
    pub fn part(self, part: &'static str) -> Option<AnatomyPart> {
        self.contains(part).then_some(AnatomyPart {
            scope: self.scope,
            part,
        })
    }
}

/// A validated scope/part pair suitable for component metadata and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnatomyPart {
    scope: &'static str,
    part: &'static str,
}

impl AnatomyPart {
    pub const fn scope(self) -> &'static str {
        self.scope
    }

    pub const fn part(self) -> &'static str {
        self.part
    }
}

/// An allocation-free ID derived from a component root ID and a named part.
///
/// `PartId::new(Some("account"), "trigger")` renders `account--trigger`.
/// Passing `None` omits the attribute, which lets compound components expose
/// optional IDs without creating temporary `String`s in the render path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartId<'a> {
    root: Option<&'a str>,
    part: &'static str,
}

impl<'a> PartId<'a> {
    pub const fn new(root: Option<&'a str>, part: &'static str) -> Self {
        Self { root, part }
    }
}

impl AttrValue for PartId<'_> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        self.root.is_some()
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        if let Some(root) = self.root {
            let dialect = cx.__proa_current_escape_dialect();
            proa_core::shared::escape::escape_attr_value_for_dialect(&mut cx.out, root, dialect)?;
            cx.out.extend_static(b"--")?;
            proa_core::shared::escape::escape_attr_value_for_dialect(
                &mut cx.out,
                self.part,
                dialect,
            )?;
        }
        Ok(())
    }
}
