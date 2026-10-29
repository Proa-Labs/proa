// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Videos linked from the homepage, shared with its structured metadata.

use crate::data::site;
use const_format::concatcp;

pub(crate) struct Video {
    pub name: &'static str,
    pub url: &'static str,
    pub thumbnail_url: &'static str,
    pub embed_url: &'static str,
}

const INTRO_ID: &str = "-CfCyvIKYyo";
const RENDER_TARGETS_ID: &str = "pJx84jdBDKU";
const INTRO_TITLE: &str = concatcp!(site::NAME, ": ", site::TAGLINE_LEAD, " In Rust!");
const RENDER_TARGETS_TITLE: &str = concatcp!(site::NAME, ": Multi-Target Rendering in Rust");

// Names and thumbnails come from these videos' public YouTube oEmbed metadata.
pub(crate) const INTRO: Video = Video {
    name: INTRO_TITLE,
    url: concatcp!("https://youtu.be/", INTRO_ID),
    thumbnail_url: concatcp!("https://i.ytimg.com/vi/", INTRO_ID, "/hqdefault.jpg"),
    embed_url: concatcp!("https://www.youtube.com/embed/", INTRO_ID),
};

pub(crate) const RENDER_TARGETS: Video = Video {
    name: RENDER_TARGETS_TITLE,
    url: concatcp!("https://youtu.be/", RENDER_TARGETS_ID),
    thumbnail_url: concatcp!(
        "https://i.ytimg.com/vi/",
        RENDER_TARGETS_ID,
        "/hqdefault.jpg"
    ),
    embed_url: concatcp!("https://www.youtube.com/embed/", RENDER_TARGETS_ID),
};
