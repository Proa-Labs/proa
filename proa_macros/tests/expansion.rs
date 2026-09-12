// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{el, text, void, Attr, AttrKey, Tag, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
#[test]
fn test_macro_expands_to_same_as_manual() {
    use proa_core::render_to_string;
    let manual = el(
        Tag::Div,
        [Attr::Str {
            k: AttrKey::Class,
            v: "container",
        }],
        |cx| el(Tag::H1, [], |cx| text("Hello").render(cx)).render(cx),
    );
    let from_macro = html_sync! {
        <div class="container">
            <h1>"Hello"</h1>
        </div>
    };
    let manual_output = render_to_string(manual);
    let macro_output = render_to_string(from_macro);
    assert_eq!(manual_output, macro_output);
    assert_eq!(
        manual_output,
        r#"<div class="container"><h1>Hello</h1></div>"#
    );
}
#[test]
fn test_void_element_expands_correctly() {
    use proa_core::render_to_string;
    let manual = void(
        Tag::Img,
        [
            Attr::Str {
                k: AttrKey::Src,
                v: "/logo.png",
            },
            Attr::Str {
                k: AttrKey::Alt,
                v: "Logo",
            },
        ],
    );
    let from_macro = html_sync! {
        <img src="/logo.png" alt="Logo" />
    };
    let manual_output = render_to_string(manual);
    let macro_output = render_to_string(from_macro);
    assert_eq!(manual_output, macro_output);
}
#[test]
fn test_component_stays_as_struct() {
    struct Counter {
        count: i32,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Counter {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <span>{text(& format!("Count: {}", self.count))}</span>
            }
            .render(cx)
        }
    }
    struct Page {
        counter: Counter,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Page {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            let counter = self.counter;
            assert_eq!(counter.count, 42);
            html_sync! {
                <div>{counter}</div>
            }
            .render(cx)
        }
    }
    use proa_core::render_to_string;
    let output = render_to_string(Page {
        counter: Counter { count: 42 },
    });
    assert_eq!(output, "<div><span>Count: 42</span></div>");
}
#[test]
fn test_nested_components_render_in_order() {
    struct Inner {
        value: String,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Inner {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <span>{text(& self.value)}</span>
            }
            .render(cx)
        }
    }
    struct Outer {
        inner1: Inner,
        inner2: Inner,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Outer {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <div>{self.inner1}{self.inner2}</div>
            }
            .render(cx)
        }
    }
    use proa_core::render_to_string;
    let output = render_to_string(Outer {
        inner1: Inner {
            value: "First".to_string(),
        },
        inner2: Inner {
            value: "Second".to_string(),
        },
    });
    assert_eq!(output, "<div><span>First</span><span>Second</span></div>");
}
#[test]
fn test_type_safety_component_must_be_struct() {
    struct Card;
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Card {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <div class="card">"Card content"</div>
            }
            .render(cx)
        }
    }
    struct Page {
        card: Card,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Page {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            let card = self.card;
            html_sync! {
                <div>{card}</div>
            }
            .render(cx)
        }
    }
    use proa_core::render_to_string;
    let output = render_to_string(Page { card: Card });
    assert_eq!(output, r#"<div><div class="card">Card content</div></div>"#);
}
#[test]
fn test_macro_result_implements_render() {
    fn assert_implements_render<T: WebRenderSync<::proa_core::NoLoader>>(_: T) {}
    let element = html_sync! {
        <div>"Hello"</div>
    };
    assert_implements_render(element);
}
#[test]
fn test_dynamic_attributes_evaluated_at_render_time() {
    use proa_core::render_to_string;
    let class_name = "dynamic";
    let output = render_to_string(html_sync! {
        <div class={class_name}>"Content"</div>
    });
    assert_eq!(output, r#"<div class="dynamic">Content</div>"#);
}
#[test]
fn test_expressions_called_inside_closure() {
    struct Component {
        rendered: std::cell::Cell<bool>,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Component {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            self.rendered.set(true);
            html_sync! {
                <span>"Component"</span>
            }
            .render(cx)
        }
    }
    use proa_core::render_to_string;
    let component = Component {
        rendered: std::cell::Cell::new(false),
    };
    assert!(!component.rendered.get());
    let _output = render_to_string(html_sync! {
        <div>{component}</div>
    });
}
