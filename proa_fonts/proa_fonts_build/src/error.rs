// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Error type for the build pipeline.

/// Anything that can go wrong while vendoring a font and generating its module.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("font data ended unexpectedly while reading a fixed-size field")]
    Truncated,

    #[error("required font table `{name}` is missing")]
    MissingTable { name: &'static str },

    #[error("invalid font: {0}")]
    InvalidFont(&'static str),

    #[error("unsupported font container (expected sfnt `.ttf`/`.otf` or WOFF2)")]
    UnsupportedContainer,

    #[error("WOFF2 decode failed: {0}")]
    Woff2(String),

    #[error("a font spec must contain at least one face")]
    NoFaces,

    #[error("fetching font data failed: {0}")]
    Fetch(String),

    #[error(
        "font `{0}` needs vendoring but the build is offline; run `cargo proa font sync` \
         online and commit proa-fonts.lock together with the vendored files"
    )]
    Offline(String),

    #[error(
        "proa-fonts.lock is stale: {0}; run `cargo proa font sync` (or `cargo build`) \
         online and commit proa-fonts.lock together with the vendored files"
    )]
    Stale(String),

    #[error(
        "font declarations `{0}` and `{1}` collide on the same generated id; \
         change one declaration (e.g. its css variable) to disambiguate"
    )]
    IdCollision(String, String),

    #[error("proa-fonts.lock is unreadable: {0}")]
    Lock(String),

    #[error("build environment variable `{0}` is not set")]
    Environment(&'static str),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}
