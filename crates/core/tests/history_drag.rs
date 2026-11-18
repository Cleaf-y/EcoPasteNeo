#![cfg(all(feature = "preview", feature = "sqlx"))]

use ecopaste_neo_core::{
    db::{self, items::find_item_by_id},
    paths::{Environment, Storage},
    preview::{self, DragFilesError},
};

async fn record(
    pool: &sqlx::SqlitePool,
    id: &str,
    kind: &str,
    content: &str,
) -> ecopaste_neo_core::db::models::ClipboardItem {
    sqlx::query("INSERT INTO clipboard_items (id,kind,content,content_hash,platform,created_at,updated_at) VALUES (?,?,?,?,'macos','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
        .bind(id).bind(kind).bind(content).bind(id).execute(pool).await.unwrap();
    find_item_by_id(pool, id).await.unwrap().unwrap()
}

#[tokio::test]
async fn file_drag_rechecks_all_references_and_rejects_empty_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let paths = Storage::new(temp.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let pool = db::open(&paths).await.unwrap();
    let first = temp.path().join("first 中文.txt");
    let second = temp.path().join("second.txt");
    let missing = temp.path().join("missing.txt");
    std::fs::write(&first, b"first").unwrap();
    std::fs::write(&second, b"second").unwrap();
    let content = format!(
        "{}\n{}\n{}",
        first.display(),
        missing.display(),
        second.display()
    );
    let mut item = record(&pool, "files", "files", &content).await;
    assert_eq!(
        preview::drag_files(&paths, &item).unwrap(),
        [first.clone(), second.clone()]
    );
    std::fs::rename(&first, temp.path().join("moved-first.txt")).unwrap();
    assert_eq!(
        preview::drag_files(&paths, &item).unwrap(),
        std::slice::from_ref(&second)
    );
    std::fs::rename(&second, temp.path().join("moved-second.txt")).unwrap();
    assert!(matches!(
        preview::drag_files(&paths, &item),
        Err(DragFilesError::FilesMissing)
    ));
    item.is_sensitive = true;
    assert!(matches!(
        preview::drag_files(&paths, &item),
        Err(DragFilesError::Sensitive)
    ));
    let text = record(&pool, "text", "text", "not a file drag").await;
    assert!(matches!(
        preview::drag_files(&paths, &text),
        Err(DragFilesError::Unsupported)
    ));
    pool.close().await;
}

#[tokio::test]
async fn image_drag_resolves_only_original_and_never_generates_or_falls_back_to_thumbnail() {
    let temp = tempfile::tempdir().unwrap();
    let paths = Storage::new(temp.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let pool = db::open(&paths).await.unwrap();
    let name = format!("{}.png", "ab".repeat(32));
    let original = paths
        .resources_dir()
        .join("clipboard-images/origin/ab")
        .join(&name);
    let thumbnail = paths
        .resources_dir()
        .join("clipboard-images/thumbnails/ab")
        .join(&name);
    std::fs::create_dir_all(original.parent().unwrap()).unwrap();
    image::RgbImage::new(1600, 900).save(&original).unwrap();
    let mut item = record(&pool, "image", "image", &name).await;
    assert_eq!(
        preview::drag_files(&paths, &item).unwrap(),
        std::slice::from_ref(&original)
    );
    assert!(!thumbnail.exists());
    preview::image_thumbnail(&paths, &name).unwrap();
    std::fs::remove_file(&original).unwrap();
    assert!(thumbnail.exists());
    assert!(matches!(
        preview::drag_files(&paths, &item),
        Err(DragFilesError::ImageMissing)
    ));
    item.content = "../../external.png".into();
    assert!(matches!(
        preview::drag_files(&paths, &item),
        Err(DragFilesError::ImageMissing)
    ));
    #[cfg(unix)]
    {
        let external = temp.path().join("external.png");
        image::RgbImage::new(20, 10).save(&external).unwrap();
        std::os::unix::fs::symlink(external, &original).unwrap();
        item.content = name;
        assert!(matches!(
            preview::drag_files(&paths, &item),
            Err(DragFilesError::ImageMissing)
        ));
    }
    pool.close().await;
}
