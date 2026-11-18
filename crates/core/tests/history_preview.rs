#![cfg(feature = "preview")]

use ecopaste_neo_core::{
    paths::{Environment, Storage},
    preview::{file_previews, image_thumbnail},
};

#[test]
fn thumbnails_are_small_cached_and_confined_to_resources() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = Storage::new(temporary.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let name = format!("{}.png", "ab".repeat(32));
    let origin = paths
        .resources_dir()
        .join("clipboard-images/origin/ab")
        .join(&name);
    std::fs::create_dir_all(origin.parent().unwrap()).unwrap();
    image::RgbImage::new(1200, 600).save(&origin).unwrap();
    let thumbnail = image_thumbnail(&paths, &name).unwrap();
    assert_eq!(image::image_dimensions(&thumbnail).unwrap(), (300, 150));
    let modified = std::fs::metadata(&thumbnail).unwrap().modified().unwrap();
    assert_eq!(
        image_thumbnail(&paths, &name).as_deref(),
        Some(thumbnail.as_path())
    );
    assert_eq!(
        std::fs::metadata(&thumbnail).unwrap().modified().unwrap(),
        modified
    );
    for invalid in ["../outside.png", "/tmp/image.png", "é.png", "ab.png"] {
        assert!(image_thumbnail(&paths, invalid).is_none());
    }
    assert!(image_thumbnail(&paths, &format!("{}.png", "cd".repeat(32))).is_none());
}

#[test]
fn file_cards_keep_missing_entries_and_stored_directory_hints() {
    let temporary = tempfile::tempdir().unwrap();
    let file = temporary.path().join("sample.txt");
    std::fs::write(&file, b"sample").unwrap();
    let missing = temporary.path().join("gone");
    let content = format!(
        "{}\n{}\n{}",
        file.display(),
        temporary.path().display(),
        missing.display()
    );
    let rows = file_previews(&content, Some("f,d,d"), 3);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].name, "sample.txt");
    assert_eq!(rows[0].size, Some(6));
    assert!(rows[1].is_directory);
    assert!(!rows[2].exists);
    assert!(rows[2].is_directory);
    assert_eq!(file_previews(&content, None, 1).len(), 1);
}

#[cfg(unix)]
#[test]
fn image_symlinks_do_not_read_external_files() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = Storage::new(temporary.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let external = temporary.path().join("external.png");
    image::RgbImage::new(2, 2).save(&external).unwrap();
    let name = format!("{}.png", "ab".repeat(32));
    let origin = paths
        .resources_dir()
        .join("clipboard-images/origin/ab")
        .join(&name);
    std::fs::create_dir_all(origin.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&external, origin).unwrap();
    assert!(image_thumbnail(&paths, &name).is_none());
}
