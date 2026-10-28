// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Videos linked from the homepage, shared with its structured metadata.

pub(crate) struct Video {
    pub name: &'static str,
    pub url: &'static str,
    pub thumbnail_url: &'static str,
    pub embed_url: &'static str,
}

const INTRO_ID: &str = "-CfCyvIKYyo";
const RENDER_TARGETS_ID: &str = "pJx84jdBDKU";

// Names and thumbnails come from these videos' public YouTube oEmbed metadata.
pub(crate) const INTRO: Video = Video {
    name: "Proa: The next era of web interfaces. In Rust!",
    url: const_format::concatcp!("https://youtu.be/", INTRO_ID),
    thumbnail_url: const_format::concatcp!("https://i.ytimg.com/vi/", INTRO_ID, "/hqdefault.jpg"),
    embed_url: const_format::concatcp!("https://www.youtube.com/embed/", INTRO_ID),
};

pub(crate) const RENDER_TARGETS: Video = Video {
    name: "Proa: Multi-Target Rendering in Rust",
    url: const_format::concatcp!("https://youtu.be/", RENDER_TARGETS_ID),
    thumbnail_url: const_format::concatcp!(
        "https://i.ytimg.com/vi/",
        RENDER_TARGETS_ID,
        "/hqdefault.jpg"
    ),
    embed_url: const_format::concatcp!("https://www.youtube.com/embed/", RENDER_TARGETS_ID),
};
