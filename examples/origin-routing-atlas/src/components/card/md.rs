// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle};
use proa_core::WebContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

/// Card is a block wrapper: a boundary before and after its children.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for Card<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.md_block_boundary()?;
        self.children.render_md(cx)?;
        cx.md_block_boundary()?;
        Ok(())
    }
}

/// CardHeader is a passthrough — its children (title, description) handle formatting.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for CardHeader<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        Ok(())
    }
}

/// CardTitle renders as a `##` heading.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for CardTitle<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.md_block_boundary()?;
        // The level is fixed, like the `<h3>` the HTML twin renders: nesting
        // a Card inside another does not deepen its title.
        cx.md_heading_with(2, |cx| self.children.render_md(cx))
    }
}

/// CardDescription renders as paragraph text.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for CardDescription<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        cx.md_blank_line()?;
        Ok(())
    }
}

/// CardContent is a passthrough.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for CardContent<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        cx.md_blank_line()?;
        Ok(())
    }
}

/// CardFooter is a passthrough.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for CardFooter<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        cx.md_blank_line()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    struct Stops(bool);
    impl<B: WriteBuf> MdRenderSync<proa_core::NoLoader, B> for Stops {
        fn render_md(self, cx: &mut WebContext<proa_core::NoLoader, B>) -> Result<(), WriteError> {
            cx.write_md_prepared_static(b"Partial")?;
            if self.0 {
                panic!("injected Card child panic");
            }
            Err(WriteError::render("injected Card child error"))
        }
    }

    #[test]
    fn nested_cards_keep_partial_output_after_errors_and_unwinding() {
        for panic in [false, true] {
            let mut cx = WebContext::with_buffer(Vec::new());
            cx.write_md_prepared_static(b"Intro").unwrap();
            let cards = Card {
                class: None,
                children: Some(Card {
                    class: None,
                    children: Some(Stops(panic)),
                }),
            };
            let result = catch_unwind(AssertUnwindSafe(|| cards.render_md(&mut cx)));
            if panic {
                assert!(result.is_err());
            } else {
                assert_eq!(
                    result.unwrap(),
                    Err(WriteError::render("injected Card child error"))
                );
            }
            assert_eq!(cx.out, b"Intro\n\nPartial");
            cx.md_block_boundary().unwrap();
            cx.md_heading(1, "Fallback").unwrap();
            assert_eq!(cx.into_output(), b"Intro\n\nPartial\n\n# Fallback\n\n");
        }
    }

    #[test]
    fn nested_card_titles_keep_the_same_level() {
        let mut cx = WebContext::with_buffer(Vec::new());
        let cards = Card {
            class: None,
            children: Some((
                CardTitle {
                    class: None,
                    children: Some(proa_macros::md_sync! { "Outer" }),
                },
                Card {
                    class: None,
                    children: Some(CardTitle {
                        class: None,
                        children: Some(proa_macros::md_sync! { "Inner" }),
                    }),
                },
            )),
        };
        cards.render_md(&mut cx).unwrap();
        assert_eq!(cx.into_output(), b"## Outer\n\n## Inner\n\n");
    }
}
