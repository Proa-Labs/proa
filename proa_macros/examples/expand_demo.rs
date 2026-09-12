// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{text, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
struct UserCard {
    name: &'static str,
}
impl<L: ::proa_core::DataLoader> WebRenderSync<L> for UserCard {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <div class="user-card">
                <span>{text(self.name)}</span>
            </div>
        }
        .render(cx)
    }
}
struct Page {
    title: &'static str,
}
impl<L: ::proa_core::DataLoader> WebRenderSync<L> for Page {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let user_card = UserCard { name: "Alice" };
        html_sync! {
            <div class="page">
                <h1>{text(self.title)}</h1>
                {user_card}
            </div>
        }
        .render(cx)
    }
}
fn main() {
    use proa_core::render_to_string;
    let page = Page {
        title: "User Profile",
    };
    let output = render_to_string(page);
    println!("{}", output);
}
