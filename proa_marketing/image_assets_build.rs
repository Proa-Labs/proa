// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Generate borrowed URL literals at build time, with one content hash per image.
pub fn generate(public: &Path) -> io::Result<String> {
    fn visit(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                visit(&path, files)?;
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                matches!(
                    e,
                    "svg" | "png" | "jpg" | "jpeg" | "webp" | "ico" | "avif" | "gif"
                )
            }) {
                files.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(public, &mut files)?;
    files.sort();
    let mut output = String::from("pub fn image_url(path: &str) -> &str { match path {\n");
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let rel = path
            .strip_prefix(public)
            .expect("public image")
            .to_string_lossy()
            .replace('\\', "/");
        let url = format!("/static/{rel}");
        let hash = fnv1a(0xcbf29ce484222325, &fs::read(&path)?);
        output.push_str(&format!(
            "{url:?} => {versioned:?},\n",
            versioned = format!("{url}?v={hash:016x}")
        ));
    }
    output.push_str("_ => path, } }\n");
    Ok(output)
}

pub(crate) fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_image_changes_only_its_url_and_new_images_are_discovered() {
        let root = std::env::temp_dir().join(format!("proa-image-assets-{}", std::process::id()));
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("a.svg"), "first image").unwrap();
        fs::write(root.join("nested/b.webp"), "second image").unwrap();
        let before = generate(&root).unwrap();
        assert_eq!(before, generate(&root).unwrap());
        fs::write(root.join("a.svg"), "updated image").unwrap();
        let after = generate(&root).unwrap();
        let line = |s: &str, key: &str| {
            s.lines()
                .find(|line| line.contains(key))
                .unwrap()
                .to_owned()
        };
        assert_ne!(line(&before, "a.svg"), line(&after, "a.svg"));
        assert_eq!(line(&before, "b.webp"), line(&after, "b.webp"));
        fs::write(root.join("nested/new.png"), "new image").unwrap();
        assert!(generate(&root)
            .unwrap()
            .contains("/static/nested/new.png?v="));
        fs::remove_dir_all(root).unwrap();
    }
}
