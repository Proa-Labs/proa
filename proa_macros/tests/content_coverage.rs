// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A small, compiled application-style pair kept under the content CI gate.
use proa_core::{MdRenderSync, WebContext, WebRenderSync, WriteError};
use proa_macros::{html_sync, md_sync};

struct Product<'a> {
    title: &'a str,
    restock_at: &'a str,
}
struct RestockNotice<'a> {
    at: &'a str,
}

impl WebRenderSync for RestockNotice<'_> {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <aside data-restock>{self.at}</aside>
        }
        .render(cx)
    }
}
impl MdRenderSync for RestockNotice<'_> {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        md_sync! { r###"{self.at}"### }.render_md(cx)
    }
}

impl WebRenderSync for Product<'_> {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <article data-product>
                <h1>{self.title}</h1>
                {RestockNotice { at: self.restock_at }}
            </article>
        }
        .render(cx)
    }
}
impl MdRenderSync for Product<'_> {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let restock = RestockNotice {
            at: self.restock_at,
        };
        md_sync! { r###"
            # {self.title}

            {restock}
            "###
        }
        .render_md(cx)
    }
}

#[test]
fn each_view_includes_dynamic_restock_information_and_escapes_for_its_target() {
    let mut html = WebContext::new();
    Product {
        title: "<Product>",
        restock_at: "Thursday & Friday",
    }
    .render(&mut html)
    .unwrap();
    let html = html.into_output().into_string();
    assert_eq!(html, "<article data-product><h1>&lt;Product&gt;</h1><aside data-restock>Thursday &amp; Friday</aside></article>");
    let mut markdown = WebContext::new();
    Product {
        title: "Next product",
        restock_at: "2030-03-04",
    }
    .render_md(&mut markdown)
    .unwrap();
    let markdown = markdown.into_output().into_string();
    assert_eq!(markdown, "# Next product\n\n2030\\-03\\-04\n");
}
