// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Build-time content hashes for local images. Unknown and external URLs pass
//! through unchanged; callers can share the same URL across HTML and metadata.
include!(concat!(env!("OUT_DIR"), "/image_urls.rs"));

#[cfg(test)]
#[path = "../image_assets_build.rs"]
mod image_assets_build;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn every_public_image_has_a_versioned_url() {
        fn visit(root: &Path, dir: &Path, count: &mut usize) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(root, &path, count);
                } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    matches!(
                        e,
                        "svg" | "png" | "jpg" | "jpeg" | "webp" | "ico" | "avif" | "gif"
                    )
                }) {
                    let url = format!(
                        "/static/{}",
                        path.strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/")
                    );
                    let versioned = image_url(&url);
                    let hash = versioned
                        .strip_prefix(&format!("{url}?v="))
                        .expect("image must be versioned");
                    assert_eq!(hash.len(), 16, "{url}");
                    assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()), "{url}");
                    assert_eq!(image_url(versioned), versioned, "must not double-version");
                    *count += 1;
                }
            }
        }
        let public = Path::new(env!("CARGO_MANIFEST_DIR")).join("public");
        let mut count = 0;
        visit(&public, &public, &mut count);
        assert!(count > 0);
    }
}
