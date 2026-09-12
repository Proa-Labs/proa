// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{render_to_string, text, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
#[test]
fn test_simple_element() {
    let output = render_to_string(html_sync! {
        <div>"Hello World"</div>
    });
    assert_eq!(output, "<div>Hello World</div>");
}
#[test]
fn test_nested_elements() {
    let output = render_to_string(html_sync! {
        <div class="container">
            <h1>"Title"</h1>
            <p>"Description"</p>
        </div>
    });
    assert_eq!(
        output,
        r#"<div class="container"><h1>Title</h1><p>Description</p></div>"#
    );
}
#[test]
fn test_void_element() {
    let output = render_to_string(html_sync! {
        <img src="/logo.png" alt="Logo" />
    });
    assert_eq!(output, r#"<img src="/logo.png" alt="Logo">"#);
}
#[test]
fn test_with_component() {
    struct UserCard {
        name: String,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for UserCard {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            use proa_core::text;
            html_sync! {
                <div class="user-card">
                    <span>"Name: "</span>
                    <span>
                        {text(&
                        self.name)}
                    </span>
                </div>
            }
            .render(cx)
        }
    }
    struct Page {
        user_name: String,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Page {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            let user_card = UserCard {
                name: self.user_name,
            };
            html_sync! {
                <div class="page">
                    <h1>"User Profile"</h1>
                    {user_card}
                </div>
            }
            .render(cx)
        }
    }
    let output = render_to_string(Page {
        user_name: "Alice".to_string(),
    });
    assert_eq!(
        output,
        r#"<div class="page"><h1>User Profile</h1><div class="user-card"><span>Name: </span><span>Alice</span></div></div>"#
    );
}
#[test]
fn test_dynamic_attribute() {
    let class_name = "my-class";
    let output = render_to_string(html_sync! {
        <div class={class_name}>"Content"</div>
    });
    assert_eq!(output, r#"<div class="my-class">Content</div>"#);
}
#[test]
fn test_boolean_attribute() {
    let output = render_to_string(html_sync! {
        <input type="text" disabled />
    });
    assert_eq!(output, r#"<input type="text" disabled>"#);
}
#[test]
fn test_if_else() {
    struct Greeting {
        logged_in: bool,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Greeting {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <div>
                    {if self.logged_in { html_sync! { < span > "Welcome back!" </ span >
                    } } else { html_sync! { < a href = "/login" > "Log in" </ a > } }}
                </div>
            }
            .render(cx)
        }
    }
    let output = render_to_string(Greeting { logged_in: true });
    assert_eq!(output, "<div><span>Welcome back!</span></div>");
    let output = render_to_string(Greeting { logged_in: false });
    assert_eq!(output, r#"<div><a href="/login">Log in</a></div>"#);
}
#[test]
fn test_if_without_else() {
    struct MaybeWarning {
        show: bool,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for MaybeWarning {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <div>
                    {if self.show { html_sync! { < span class = "warn" > "Warning!" </
                    span > } }}
                </div>
            }
            .render(cx)
        }
    }
    let output = render_to_string(MaybeWarning { show: true });
    assert_eq!(output, r#"<div><span class="warn">Warning!</span></div>"#);
    let output = render_to_string(MaybeWarning { show: false });
    assert_eq!(output, "<div></div>");
}
#[test]
fn test_else_if_chain() {
    struct StatusBadge {
        level: u8,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for StatusBadge {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <div>
                    {if self.level == 0 { html_sync! { < span class = "low" > "Low" </
                    span > } } else if self.level == 1 { html_sync! { < span class = "mid" >
                    "Medium" </ span > } } else { html_sync! { < span class = "high" > "High" </
                    span > } }}
                </div>
            }
            .render(cx)
        }
    }
    let output = render_to_string(StatusBadge { level: 0 });
    assert_eq!(output, r#"<div><span class="low">Low</span></div>"#);
    let output = render_to_string(StatusBadge { level: 1 });
    assert_eq!(output, r#"<div><span class="mid">Medium</span></div>"#);
    let output = render_to_string(StatusBadge { level: 2 });
    assert_eq!(output, r#"<div><span class="high">High</span></div>"#);
}
#[test]
fn test_match_arms() {
    enum Color {
        Red,
        Green,
        Blue,
    }
    struct ColorLabel {
        color: Color,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for ColorLabel {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <div>
                    {match self.color { Color::Red => html_sync! { < span class = "red" >
                    "Red" </ span > }, Color::Green => html_sync! { < span class = "green" >
                    "Green" </ span > }, Color::Blue => html_sync! { < span class = "blue" >
                    "Blue" </ span > }, }}
                </div>
            }
            .render(cx)
        }
    }
    let output = render_to_string(ColorLabel { color: Color::Red });
    assert_eq!(output, r#"<div><span class="red">Red</span></div>"#);
    let output = render_to_string(ColorLabel {
        color: Color::Green,
    });
    assert_eq!(output, r#"<div><span class="green">Green</span></div>"#);
    let output = render_to_string(ColorLabel { color: Color::Blue });
    assert_eq!(output, r#"<div><span class="blue">Blue</span></div>"#);
}
#[test]
fn test_for_loop() {
    struct ItemList {
        items: Vec<String>,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for ItemList {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <ul>
                    {for item in & self.items { html_sync! { < li > { text(item) } </ li >
                    } }}
                </ul>
            }
            .render(cx)
        }
    }
    let output = render_to_string(ItemList {
        items: vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
    });
    assert_eq!(output, "<ul><li>Alpha</li><li>Beta</li><li>Gamma</li></ul>");
}
#[test]
fn test_for_loop_multi_statement_body() {
    struct NumberList {
        numbers: Vec<i32>,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for NumberList {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <ul>
                    {for n in & self.numbers { let label = format!("Item {}", n);
                    html_sync! { < li > { text(& label) } </ li > } }}
                </ul>
            }
            .render(cx)
        }
    }
    let output = render_to_string(NumberList {
        numbers: vec![1, 2, 3],
    });
    assert_eq!(
        output,
        "<ul><li>Item 1</li><li>Item 2</li><li>Item 3</li></ul>"
    );
}
#[test]
fn test_nested_control_flow() {
    struct Grid {
        rows: Vec<Vec<String>>,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Grid {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <table>
                    {for row in & self.rows { html_sync! { < tr > { for cell in row {
                    html_sync! { < td > { text(cell) } </ td > } } } </ tr > } }}
                </table>
            }
            .render(cx)
        }
    }
    let output = render_to_string(Grid {
        rows: vec![vec!["A".into(), "B".into()], vec!["C".into(), "D".into()]],
    });
    assert_eq!(
        output,
        "<table><tr><td>A</td><td>B</td></tr><tr><td>C</td><td>D</td></tr></table>"
    );
}
