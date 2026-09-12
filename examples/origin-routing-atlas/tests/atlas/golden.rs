// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Byte-for-byte golden test for the server-rendered atlas document.
//!
//! The page renders live RIPEstat data, so the golden run injects the fixture
//! loader and stays deterministic. Three normalizations: the rsjs owner asset
//! id embeds the linked build plan hash, which changes with the toolchain and
//! dependency set; the static asset URLs embed a cache-busting byte length
//! that changes with every asset edit; and the receipt's render benchmark is
//! a process-wide value that the route tests in this binary populate by
//! calling `app()`. Refresh with:
//!
//! ```text
//! PROA_UPDATE_GOLDEN=1 cargo proa test -p proa_origin_routing_atlas --test atlas
//! ```

use std::sync::Arc;

use proa_origin_routing_atlas::loader::FixtureLoader;

fn normalize(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(index) = rest.find("rsjs_owner_") {
        let (head, tail) = rest.split_at(index + "rsjs_owner_".len());
        output.push_str(head);
        let hash_length = tail
            .chars()
            .take_while(|character| character.is_ascii_hexdigit())
            .count();
        if hash_length == 64 {
            output.push_str("PLAN");
            rest = &tail[64..];
        } else {
            rest = tail;
        }
    }
    output.push_str(rest);
    normalize_receipt_render(&normalize_asset_versions(&output))
}

/// The receipt prints the startup render benchmark once `app()` has recorded
/// it (a process-wide `OnceLock`). The route tests that share this binary
/// call `app()` while the golden renders, so the measured value is folded
/// back to the placeholder the fixture was captured with.
fn normalize_receipt_render(html: &str) -> String {
    const MARKER: &str = "data-receipt-render";
    let Some(marker) = html.find(MARKER) else {
        return html.to_string();
    };
    let Some(tag_end) = html[marker..].find('>') else {
        return html.to_string();
    };
    let content = marker + tag_end + 1;
    let Some(length) = html[content..].find('<') else {
        return html.to_string();
    };
    let mut output = String::with_capacity(html.len());
    output.push_str(&html[..content]);
    output.push('\u{2014}');
    output.push_str(&html[content + length..]);
    output
}

/// The asset URLs carry a cache-busting byte length (`?v=18849`) that changes
/// with every asset edit; the digits are normalized while the shape stays.
fn normalize_asset_versions(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(index) = rest.find("?v=") {
        let (head, tail) = rest.split_at(index + "?v=".len());
        output.push_str(head);
        let digits = tail
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .count();
        if digits > 0 {
            output.push_str("BYTES");
            rest = &tail[digits..];
        } else {
            rest = tail;
        }
    }
    output.push_str(rest);
    output
}

#[tokio::test]
async fn atlas_document_matches_the_golden_fixture_byte_for_byte() {
    let html =
        proa_origin_routing_atlas::render_atlas_page(None, Arc::new(FixtureLoader::github())).await;
    let normalized = normalize(&html);

    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/home.html");
    if std::env::var_os("PROA_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden")).unwrap();
        std::fs::write(fixture_path, &normalized).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(fixture_path)
        .expect("golden fixture exists; run PROA_UPDATE_GOLDEN=1 cargo proa test -p proa_origin_routing_atlas --test atlas");
    if normalized != expected {
        let divergence = normalized
            .bytes()
            .zip(expected.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| normalized.len().min(expected.len()));
        let start = divergence.saturating_sub(80);
        let window = |s: &str| {
            s.get(start..(divergence + 80).min(s.len()))
                .unwrap_or("<out of range>")
                .to_string()
        };
        panic!(
            "SSR output drifted from tests/golden/home.html at byte {divergence};\n\
             rendered: …{}…\n\
             golden:   …{}…\n\
             refresh with PROA_UPDATE_GOLDEN=1 cargo proa test -p proa_origin_routing_atlas --test atlas if the change is intentional",
            window(&normalized),
            window(&expected)
        );
    }
}
