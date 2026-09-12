// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Dump the rendered GitHub PR-diff page HTML to stdout, for parity diffing
//! against the React SSR companion.

use proa_core::{WebContext, WebRenderSync};
use proa_demo::github_pr_diff::{GithubPrDiffPage, GithubPrDiffProps};

fn main() {
    let props = GithubPrDiffProps::default().clamped();
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    GithubPrDiffPage { props: &props }
        .render(&mut cx)
        .expect("render github pr diff page");
    print!("{}", String::from_utf8(out).expect("utf8 html"));
}
