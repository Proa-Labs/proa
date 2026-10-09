// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Run with: cargo run -p proa_core --example html_to_markdown --features html-to-markdown

use proa_core::{to_markdown, DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteError};
use proa_macros::{html_sync, md_sync};

struct ProductCard<'a> {
    name: &'a str,
    description: &'a str,
}

impl<L: DataLoader> WebRenderSync<L> for ProductCard<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <article class="rounded-lg border p-4">
                <h2>{self.name}</h2>
                <p>{self.description}</p>
                <a href="/products/proa">"Learn more"</a>
            </article>
        }
        .render(cx)
    }
}

impl<L: DataLoader> MdRenderSync<L> for ProductCard<'_> {
    fn render_md(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        to_markdown(self).render_md(cx)
    }
}

struct Notice<'a>(&'a str);

impl<L: DataLoader> MdRenderSync<L> for Notice<'_> {
    fn render_md(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        md_sync! { r###"
            > {self.0}
            "###
        }
        .render_md(cx)
    }
}

fn main() -> Result<(), WriteError> {
    let mut cx = WebContext::new();
    md_sync! { r###"
        # Our products

        {ProductCard {
            name: "Proa",
            description: "Compiled templates for Rust.",
        }}

        {to_markdown(html_sync! {
            <p>"Built for " <strong>"fast rendering"</strong> "."</p>
        })}

        {Notice("This child uses its handwritten Markdown representation.")}
        "###
    }
    .render_md(&mut cx)?;
    println!("{}", cx.into_output().into_string());
    Ok(())
}
