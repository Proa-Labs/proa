// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{text, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
#[test]
fn test_component_is_struct_not_string() {
    struct UserCard {
        name: String,
        render_count: std::rc::Rc<std::cell::Cell<usize>>,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for UserCard {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            self.render_count.set(self.render_count.get() + 1);
            html_sync! {
                <div>{text(& self.name)}</div>
            }
            .render(cx)
        }
    }
    struct Page {
        user_card: UserCard,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Page {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            let user_card = self.user_card;
            let name_length = user_card.name.len();
            assert_eq!(name_length, 5);
            assert_eq!(user_card.render_count.get(), 0);
            html_sync! {
                <div>{user_card}</div>
            }
            .render(cx)?;
            Ok(())
        }
    }
    use proa_core::render_to_string;
    let render_count = std::rc::Rc::new(std::cell::Cell::new(0));
    let _output = render_to_string(Page {
        user_card: UserCard {
            name: "Alice".to_string(),
            render_count: render_count.clone(),
        },
    });
    assert_eq!(render_count.get(), 1);
}
#[test]
fn test_can_inspect_component_before_rendering() {
    struct Counter {
        value: i32,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Counter {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <span>{text(& format!("{}", self.value))}</span>
            }
            .render(cx)
        }
    }
    struct Page;
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Page {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            let counter = Counter { value: 42 };
            assert_eq!(counter.value, 42);
            let doubled = counter.value * 2;
            assert_eq!(doubled, 84);
            html_sync! {
                <div>{counter}</div>
            }
            .render(cx)
        }
    }
    use proa_core::render_to_string;
    let output = render_to_string(Page);
    assert_eq!(output, "<div><span>42</span></div>");
}
#[test]
fn test_can_conditionally_render_struct() {
    struct Badge {
        text: String,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Badge {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <span class="badge">{text(& self.text)}</span>
            }
            .render(cx)
        }
    }
    struct Page {
        show_admin: bool,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Page {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            let admin_badge = Badge {
                text: "Admin".to_string(),
            };
            let user_badge = Badge {
                text: "User".to_string(),
            };
            assert_eq!(admin_badge.text, "Admin");
            assert_eq!(user_badge.text, "User");
            html_sync! {
                <div>{if self.show_admin { admin_badge } else { user_badge }}</div>
            }
            .render(cx)
        }
    }
    use proa_core::render_to_string;
    let output_admin = render_to_string(Page { show_admin: true });
    assert_eq!(
        output_admin,
        r#"<div><span class="badge">Admin</span></div>"#
    );
    let output_user = render_to_string(Page { show_admin: false });
    assert_eq!(output_user, r#"<div><span class="badge">User</span></div>"#);
}
#[test]
fn test_component_not_cloned_or_copied() {
    struct Expensive {
        data: Vec<String>,
    }
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Expensive {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <div>{text(& format!("Items: {}", self.data.len()))}</div>
            }
            .render(cx)
        }
    }
    struct Page;
    impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Page {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            let expensive = Expensive {
                data: vec!["a".to_string(), "b".to_string()],
            };
            html_sync! {
                <div>{expensive}</div>
            }
            .render(cx)?;
            Ok(())
        }
    }
    use proa_core::render_to_string;
    let output = render_to_string(Page);
    assert_eq!(output, "<div><div>Items: 2</div></div>");
}
