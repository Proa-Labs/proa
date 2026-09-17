// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The island hydration blob must stay inside the HTML script-data boundary
//! even when a `ClientEncode` implementation does not.
//!
//! `ClientEncode` is public and unsealed, and its documented wire format is
//! plain JSON string escaping, which says nothing about `<`. An application
//! encoder written to that contract (delegating to `serde_json`, for instance)
//! emits `</script>` verbatim, and the blob is written into an inline
//! `<script type="application/json">` through `UnsafeRaw`. Without a
//! script-safe pass at the sink, attacker-controlled text stored in a signal
//! closes the JSON script element and the rest of the payload runs as markup.

use proa_core::{Ctx, NoLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

/// A deliberately minimal encoder: quotes, backslash, and control characters,
/// exactly what the wire-format contract documents and what `serde_json`
/// produces. It does not escape `<`, `>`, or `&`.
pub struct RawBio(pub String);

impl rsjs::ClientEncode for RawBio {
    fn encode_into(&self, out: &mut String) {
        out.push('"');
        for character in self.0.chars() {
            match character {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                other if (other as u32) < 0x20 => {
                    out.push_str(&format!("\\u{:04x}", other as u32));
                }
                other => out.push(other),
            }
        }
        out.push('"');
    }
}

pub struct ProfileCard {
    pub bio: RawBio,
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ProfileCard {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let bio = rsjs::signal(self.bio);

        proa_macros::html_sync! {
            <div on_click={|_| bio.update(|value| value)}>
                <span>"profile"</span>
            </div>
        }
        .render(cx)
    }
}

fn render_profile(bio: &str) -> String {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    ProfileCard {
        bio: RawBio(bio.to_string()),
    }
    .render(&mut cx)
    .expect("island renders");
    String::from_utf8(out).expect("utf-8 html")
}

#[test]
fn a_client_encoder_that_ignores_angle_brackets_cannot_break_out_of_the_blob() {
    let html = render_profile("</script><script>alert(document.cookie)</script>");

    assert!(
        !html.contains("</script><script>"),
        "hydration blob escaped its script element: {html}"
    );
    assert!(
        !html.contains("alert(document.cookie)</script>"),
        "attacker markup survived into the document: {html}"
    );
    // One opening tag for the blob, and one closing tag that the emitter wrote.
    assert_eq!(html.matches("<script").count(), 1, "{html}");
    assert_eq!(html.matches("</script>").count(), 1, "{html}");
    assert!(html.contains("\\u003C"), "{html}");
}

#[test]
fn an_already_escaped_payload_is_not_escaped_twice() {
    // The literal six characters `<`: an encoder that already did the
    // escaping (the derive-generated one does) must not have its backslash
    // re-encoded into `\\u003C`, which would hydrate as the wrong text.
    let html = render_profile("safe \\u003C tail");

    assert!(html.contains("safe \\\\u003C tail"), "{html}");
    assert!(!html.contains("\\\\\\\\u003C"), "{html}");
}
