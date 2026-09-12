// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Showcase pages — pixel-perfect mirrors of real e-commerce / travel
//! sites used as comparison targets for proa-rsjs vs Marko 6 / React 19 SSR.
//! Lives in its own crate so that edits to a single page
//! (`airbnb/search.rs`, etc.) only recompile this crate (~5K LOC)
//! instead of the full component catalog (~20K LOC).

// Showcase pages call `.render(cx)` extensively; the Result is intentionally unused.
// Struct updates with `..Default::default()` are used for clarity even when all fields are set.
#![allow(unused_must_use, clippy::needless_update)]

pub mod airbnb;
pub mod ebay;
#[cfg(feature = "rsjs-pages")]
pub mod espn;
pub mod nike;
#[cfg(feature = "sentry-otel")]
pub mod sentry_otel;
#[cfg(feature = "rsjs-pages")]
pub mod settings_console;

#[cfg(feature = "rsjs-pages")]
use proa_core::{CspNonce, RsjsRenderArtifacts, WriteError};

/// Finalize rsjs browser assets for a fully rendered showcase response.
///
/// Component and `render_into` paths deliberately do not call this helper.
/// Only response owners that also own the completed document should insert the
/// page-local runtime plan before the terminal body tag.
#[cfg(feature = "rsjs-pages")]
pub(crate) fn finalize_rsjs_document(
    html: &mut String,
    artifacts: &RsjsRenderArtifacts,
    nonce: Option<&CspNonce>,
) -> Result<(), WriteError> {
    let plan = rsjs::manifest()
        .page_runtime_plan(artifacts.rendered_islands())
        .expect("rendered showcase islands must exist in the linked rsjs manifest");
    let mut tags = Vec::new();
    plan.write_html(&mut tags, nonce)?;

    const DOCUMENT_END: &str = "</body></html>";
    let body_end = html
        .strip_suffix(DOCUMENT_END)
        .expect("showcase document must end with </body></html>")
        .len();
    html.truncate(body_end);
    html.push_str(std::str::from_utf8(&tags).expect("rsjs runtime plan must be valid UTF-8"));
    html.push_str(DOCUMENT_END);
    Ok(())
}
