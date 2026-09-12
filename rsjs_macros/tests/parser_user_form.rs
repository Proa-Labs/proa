// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M7 integration test: a real `#[rsjs(component, client)] impl WebRenderSync<::proa_core::NoLoader, ...> for UserForm`
//! block end-to-end through the proc-macro pipeline.
//!
//! Mirrors the M5 hand-built UserForm fixture's rendered HTML
//! byte-for-byte. Exercises:
//!
//! - Multiple signals (`username`, `email`, `submitting`).
//! - Two `bind_value` input bindings.
//! - An `on_submit` handler that performs `signal.set(true)`.
//! - Two conditional sections (one per error message), each gated by
//!   a different predicate shape: `IsNonEmpty`-based (username) and
//!   `==`-against-empty-string (email).

// See parser_counter.rs for why these imports are silenced.
use proa_core::render_to_string;
#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs_macros::rsjs;

// Parser marker replaced by the attribute macro before method resolution.
// Only the constructor and initial value survive expansion.
#[allow(dead_code)]
fn signal<T>(initial: T) -> T {
    initial
}

pub struct UserForm;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for UserForm {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let username = signal(String::new());
        let email = signal(String::new());
        let submitting = signal(false);
        proa_macros::html_sync! {
            <form on_submit={|_| submitting.set(true)}>
                <input bind_value={username} />
                <div>
                    {if !username.is_non_empty() {
                        proa_macros::html_sync! {
                            <p class="error">"Username required"</p>
                        }
                    }}
                </div>
                <input bind_value={email} />
                <div>
                    {if email.get() == String::new() {
                        proa_macros::html_sync! {
                            <p class="error">"Email required"</p>
                        }
                    }}
                </div>
                <button type="submit">"Submit"</button>
            </form>
        }
        .render(cx)
    }
}

/// Expected HTML body (modulo the asset id, which is now a hashed
/// disambiguator: `userform_<hash8>`; see `rsjs_macros::asset`).
///
/// User-authored attributes (`class="error"`, `type="submit"`) and
/// static text (`"Username required"`, `"Email required"`,
/// `"Submit"`) are now preserved verbatim: emit_rust threads
/// `ViewNode::Element::static_attrs` and `ViewNode::StaticText`
/// straight into the generated `html_sync!` body.
fn expected_user_form_html(asset_id: &str) -> String {
    format!(
        concat!(
            r#"<rsjs-island data-rsjs-island-root="{id}">"#,
            r#"<form data-rsjs-handler="0">"#,
            r#"<input data-rsjs-input-value="1" value="">"#,
            r#"<div>"#,
            r#"<p class="error" data-rsjs-cond-then="2:0" style="">Username required</p>"#,
            r#"</div>"#,
            r#"<input data-rsjs-input-value="5" value="">"#,
            r#"<div>"#,
            r#"<p class="error" data-rsjs-cond-then="6:0" style="">Email required</p>"#,
            r#"</div>"#,
            r#"<button type="submit">Submit</button>"#,
            r#"</form>"#,
            r#"</rsjs-island>"#,
            r#"<script type="application/json" data-rsjs-island="{id}">"#,
            r#"{{"signals":["","",false]}}"#,
            r#"</script>"#,
        ),
        id = asset_id
    )
}

#[test]
fn user_form_renders_expected_hydration_html() {
    let html = render_to_string(UserForm);
    let prefix = r#"<rsjs-island data-rsjs-island-root=""#;
    let after_prefix = html
        .strip_prefix(prefix)
        .expect("rendered HTML must start with the rsjs-island wrapper");
    let id_end = after_prefix.find('"').expect("missing closing quote");
    let asset_id = &after_prefix[..id_end];
    assert!(
        asset_id.starts_with("userform_"),
        "expected userform_<hash> asset id, got: {asset_id}"
    );
    assert_eq!(html, expected_user_form_html(asset_id));
}
