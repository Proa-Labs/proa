// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared site identity and external destinations. Keep page-specific copy in
//! its owning component. Absolute links to this deployment use public_site_url().

use const_format::{concatcp, map_ascii_case, Case};

pub const NAME: &str = "Proa";
pub const ORGANIZATION_NAME: &str = "Proa Labs";
pub const ORGANIZATION: &str = concatcp!(ORGANIZATION_NAME, ", Inc.");
pub const TAGLINE: &str = "The next era of web interfaces. Built in Rust.";
pub const HOME_TITLE: &str = concatcp!(NAME, " — ", TAGLINE);
// Inherit the workspace's canonical homepage instead of duplicating it here.
pub const CANONICAL_ORIGIN: &str = env!("CARGO_PKG_HOMEPAGE");

pub const GITHUB_ORIGIN: &str = "https://github.com";
pub const GITHUB_ORGANIZATION: &str = "Proa-Labs";
pub const GITHUB_REPOSITORY: &str = concatcp!(GITHUB_ORGANIZATION, "/proa");
// Preserve the site's lowercase links/copy and the release API's repository
// spelling without maintaining a second owner/repository value.
pub const GITHUB_REPOSITORY_SLUG: &str = map_ascii_case!(Case::Lower, GITHUB_REPOSITORY);
pub const GITHUB_ORGANIZATION_URL: &str = concatcp!(
    GITHUB_ORIGIN,
    "/",
    map_ascii_case!(Case::Lower, GITHUB_ORGANIZATION)
);
pub const GITHUB_URL: &str = concatcp!(GITHUB_ORIGIN, "/", GITHUB_REPOSITORY_SLUG);
pub const ISSUES_URL: &str = concatcp!(GITHUB_URL, "/issues");
pub const LICENSE_URL: &str = concatcp!(GITHUB_URL, "/blob/main/LICENSING.md");
pub const BENCHMARKS_URL: &str =
    concatcp!(GITHUB_ORIGIN, "/", GITHUB_ORGANIZATION, "/proa-benchmarks");

// This community redirect stays on the canonical site, including in previews.
pub const DISCORD_URL: &str = concatcp!(CANONICAL_ORIGIN, "/discord");
pub const REDDIT_URL: &str = "https://www.reddit.com/r/proa_rs/";
pub const YOUTUBE_URL: &str = "https://www.youtube.com/@proa-web";
pub const SOCIAL_USERNAME: &str = "drewhariri";
pub const SOCIAL_HANDLE: &str = concatcp!("@", SOCIAL_USERNAME);
pub const SOCIAL_URL: &str = concatcp!("https://x.com/", SOCIAL_USERNAME);

pub const UI_URL: &str = "https://ui.proa.so";
pub const UI_DOCS_URL: &str = concatcp!(UI_URL, "/docs");
pub const UI_COMPONENTS_URL: &str = concatcp!(UI_URL, "/components");

pub const PRIVACY_EMAIL: &str = "privacy@proa.so";
pub const PRIVACY_MAILTO: &str = concatcp!("mailto:", PRIVACY_EMAIL);
pub const LEGAL_EMAIL: &str = "legal@proa.so";
pub const LEGAL_MAILTO: &str = concatcp!("mailto:", LEGAL_EMAIL);
