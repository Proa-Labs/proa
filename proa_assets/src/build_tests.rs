// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

struct Fixture {
    _temp: tempfile::TempDir,
    source: PathBuf,
    output: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("public");
        let output = temp.path().join("target/proa-output/test");
        fs::create_dir(&source).unwrap();
        Self {
            _temp: temp,
            source,
            output,
        }
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        let path = self.source.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn publish(&self) -> PublishReport {
        let report = OutputStore::open(&self.output)
            .unwrap()
            .publish(&self.source)
            .unwrap();
        assert!(report.durability_error.is_none());
        assert!(report.cleanup.is_ok());
        report
    }
}

#[test]
fn copied_bytes_drive_names_even_when_size_and_mtime_are_unchanged() {
    let f = Fixture::new();
    f.write("images/logo.svg", b"first");
    let first = f.publish().manifest;
    assert_eq!(first, f.publish().manifest);
    let path = f.source.join("images/logo.svg");
    let times = fs::FileTimes::new().set_modified(fs::metadata(&path).unwrap().modified().unwrap());
    f.write("images/logo.svg", b"other");
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(times)
        .unwrap();
    let second = f.publish().manifest;
    let before = &first.assets["images/logo.svg"];
    let after = &second.assets["images/logo.svg"];
    assert_eq!(before.byte_length, after.byte_length);
    assert_ne!(before.public_path, after.public_path);
    let catalog = AssetCatalog::load(&f.output).unwrap();
    assert_eq!(
        fs::read(catalog.object_path("images/logo.svg").unwrap()).unwrap(),
        b"other"
    );
    assert_eq!(after.sha256, sha256(b"other"));
}

#[test]
fn shared_catalog_protects_objects_until_the_last_reader_drops() {
    let f = Fixture::new();
    f.write("movie.mp4", b"old movie");
    f.publish();
    let old = std::sync::Arc::new(AssetCatalog::load(&f.output).unwrap());
    let old_file = old.object_path("movie.mp4").unwrap();
    let clone = old.clone();
    f.write("movie.mp4", b"new movie");
    assert!(f.publish().cleanup.unwrap().live_readers);
    let new = AssetCatalog::load(&f.output).unwrap();
    assert_ne!(old.generation(), new.generation());
    assert_eq!(fs::read(&old_file).unwrap(), b"old movie");
    drop(old);
    drop(new);
    let mut writer = OutputStore::open(&f.output).unwrap();
    assert!(writer.cleanup().unwrap().live_readers);
    drop(clone);
    assert_eq!(writer.cleanup().unwrap().removed_objects, 1);
    assert!(!old_file.exists());
    assert!(AssetCatalog::load(&f.output).is_ok());
}

#[test]
fn only_a_registered_live_reader_lets_the_writer_skip_a_restart() {
    let f = Fixture::new();
    f.write("logo.svg", b"logo");
    f.publish();
    let writer = OutputStore::open(&f.output).unwrap();
    assert!(!writer.has_live_reader().unwrap());

    // A fixed catalog holds the object reader lock, yet cannot follow a new
    // manifest. It must never be mistaken for a live reader.
    let fixed = AssetCatalog::load(&f.output).unwrap();
    assert!(!writer.has_live_reader().unwrap());

    let first = LiveReader::register(&f.output).unwrap();
    let second = LiveReader::register(&f.output).unwrap();
    assert!(writer.has_live_reader().unwrap());
    // Probing is repeatable and does not evict the registered readers.
    assert!(writer.has_live_reader().unwrap());
    drop(first);
    assert!(writer.has_live_reader().unwrap());
    drop(second);
    assert!(!writer.has_live_reader().unwrap());
    drop(fixed);

    // An output prepared by a writer without the registration file cannot
    // promise in-place reloads, so registration fails rather than creating it.
    fs::remove_file(f.output.join(LIVE_READERS_FILE)).unwrap();
    assert!(LiveReader::register(&f.output).is_err());
}

#[test]
fn cleanup_keeps_stale_objects_while_an_active_object_is_incomplete() {
    let f = Fixture::new();
    f.write("movie.mp4", b"old movie");
    f.publish();
    let reader = AssetCatalog::load(&f.output).unwrap();
    let stale = reader.object_path("movie.mp4").unwrap();
    f.write("movie.mp4", b"new movie");
    f.publish();
    drop(reader);
    let active = AssetCatalog::load(&f.output)
        .unwrap()
        .object_path("movie.mp4")
        .unwrap();
    fs::write(&active, b"new").unwrap();
    let mut writer = OutputStore::open(&f.output).unwrap();
    assert!(matches!(writer.cleanup(), Err(AssetError::Integrity(_))));
    assert!(stale.exists());
    fs::write(&active, b"new movie").unwrap();
    assert_eq!(writer.cleanup().unwrap().removed_objects, 1);
    assert!(!stale.exists());
}

#[test]
fn collision_reservations_survive_object_cleanup() {
    let f = Fixture::new();
    f.write("logo.svg", b"old");
    let old = f.publish().manifest;
    let old_entry = &old.assets["logo.svg"];
    fs::remove_file(f.source.join("logo.svg")).unwrap();
    f.publish();
    assert!(!f.output.join("objects").join(&old_entry.object).exists());
    let mut writer = OutputStore::open(&f.output).unwrap();
    assert_eq!(
        writer.history().unwrap().assignments[&old_entry.object],
        old_entry.sha256
    );

    // Force the otherwise expensive SHA-prefix collision at the persisted
    // reservation boundary, after its object has been pruned.
    f.write("logo.svg", b"new");
    let digest = sha256(b"new");
    let object = fingerprinted_path("logo.svg", &digest);
    let mut history = writer.history().unwrap();
    let other_digest = format!(
        "{}{}",
        &digest[..FINGERPRINT_LENGTH],
        "0".repeat(64 - FINGERPRINT_LENGTH)
    );
    history.assignments.insert(object.clone(), other_digest);
    fs::write(
        f.output.join(HISTORY_FILE),
        serde_json::to_vec(&history).unwrap(),
    )
    .unwrap();
    let pointer = fs::read(f.output.join(MANIFEST_FILE)).unwrap();
    assert!(matches!(
        writer.publish(&f.source),
        Err(AssetError::Collision { .. })
    ));
    assert_eq!(pointer, fs::read(f.output.join(MANIFEST_FILE)).unwrap());
    assert!(!f.output.join("objects").join(object).exists());
}

#[test]
fn only_immutable_names_are_reserved_and_an_older_history_is_not_carried_over() {
    let f = Fixture::new();
    f.write("icon.svg", b"image");
    f.write("style.css", b"css");
    f.write("main.mjs", b"js");
    let first = f.publish().manifest;
    let mut writer = OutputStore::open(&f.output).unwrap();
    let reserved = |manifest: &AssetManifest| -> BTreeMap<String, String> {
        ["icon.svg", "style.css"]
            .into_iter()
            .map(|logical| {
                let entry = &manifest.assets[logical];
                (entry.public_path.clone(), entry.sha256.clone())
            })
            .collect()
    };
    assert_eq!(writer.history().unwrap().assignments, reserved(&first));

    // Names issued under another scheme cannot collide with this one.
    let legacy = History {
        version: MANIFEST_VERSION - 1,
        assignments: BTreeMap::from([("icon.0a1b2c3d.svg".into(), sha256(b"legacy"))]),
    };
    fs::write(
        f.output.join(HISTORY_FILE),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let second = writer.publish(&f.source).unwrap().manifest;
    assert_eq!(writer.history().unwrap().assignments, reserved(&second));
}

#[test]
fn warm_publish_repairs_deleted_output_and_removes_deleted_inputs() {
    let f = Fixture::new();
    f.write("one.png", b"1");
    f.write("two.png", b"2");
    let before = f.publish().manifest;
    fs::remove_dir_all(f.output.join("objects")).unwrap();
    assert_eq!(before, f.publish().manifest);
    fs::remove_file(f.source.join("two.png")).unwrap();
    let after = f.publish();
    assert_eq!(after.manifest.assets.len(), 1);
    assert_eq!(after.cleanup.unwrap().removed_objects, 1);
    assert!(AssetCatalog::load(f.output)
        .unwrap()
        .object_path("two.png")
        .is_none());
}

#[test]
fn missing_source_or_corrupt_output_does_not_replace_the_pointer() {
    let f = Fixture::new();
    f.write("logo.png", b"bytes");
    f.publish();
    let pointer = fs::read(f.output.join(MANIFEST_FILE)).unwrap();
    let mut writer = OutputStore::open(&f.output).unwrap();
    assert!(writer.publish(f.source.join("missing")).is_err());
    let catalog = AssetCatalog::load(&f.output).unwrap();
    fs::write(catalog.object_path("logo.png").unwrap(), b"wrong").unwrap();
    assert!(matches!(
        AssetCatalog::load(&f.output),
        Err(AssetError::Integrity(_))
    ));
    assert!(matches!(
        writer.publish(&f.source),
        Err(AssetError::Integrity(_))
    ));
    assert_eq!(pointer, fs::read(f.output.join(MANIFEST_FILE)).unwrap());
}

#[test]
fn only_owned_output_is_cleaned_and_writers_are_exclusive() {
    let f = Fixture::new();
    fs::create_dir_all(&f.output).unwrap();
    fs::write(f.output.join("unrelated"), b"keep").unwrap();
    assert!(matches!(
        OutputStore::open(&f.output),
        Err(AssetError::NotOwned(_))
    ));
    assert_eq!(fs::read(f.output.join("unrelated")).unwrap(), b"keep");
    fs::remove_file(f.output.join("unrelated")).unwrap();
    let writer = OutputStore::open(&f.output).unwrap();
    assert!(matches!(
        OutputStore::open(&f.output),
        Err(AssetError::WriterBusy(_))
    ));
    let abandoned = f.output.join("staging/attempt-abandoned");
    fs::create_dir(&abandoned).unwrap();
    fs::write(abandoned.join("partial"), b"partial").unwrap();
    drop(writer);
    let _writer = OutputStore::open(&f.output).unwrap();
    assert!(!abandoned.exists());
}

#[test]
fn fingerprinted_names_are_kept_and_formats_with_unmanaged_urls_keep_theirs() {
    let f = Fixture::new();
    let font = format!(
        "fonts/inter.{}.woff2",
        &sha256(b"font")[..FINGERPRINT_LENGTH]
    );
    f.write(&font, b"font");
    f.write("scripts/main.js", b"import './dependency.js';");
    f.write("data/feed.json", b"{}");
    let manifest = f.publish().manifest;
    assert_eq!(manifest.assets[&font].public_path, font);
    assert_eq!(
        manifest.assets[&font].url_path(),
        format!("immutable/{font}")
    );
    for name in ["scripts/main.js", "data/feed.json"] {
        let entry = &manifest.assets[name];
        assert_eq!(entry.public_path, name);
        assert_eq!(entry.url_path(), format!("mutable/{name}"));
        assert_eq!(entry.cache_policy, CachePolicy::Revalidate);
        assert_ne!(entry.object, name);
    }
}

#[test]
fn stylesheets_are_hashed_after_their_references_point_at_published_files() {
    let f = Fixture::new();
    f.write("images/a.png", b"image");
    f.write("data.json", b"{}");
    f.write("styles/base.css", b"b { background: url(../images/a.png) }");
    f.write(
        "styles/app.css",
        br#"@import "base.css";
a { background: url('../images/a.png?v=1#x'); mask: url(../data.json) }
c { background: url(https://cdn.example/x.png), url(data:image/png;base64,AA), url(#id) }"#,
    );
    let report = f.publish();
    assert_eq!(report.diagnostics, []);
    let manifest = report.manifest;
    let published = |logical: &str| {
        let catalog = AssetCatalog::load(&f.output).unwrap();
        fs::read_to_string(catalog.object_path(logical).unwrap()).unwrap()
    };
    let image = &manifest.assets["images/a.png"].public_path;
    let base = &manifest.assets["styles/base.css"];
    let app = &manifest.assets["styles/app.css"];
    assert_eq!(
        published("styles/base.css"),
        format!("b {{ background: url(\"../{image}\") }}")
    );
    assert_eq!(
        published("styles/app.css"),
        format!(
            r#"@import "{}";
a {{ background: url("../{image}?v=1#x"); mask: url("../../mutable/data.json") }}
c {{ background: url(https://cdn.example/x.png), url(data:image/png;base64,AA), url(#id) }}"#,
            base.public_path.strip_prefix("styles/").unwrap()
        )
    );
    for entry in [base, app] {
        assert_eq!(entry.cache_policy, CachePolicy::Immutable);
        assert_eq!(
            entry.sha256,
            sha256(&fs::read(f.output.join("objects").join(&entry.object)).unwrap())
        );
        assert_eq!(
            entry.integrity,
            integrity(&fs::read(f.output.join("objects").join(&entry.object)).unwrap())
        );
    }

    // A changed image renames it, which changes the bytes and so the name of
    // every stylesheet that reaches it, directly or through an import.
    f.write("images/a.png", b"other image");
    let next = f.publish().manifest;
    for logical in ["styles/base.css", "styles/app.css"] {
        assert_ne!(
            manifest.assets[logical].public_path,
            next.assets[logical].public_path
        );
    }
    // The sources are never touched.
    assert_eq!(
        fs::read(f.source.join("styles/base.css")).unwrap(),
        b"b { background: url(../images/a.png) }"
    );
}

#[test]
fn references_that_cannot_follow_the_published_file_are_reported() {
    let f = Fixture::new();
    f.write(
        "app.css",
        b"a { background: url(missing.png), url(/static/x.png), url(../outside.png) }",
    );
    f.write("broken.css", &[0xff, 0xfe]);
    let report = f.publish();
    let problems: Vec<_> = report
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.file.as_str(), diagnostic.problem.clone()))
        .collect();
    assert_eq!(
        problems,
        [
            ("app.css", Problem::Missing("missing.png".into())),
            ("app.css", Problem::RootRelative("/static/x.png".into())),
            ("app.css", Problem::Missing("../outside.png".into())),
            ("broken.css", Problem::Unreadable),
        ]
    );
    // Both are still published, unchanged.
    let catalog = AssetCatalog::load(&f.output).unwrap();
    assert_eq!(
        fs::read(catalog.object_path("app.css").unwrap()).unwrap(),
        fs::read(f.source.join("app.css")).unwrap()
    );
    assert_eq!(
        report.manifest.assets["broken.css"].cache_policy,
        CachePolicy::Immutable
    );
}

#[test]
fn a_reference_cycle_between_stylesheets_is_reported_and_left_as_written() {
    let f = Fixture::new();
    f.write("a.css", b"@import 'b.css'; a { mask: url(a.css#self) }");
    f.write("b.css", b"@import 'a.css';");
    let report = f.publish();
    let problems: Vec<_> = report
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.file.as_str(), diagnostic.problem.clone()))
        .collect();
    assert_eq!(
        problems,
        [
            ("b.css", Problem::Cycle("a.css".into())),
            ("a.css", Problem::Cycle("a.css#self".into())),
        ]
    );
    // The forward reference is still rewritten; the two back to a file whose
    // name was not settled yet are the ones left alone.
    let catalog = AssetCatalog::load(&f.output).unwrap();
    let a = fs::read_to_string(catalog.object_path("a.css").unwrap()).unwrap();
    let b = &report.manifest.assets["b.css"].public_path;
    assert_eq!(a, format!("@import \"{b}\"; a {{ mask: url(a.css#self) }}"));
    assert_eq!(
        fs::read(catalog.object_path("b.css").unwrap()).unwrap(),
        b"@import 'a.css';"
    );
}

#[test]
fn a_path_from_an_unhashed_file_to_a_hashed_one_is_reported() {
    let f = Fixture::new();
    f.write("icons/icon-192.png", b"icon");
    f.write("app_bg.wasm", b"wasm");
    f.write("vendor/lib.mjs", b"export {};");
    f.write("data.json", b"{}");
    f.write(
        "site.webmanifest",
        br#"{"name":"App","icons":[{"src":"icons/icon-192.png"}],"start_url":"/"}"#,
    );
    f.write(
        "app.js",
        b"import './vendor/lib.mjs'; fetch('data.json'); new URL('app_bg.wasm', import.meta.url); const label = 'a sentence. With a dot';",
    );
    let diagnostics = f.publish().diagnostics;
    let problems: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.file.as_str(), diagnostic.problem.clone()))
        .collect();
    // References between files that keep their names still work, and are
    // not reported.
    assert_eq!(
        problems,
        [
            ("app.js", Problem::Hashed("app_bg.wasm".into())),
            (
                "site.webmanifest",
                Problem::Hashed("icons/icon-192.png".into())
            ),
        ]
    );
    assert!(diagnostics[0].to_string().contains("cx.url("));
}

#[test]
fn read_only_sources_can_be_published_and_cleaned() {
    let f = Fixture::new();
    f.write("read-only.png", b"image");
    let path = f.source.join("read-only.png");
    let original = fs::metadata(&path).unwrap().permissions();
    let mut permissions = original.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    let manifest = f.publish().manifest;
    fs::set_permissions(&path, original).unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(f.publish().cleanup.unwrap().removed_objects, 1);
    assert!(!f
        .output
        .join("objects")
        .join(&manifest.assets["read-only.png"].object)
        .exists());
}

#[test]
fn manifest_rejects_traversal_and_unsupported_schema() {
    let f = Fixture::new();
    f.write("logo.png", b"bytes");
    let mut manifest = f.publish().manifest;
    manifest.version += 1;
    assert!(matches!(
        manifest.validate(),
        Err(AssetError::InvalidManifest(_))
    ));
    manifest.version = MANIFEST_VERSION;
    manifest.assets.get_mut("logo.png").unwrap().object = "../private".into();
    assert!(matches!(
        manifest.validate(),
        Err(AssetError::InvalidPath(_))
    ));

    manifest = f.publish().manifest;
    let mut other = manifest.assets["logo.png"].clone();
    other.sha256 = sha256(b"different");
    other.object = fingerprinted_path("logo.png", &other.sha256);
    other.public_path = manifest.assets["logo.png"].public_path.clone();
    manifest.assets.insert("other.png".into(), other);
    assert!(manifest.validate().is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_in_input_or_output_are_rejected_without_following_them() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.write("logo.png", b"bytes");
    f.publish();
    let pointer = fs::read(f.output.join(MANIFEST_FILE)).unwrap();
    symlink(f.output.join(MANIFEST_FILE), f.source.join("secret.json")).unwrap();
    assert!(matches!(
        OutputStore::open(&f.output).unwrap().publish(&f.source),
        Err(AssetError::InvalidPath(_))
    ));
    assert_eq!(pointer, fs::read(f.output.join(MANIFEST_FILE)).unwrap());
    let object = AssetCatalog::load(&f.output)
        .unwrap()
        .object_path("logo.png")
        .unwrap();
    fs::remove_file(&object).unwrap();
    symlink(f.source.join("logo.png"), object).unwrap();
    assert!(matches!(
        AssetCatalog::load(f.output),
        Err(AssetError::InvalidPath(_))
    ));
}

#[test]
fn extensionless_files_named_after_extensions_stay_immutable() {
    let f = Fixture::new();
    f.write("css", b"binary-blob");
    f.write("dir/js", b"another-blob");
    f.write(".css", b"dotfile-blob");
    let manifest = f.publish().manifest;
    for logical in ["css", "dir/js", ".css"] {
        let entry = &manifest.assets[logical];
        assert_eq!(entry.cache_policy, CachePolicy::Immutable, "{logical}");
        assert_ne!(entry.public_path, logical, "{logical}");
    }
}

#[cfg(unix)]
#[test]
fn published_catalog_files_are_world_readable_for_non_root_runtimes() {
    use std::os::unix::fs::PermissionsExt;

    let f = Fixture::new();
    f.write("images/hero.png", b"png-bytes");
    // Even if a source asset was owner-only (0600), the published object must be readable.
    fs::set_permissions(
        f.source.join("images/hero.png"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let report = f.publish();
    let object_path = f
        .output
        .join("objects")
        .join(&report.manifest.assets["images/hero.png"].object);

    for path in [
        f.output.join(MANIFEST_FILE),
        f.output.join(OWNER_FILE),
        f.output.join(READERS_FILE),
        f.output.join(LIVE_READERS_FILE),
        object_path.clone(),
    ] {
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode & 0o444,
            0o444,
            "{} must be readable by non-root users (mode {:o})",
            path.display(),
            mode
        );
    }

    // Re-publishing into an existing catalog also heals restrictive permissions on retained
    // objects, the ownership marker, and the directories leading to them.
    let directories = [
        f.output.clone(),
        f.output.join("objects"),
        f.output.join("objects/images"),
    ];
    fs::set_permissions(&object_path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(f.output.join(OWNER_FILE), fs::Permissions::from_mode(0o600)).unwrap();
    for directory in &directories {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    }
    f.publish();
    let marker_mode = fs::metadata(f.output.join(OWNER_FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(marker_mode & 0o444, 0o444, "marker mode {marker_mode:o}");
    for directory in &directories {
        let mode = fs::metadata(directory).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode & 0o755, 0o755, "{} mode {mode:o}", directory.display());
    }
    let healed_mode = fs::metadata(&object_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        healed_mode & 0o444,
        0o444,
        "retained object must be healed to world-readable on re-publish (mode {:o})",
        healed_mode
    );
}

#[test]
fn bundled_output_is_a_sibling_named_after_the_executable() {
    assert_eq!(
        bundled_output(Path::new("/srv/app")),
        Some(PathBuf::from("/srv/app.assets"))
    );
    assert_eq!(
        bundled_output(Path::new("dist/app.exe")),
        Some(PathBuf::from("dist/app.exe.assets"))
    );
    assert_eq!(bundled_output(Path::new("/")), None);
}

#[test]
fn removal_waits_for_the_last_reader() {
    let f = Fixture::new();
    f.write("a.txt", b"a");
    f.publish();
    let reader = AssetCatalog::load(&f.output).unwrap();
    assert!(!OutputStore::open(&f.output).unwrap().remove().unwrap());
    assert!(reader.object_path("a.txt").unwrap().is_file());
    drop(reader);

    // A claim keeps writers out, and dropping it leaves the output intact.
    let claim = OutputStore::open(&f.output)
        .unwrap()
        .reserve_removal()
        .unwrap()
        .unwrap();
    assert!(matches!(
        OutputStore::open(&f.output),
        Err(AssetError::WriterBusy(_))
    ));
    drop(claim);
    assert!(AssetCatalog::load(&f.output).is_ok());

    assert!(OutputStore::open(&f.output).unwrap().remove().unwrap());
    assert!(!f.output.exists());
}

fn tree(root: &Path) -> Vec<String> {
    let mut files = collect_files(root).unwrap();
    files.sort();
    files
}

#[test]
fn a_bundle_holds_every_file_at_the_path_of_its_url_and_nothing_else() {
    let f = Fixture::new();
    f.write("favicon.ico", b"icon");
    f.write("images/a.png", b"image");
    f.write("styles.css", b"a { background: url(images/a.png) }");
    f.write("data.json", b"{}");
    let bundle = f.output.with_file_name("app.assets");
    let runtime = b"export const runtime = 1;";
    let report = export(
        &bundle,
        Some(&f.source),
        &[FrameworkFile {
            name: "rsjs-runtime.js",
            path: "immutable/rsjs/runtime.0123456789abcdef.js",
            bytes: runtime,
        }],
    )
    .unwrap();
    let manifest = &report.manifest;
    assert_eq!(manifest.layout, Layout::Bundle);
    let image = manifest.assets["images/a.png"].url_path();
    let styles = manifest.assets["styles.css"].url_path();
    let icon = manifest.assets["favicon.ico"].url_path();
    let mut expected = vec![
        ".proa-assets".to_owned(),
        "manifest.json".to_owned(),
        "public/favicon.ico".to_owned(),
        format!("public/_proa/{icon}"),
        format!("public/_proa/{image}"),
        format!("public/_proa/{styles}"),
        "public/_proa/mutable/data.json".to_owned(),
        "public/_proa/immutable/rsjs/runtime.0123456789abcdef.js".to_owned(),
    ];
    expected.sort();
    assert_eq!(tree(&bundle), expected);
    // Nothing is left beside it.
    assert_eq!(
        fs::read_dir(bundle.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with(".proa-export-"))
            .count(),
        0
    );
    assert_eq!(manifest.root["favicon.ico"], "favicon.ico");
    assert_eq!(
        fs::read(bundle.join("public/favicon.ico")).unwrap(),
        b"icon"
    );

    let catalog = AssetCatalog::load(&bundle).unwrap();
    assert_eq!(catalog.manifest(), manifest);
    assert_eq!(
        catalog.object_path("images/a.png").unwrap(),
        bundle
            .canonicalize()
            .unwrap()
            .join("public/_proa")
            .join(&image)
    );
    assert_eq!(
        fs::read(catalog.framework_path("rsjs-runtime.js").unwrap()).unwrap(),
        runtime
    );
    // The stylesheet reaches the image by a path that holds in the mirror.
    let css = fs::read_to_string(catalog.object_path("styles.css").unwrap()).unwrap();
    assert!(
        css.contains(image.strip_prefix("immutable/").unwrap()),
        "{css}"
    );

    // A changed file is caught the same way as in a store.
    fs::write(
        catalog.framework_path("rsjs-runtime.js").unwrap(),
        b"tampered",
    )
    .unwrap();
    drop(catalog);
    assert!(matches!(
        AssetCatalog::load(&bundle),
        Err(AssetError::Integrity(_))
    ));
}

#[test]
fn an_export_replaces_the_previous_output_whole_or_not_at_all() {
    let f = Fixture::new();
    f.write("logo.png", b"one");
    let bundle = f.output.with_file_name("app.assets");
    let first = export(&bundle, Some(&f.source), &[]).unwrap().manifest;

    // A running server keeps the bundle it loaded.
    let reader = AssetCatalog::load(&bundle).unwrap();
    f.write("logo.png", b"two");
    assert!(matches!(
        export(&bundle, Some(&f.source), &[]),
        Err(AssetError::InUse(_))
    ));
    assert!(!remove_bundle(&bundle).unwrap());
    assert_eq!(reader.manifest(), &first);
    assert_eq!(
        fs::read(reader.object_path("logo.png").unwrap()).unwrap(),
        b"one"
    );
    drop(reader);

    // A failed export leaves it alone too.
    f.write("_proa/reserved.png", b"refused");
    assert!(export(&bundle, Some(&f.source), &[]).is_err());
    assert_eq!(AssetCatalog::load(&bundle).unwrap().manifest(), &first);
    fs::remove_dir_all(f.source.join("_proa")).unwrap();

    let second = export(&bundle, Some(&f.source), &[]).unwrap().manifest;
    assert_ne!(first, second);
    // The first build's file is gone: a bundle is one build, not a history.
    assert_eq!(
        tree(&bundle.join("public")),
        [format!("_proa/{}", second.assets["logo.png"].url_path())]
    );

    // A store an earlier release bundled is replaced the same way.
    remove_bundle(&bundle).unwrap();
    OutputStore::open(&bundle)
        .unwrap()
        .publish(&f.source)
        .unwrap();
    assert!(bundle.join("objects").exists());
    export(&bundle, Some(&f.source), &[]).unwrap();
    assert!(!bundle.join("objects").exists());
    assert_eq!(
        AssetCatalog::load(&bundle).unwrap().manifest().layout,
        Layout::Bundle
    );

    // Anything else at that path is not Proa's to replace.
    let foreign = f.output.with_file_name("data");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("keep"), b"keep").unwrap();
    assert!(matches!(
        export(&foreign, Some(&f.source), &[]),
        Err(AssetError::NotOwned(_))
    ));
    assert_eq!(fs::read(foreign.join("keep")).unwrap(), b"keep");
}

#[test]
fn published_names_under_the_reserved_directory_are_refused() {
    let f = Fixture::new();
    f.write("_proa/immutable/x.png", b"x");
    assert!(matches!(
        OutputStore::open(&f.output).unwrap().publish(&f.source),
        Err(AssetError::ReservedPath(_))
    ));
}
