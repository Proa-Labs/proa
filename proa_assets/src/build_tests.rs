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
fn live_catalog_clones_protect_objects_until_the_last_reader_drops() {
    let f = Fixture::new();
    f.write("movie.mp4", b"old movie");
    f.publish();
    let old = AssetCatalog::load(&f.output).unwrap();
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
    let other_digest = format!("{}{}", &digest[..8], "0".repeat(56));
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
fn history_keeps_only_public_fingerprint_reservations() {
    let f = Fixture::new();
    f.write("icon.svg", b"old image");
    f.write("style.css", b"old css");
    f.write("main.mjs", b"old js");
    let first = f.publish().manifest;
    let mut writer = OutputStore::open(&f.output).unwrap();
    let mut legacy = writer.history().unwrap();
    for logical in ["style.css", "main.mjs"] {
        let asset = &first.assets[logical];
        legacy
            .assignments
            .insert(asset.object.clone(), asset.sha256.clone());
    }
    fs::write(
        f.output.join(HISTORY_FILE),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();

    f.write("icon.svg", b"new image");
    f.write("style.css", b"new css");
    f.write("main.mjs", b"new js");
    let second = writer.publish(&f.source).unwrap().manifest;
    assert_eq!(
        writer.history().unwrap().assignments,
        [&first, &second]
            .into_iter()
            .map(|manifest| {
                let icon = &manifest.assets["icon.svg"];
                (icon.public_path.clone(), icon.sha256.clone())
            })
            .collect()
    );
    for logical in ["style.css", "main.mjs"] {
        assert_eq!(second.assets[logical].public_path, logical);
        assert_ne!(first.assets[logical].object, second.assets[logical].object);
        assert!(!f
            .output
            .join("objects")
            .join(&first.assets[logical].object)
            .exists());
    }
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
        .get("two.png")
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
fn fingerprinted_fonts_keep_names_and_url_bearing_formats_revalidate() {
    let f = Fixture::new();
    let font = format!("fonts/inter.{}.woff2", &sha256(b"font")[..8]);
    f.write(&font, b"font");
    f.write("scripts/main.js", b"import './dependency.js';");
    f.write(
        "styles/app.css",
        b"a { background: url('../images/a.png'); }",
    );
    let manifest = f.publish().manifest;
    assert_eq!(manifest.assets[&font].public_path, font);
    assert_eq!(manifest.assets[&font].cache_policy, CachePolicy::Immutable);
    for name in ["scripts/main.js", "styles/app.css"] {
        assert_eq!(manifest.assets[name].public_path, name);
        assert_eq!(manifest.assets[name].cache_policy, CachePolicy::Revalidate);
        assert_ne!(manifest.assets[name].object, name);
    }
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
fn manifest_rejects_traversal_unsupported_schema_and_alias_collisions() {
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

    let valid = f.publish().manifest;
    f.write(&valid.assets["logo.png"].object, b"different");
    assert!(matches!(
        OutputStore::open(&f.output).unwrap().publish(&f.source),
        Err(AssetError::Collision { .. })
    ));
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
