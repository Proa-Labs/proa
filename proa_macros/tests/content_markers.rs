// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{MdRenderSync, WebContext, WebRenderSync, WriteError};
use proa_macros::{content, content_part, html_sync, md_sync, omit};
use std::{cell::Cell, ops::Deref};

struct Product {
    title: String,
}

#[content(skip)]
impl WebRenderSync for Product {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <h1>{content_part!("title", self.title)}</h1>
        }
        .render(cx)
    }
}

impl MdRenderSync for Product {
    #[content(skip, reason = "Independent Markdown representation")]
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        omit!(self.title);
        md_sync! { r###"# {content_part!("title", self.title)}"### }.render_md(cx)
    }
}

#[test]
fn markers_preserve_owned_values_and_target_escaping() {
    let title = String::from("<&title>");
    let mut cx = WebContext::new();
    Product {
        title: title.clone(),
    }
    .render(&mut cx)
    .unwrap();
    let html = cx.into_output().into_string();
    assert_eq!(html, "<h1>&lt;&amp;title&gt;</h1>");
    let mut actual = WebContext::new();
    Product {
        title: title.clone(),
    }
    .render_md(&mut actual)
    .unwrap();
    let mut expected = WebContext::new();
    md_sync! { r###"# {title}"### }
        .render_md(&mut expected)
        .unwrap();
    assert_eq!(
        actual.into_output().into_string(),
        expected.into_output().into_string()
    );
}

struct Fields {
    image: String,
}
struct Observable<'a> {
    fields: Fields,
    dereferences: &'a Cell<usize>,
}
impl Deref for Observable<'_> {
    type Target = Fields;
    fn deref(&self) -> &Fields {
        self.dereferences.set(self.dereferences.get() + 1);
        &self.fields
    }
}

#[test]
fn omission_type_checks_without_evaluating_field_access_or_moving_values() {
    let dereferences = Cell::new(0);
    let value = Observable {
        fields: Fields {
            image: String::from("owned"),
        },
        dereferences: &dereferences,
    };
    omit!(value.image, "Decorative", from = "html");
    assert_eq!(dereferences.get(), 0);
    assert_eq!(value.fields.image, "owned");
    let value = String::from("still owned");
    omit!(&value);
    assert_eq!(value, "still owned");
}

#[test]
fn child_and_role_selectors_do_not_construct_or_evaluate_content() {
    #[allow(dead_code)]
    struct Notice<'a, T> {
        label: &'a str,
        value: T,
    }
    omit!(Notice<'_, String>);
    omit!(str);
    omit!("restock", "Intentionally omitted");
    let calls = Cell::new(0);
    let output = content_part!("title", {
        calls.set(calls.get() + 1);
        "value"
    });
    assert_eq!(calls.get(), 1);
    assert_eq!(output, "value");
}

#[test]
fn omission_selectors_allow_locals_named_child_and_role() {
    let child = String::from("child");
    let role = String::from("role");
    omit!(&child);
    omit!(&role, "intentional");
    assert_eq!(child, "child");
    assert_eq!(role, "role");
}

#[test]
fn type_and_value_names_can_overlap_without_constructing_or_moving_them() {
    #[allow(non_camel_case_types, dead_code)]
    struct notice {
        value: String,
    }
    let notice = String::from("still owned");
    omit!(notice);
    omit!(&notice);
    assert_eq!(notice, "still owned");

    struct Impact;
    impl Drop for Impact {
        fn drop(&mut self) {
            panic!("a type omission must never construct the child");
        }
    }
    omit!(Impact);
}
