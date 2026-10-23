// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![cfg(feature = "macros")]

use proa_fonts::{google_font, local_font, Font};

// Stand-ins for the collector's generated descriptors. These names pin the
// existing declaration IDs; macro expansion must require no font-file I/O.
mod __proa_fonts_generated {
    use proa_fonts::Font;

    pub const FONT_96F015FA: Font = Font {
        family: "Google fixture",
        class_name: "google-fixture",
        variable: "",
        family_stack: "sans-serif",
        faces: &[],
        css: "",
    };
    pub const FONT_AF3EBFCD: Font = Font {
        family: "Local fixture",
        class_name: "local-fixture",
        ..FONT_96F015FA
    };
}

static GOOGLE: Font = google_font!("Geist");
static LOCAL: Font = local_font!("Brand", files = ["fonts/Brand.woff2"]);

#[test]
fn public_macros_resolve_the_collectors_stable_descriptor_names() {
    assert_eq!(GOOGLE.class_name(), "google-fixture");
    assert_eq!(LOCAL.class_name(), "local-fixture");
    assert_eq!(
        proa_fonts::request::FontRequest::google("Geist").const_ident(),
        "FONT_96F015FA"
    );
    assert_eq!(
        proa_fonts::request::FontRequest::local("Brand", vec!["fonts/Brand.woff2".into()])
            .const_ident(),
        "FONT_AF3EBFCD"
    );
}
