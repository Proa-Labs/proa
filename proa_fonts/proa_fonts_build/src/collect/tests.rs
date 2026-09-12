// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn scans_both_macros_from_source() {
    let src = r#"
        use proa_fonts::Font;
        static SANS: Font = google_font!("Geist", weights = [400, 700], variable = "--font-sans");
        fn layout() {
            let _ = local_font!("Brand", files = ["fonts/Brand.woff2"]);
        }
        // a fully-qualified path still matches on the last segment:
        static MONO: Font = proa_fonts_macros::google_font!("Geist Mono", variable_font = true, generic = mono);
    "#;
    let mut c = Collector::default();
    c.visit_file(&syn::parse_file(src).unwrap());
    assert_eq!(c.requests.len(), 3);
    let families: Vec<_> = c.requests.iter().map(|r| r.family.as_str()).collect();
    assert!(families.contains(&"Geist"));
    assert!(families.contains(&"Brand"));
    assert!(families.contains(&"Geist Mono"));
}

#[test]
fn malformed_calls_are_skipped_not_fatal() {
    let src = r#"
        static A: Font = google_font!("Ok", weights = [400]);
        static B: Font = google_font!("Bad", bogus = 1);
    "#;
    let mut c = Collector::default();
    c.visit_file(&syn::parse_file(src).unwrap());
    // Only the well-formed call is collected; the macro reports the other.
    assert_eq!(c.requests.len(), 1);
    assert_eq!(c.requests[0].family, "Ok");
}

#[test]
fn unparseable_files_are_reported_not_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("ok.rs"), "fn fine() {}\n").unwrap();
    std::fs::write(src.join("broken.rs"), "fn broken( {\n").unwrap();
    let outcome = scan_requests(&src).unwrap();
    assert_eq!(outcome.skipped.len(), 1);
    assert!(outcome.skipped[0].ends_with("broken.rs"));
}

#[test]
fn infers_weight_and_style() {
    assert_eq!(
        infer_weight_style("Geist-700-italic.woff2", false, 400),
        (FontWeight::Fixed(700), FontStyle::Italic)
    );
    assert_eq!(
        infer_weight_style("Brand[wght].woff2", true, 400),
        (
            FontWeight::Variable { min: 100, max: 900 },
            FontStyle::Normal
        )
    );
}

#[test]
fn weight_field_round_trips() {
    assert_eq!(format_weight_field(FontWeight::Fixed(400)), "400");
    assert_eq!(
        format_weight_field(FontWeight::Variable { min: 100, max: 900 }),
        "100..900"
    );
    assert_eq!(parse_weight_field("400"), Some(FontWeight::Fixed(400)));
    assert_eq!(
        parse_weight_field("100..900"),
        Some(FontWeight::Variable { min: 100, max: 900 })
    );
    assert_eq!(parse_weight_field("bold"), None);
}

#[test]
fn plain_filenames_only() {
    assert!(is_plain_filename("geist-latin-400.abc123.woff2"));
    assert!(!is_plain_filename("../escape.woff2"));
    assert!(!is_plain_filename("dir/inner.woff2"));
    assert!(!is_plain_filename(""));
}

#[test]
fn write_if_changed_leaves_identical_files_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.txt");
    write_if_changed(&path, b"content").unwrap();
    let first = std::fs::metadata(&path).unwrap().modified().unwrap();
    // A same-content write must not rewrite the file.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_if_changed(&path, b"content").unwrap();
    let second = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(first, second);
    write_if_changed(&path, b"changed").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"changed");
}

fn system_ttf() -> Option<Vec<u8>> {
    [
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Geneva.ttf",
        "/System/Library/Fonts/Monaco.ttf",
    ]
    .iter()
    .find(|p| Path::new(p).exists())
    .map(|p| std::fs::read(p).unwrap())
}

/// Scaffold a scratch crate declaring one local font and collect it.
fn scratch_crate(dir: &Path, bytes: &[u8], declaration: &str) -> CollectOptions {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("fonts")).unwrap();
    std::fs::write(dir.join("fonts/Brand-400.ttf"), bytes).unwrap();
    std::fs::write(dir.join("src/lib.rs"), declaration).unwrap();
    CollectOptions {
        crate_root: dir.to_path_buf(),
        out_dir: dir.join("out"),
        fonts_out_dir: PathBuf::from("public/fonts"),
        url_prefix: "/fonts".to_string(),
        lock_path: None,
    }
}

const DECLARATION: &str = r#"
    static BRAND: Font = local_font!("Brand", files = ["fonts/Brand-400.ttf"], variable = "--font-brand", license = "OFL-1.1");
"#;

#[test]
fn vendors_locks_and_takes_the_fast_path_on_rerun() {
    let Some(bytes) = system_ttf() else {
        eprintln!("no system font; skipping collector end-to-end test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let opts = scratch_crate(dir.path(), &bytes, DECLARATION);

    // First run vendors and locks.
    let first = run(&opts).unwrap();
    assert_eq!(first.fonts.len(), 1);
    assert!(first.reports[0].refreshed);
    assert!(!first.reports[0].repaired);
    assert_eq!(first.reports[0].files, 1);
    let lock_path = dir.path().join(LOCK_FILE);
    let lock_text = std::fs::read_to_string(&lock_path).unwrap();
    assert!(lock_text.contains("request_id"));
    assert!(lock_text.contains("OFL-1.1"));
    let module = std::fs::read_to_string(&first.module_path).unwrap();
    assert!(module.contains(&first.fonts[0]));

    // Second run is satisfied from the lock: nothing refreshed, same output.
    let lock_mtime = std::fs::metadata(&lock_path).unwrap().modified().unwrap();
    let second = run(&opts).unwrap();
    assert!(!second.reports[0].refreshed);
    assert_eq!(second.fonts, first.fonts);
    assert_eq!(
        std::fs::metadata(&lock_path).unwrap().modified().unwrap(),
        lock_mtime,
        "an unchanged lock must not be rewritten"
    );

    // Verify-only mode passes and writes nothing.
    let verified = sync(&opts, true).unwrap();
    assert!(!verified.reports[0].refreshed);
}

#[test]
fn changed_declaration_revendors_and_cleans_up_the_old_files() {
    let Some(bytes) = system_ttf() else {
        eprintln!("no system font; skipping collector cleanup test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let opts = scratch_crate(dir.path(), &bytes, DECLARATION);
    let first = run(&opts).unwrap();
    let first_key = first.reports[0].key.clone();

    // Change the declaration (new css variable -> new request id).
    std::fs::write(
        dir.path().join("src/lib.rs"),
        r#"
        static BRAND: Font = local_font!("Brand", files = ["fonts/Brand-400.ttf"], variable = "--font-brand-two");
        "#,
    )
    .unwrap();
    let second = run(&opts).unwrap();
    assert!(second.reports[0].refreshed);
    assert!(!second.reports[0].repaired);
    assert_ne!(second.reports[0].key, first_key);
    // Same bytes -> same hashed filename, so the vendored file survives as
    // shared content and nothing is removed.
    assert!(second.removed.is_empty());

    // Remove every declaration: the lock becomes a comment-only stub (it must
    // keep existing for rerun-if-changed) and vendored files are deleted.
    std::fs::write(dir.path().join("src/lib.rs"), "// no fonts\n").unwrap();
    let third = run(&opts).unwrap();
    assert!(third.fonts.is_empty());
    assert_eq!(third.removed.len(), 1);
    let lock_path = dir.path().join(LOCK_FILE);
    assert!(lock_path.exists(), "empty lock stub must exist");
    let stub: FontsLock = toml::from_str(&std::fs::read_to_string(&lock_path).unwrap()).unwrap();
    assert!(stub.fonts.is_empty());
    let vendored: Vec<_> = std::fs::read_dir(dir.path().join("public/fonts"))
        .unwrap()
        .collect();
    assert!(vendored.is_empty(), "vendored dir should be empty");
    // The generated module exists but declares nothing.
    let module = std::fs::read_to_string(&third.module_path).unwrap();
    assert!(!module.contains("use proa_fonts"));
}

#[test]
fn empty_crate_still_writes_the_lock_stub() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "// no fonts\n").unwrap();
    let opts = CollectOptions {
        crate_root: dir.path().to_path_buf(),
        out_dir: dir.path().join("out"),
        fonts_out_dir: PathBuf::from("public/fonts"),
        url_prefix: "/fonts".to_string(),
        lock_path: None,
    };
    let result = run(&opts).unwrap();
    assert!(result.fonts.is_empty());
    let lock_path = dir.path().join(LOCK_FILE);
    assert!(lock_path.exists());
    // The stub parses as an empty lock and stays byte-stable across reruns.
    let text = std::fs::read_to_string(&lock_path).unwrap();
    let parsed: FontsLock = toml::from_str(&text).unwrap();
    assert!(parsed.fonts.is_empty());
    let mtime = std::fs::metadata(&lock_path).unwrap().modified().unwrap();
    run(&opts).unwrap();
    assert_eq!(
        std::fs::metadata(&lock_path).unwrap().modified().unwrap(),
        mtime
    );
}

#[test]
fn tampered_vendored_bytes_force_a_repair() {
    let Some(bytes) = system_ttf() else {
        eprintln!("no system font; skipping collector tamper test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let opts = scratch_crate(dir.path(), &bytes, DECLARATION);
    let first = run(&opts).unwrap();

    // Corrupt the vendored file; the next run must re-vendor from the source
    // and report the repair.
    let lock: FontsLock =
        toml::from_str(&std::fs::read_to_string(dir.path().join(LOCK_FILE)).unwrap()).unwrap();
    let filename = &lock.fonts.values().next().unwrap().files[0].filename;
    let vendored = dir.path().join("public/fonts").join(filename);
    std::fs::write(&vendored, b"corrupt").unwrap();

    // Verify-only mode fails on the tamper instead of writing.
    let err = sync(&opts, true).unwrap_err();
    assert!(matches!(err, BuildError::Stale(_)));
    assert_eq!(std::fs::read(&vendored).unwrap(), b"corrupt");

    let second = run(&opts).unwrap();
    assert!(second.reports[0].refreshed);
    assert!(second.reports[0].repaired);
    assert_eq!(second.fonts, first.fonts);
    assert_eq!(std::fs::read(&vendored).unwrap(), bytes);
}

#[test]
fn edited_local_source_file_forces_a_refresh() {
    let Some(bytes) = system_ttf() else {
        eprintln!("no system font; skipping local-edit test");
        return;
    };
    // A second, different valid font for the edit.
    let other = [
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Geneva.ttf",
        "/System/Library/Fonts/Monaco.ttf",
    ]
    .iter()
    .filter(|p| Path::new(p).exists())
    .nth(1)
    .map(|p| std::fs::read(p).unwrap());
    let Some(other) = other else {
        eprintln!("need two system fonts; skipping local-edit test");
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let opts = scratch_crate(dir.path(), &bytes, DECLARATION);
    let first = run(&opts).unwrap();
    assert!(first.reports[0].refreshed);
    let second = run(&opts).unwrap();
    assert!(!second.reports[0].refreshed);

    // Replace the source file's bytes; the declaration is unchanged but the
    // fast path must notice and re-vendor, cleaning up the orphaned file.
    std::fs::write(dir.path().join("fonts/Brand-400.ttf"), &other).unwrap();
    let third = run(&opts).unwrap();
    assert!(third.reports[0].refreshed);
    assert_eq!(third.removed.len(), 1, "old vendored file is orphaned");
    let lock: FontsLock =
        toml::from_str(&std::fs::read_to_string(dir.path().join(LOCK_FILE)).unwrap()).unwrap();
    assert_eq!(
        lock.fonts.values().next().unwrap().files[0].sha256,
        hash::sha256_hex(&other)
    );
}

#[test]
fn parse_failure_preserves_lock_and_files() {
    let Some(bytes) = system_ttf() else {
        eprintln!("no system font; skipping parse-failure test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let opts = scratch_crate(dir.path(), &bytes, DECLARATION);
    run(&opts).unwrap();
    let lock_before = std::fs::read_to_string(dir.path().join(LOCK_FILE)).unwrap();
    let lock: FontsLock = toml::from_str(&lock_before).unwrap();
    let filename = lock.fonts.values().next().unwrap().files[0]
        .filename
        .clone();
    let vendored = dir.path().join("public/fonts").join(filename);

    // Save the declaration file mid-edit with a syntax error: the collector
    // must preserve everything rather than treat the fonts as removed.
    std::fs::write(dir.path().join("src/lib.rs"), "fn broken( {\n").unwrap();
    let second = run(&opts).unwrap();
    assert_eq!(second.skipped_sources.len(), 1);
    assert_eq!(second.preserved.len(), 1);
    assert!(second.removed.is_empty());
    assert!(
        vendored.exists(),
        "vendored file must survive a broken save"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(LOCK_FILE)).unwrap(),
        lock_before,
        "lock must survive a broken save"
    );

    // Restoring the file goes back to the fast path.
    std::fs::write(dir.path().join("src/lib.rs"), DECLARATION).unwrap();
    let third = run(&opts).unwrap();
    assert!(!third.reports[0].refreshed);
}

#[test]
fn verify_mode_fails_on_unvendored_declarations_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        r#"static SANS: Font = google_font!("Geist", variable = "--font-sans");"#,
    )
    .unwrap();
    let opts = CollectOptions {
        crate_root: dir.path().to_path_buf(),
        out_dir: dir.path().join("out"),
        fonts_out_dir: PathBuf::from("public/fonts"),
        url_prefix: "/fonts".to_string(),
        lock_path: None,
    };
    let err = sync(&opts, true).unwrap_err();
    assert!(matches!(err, BuildError::Stale(_)));
    assert!(
        !dir.path().join(LOCK_FILE).exists(),
        "verify writes nothing"
    );
}

#[test]
fn verify_mode_fails_on_local_drift_too() {
    let Some(bytes) = system_ttf() else {
        eprintln!("no system font; skipping local verify test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let opts = scratch_crate(dir.path(), &bytes, DECLARATION);
    // Never vendored: verify must fail even though the source is local.
    let err = sync(&opts, true).unwrap_err();
    assert!(matches!(err, BuildError::Stale(_)));
    assert!(!dir.path().join(LOCK_FILE).exists());

    // After vendoring, a removed declaration is drift in verify mode.
    run(&opts).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "// no fonts\n").unwrap();
    let err = sync(&opts, true).unwrap_err();
    assert!(matches!(err, BuildError::Stale(_)));
    let lock: FontsLock =
        toml::from_str(&std::fs::read_to_string(dir.path().join(LOCK_FILE)).unwrap()).unwrap();
    assert_eq!(lock.fonts.len(), 1, "verify must not clean up");
}
