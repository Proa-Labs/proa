// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn path_to_slug<'a>(path: &'a str, base_path: &str) -> &'a str {
    let normalized = normalize_path(path);
    let base = normalize_path(base_path);
    let without_base = normalized.strip_prefix(base).unwrap_or(normalized);
    normalize_slug(without_base.trim_start_matches('/'))
}

pub(crate) fn normalize_path(path: &str) -> &str {
    if path.is_empty() {
        "/"
    } else {
        path.trim_end_matches('/').trim_end_matches("index.html")
    }
}

pub(crate) fn normalize_slug(slug: &str) -> &str {
    slug.trim_matches('/')
        .strip_suffix(".html")
        .unwrap_or(slug.trim_matches('/'))
        .strip_suffix(".md")
        .unwrap_or_else(|| slug.trim_matches('/'))
}
