#![cfg(feature = "capture")]

use std::io::Cursor;

use chrono::Utc;
use ecopaste_neo_core::clipboard::payload::{ClipboardPayload, ImagePayload, TextPayload};
use ecopaste_neo_core::clipboard::{
    persist_capture, prepare_capture, CaptureSettings, ImageStore, PreparedCapture,
};
use ecopaste_neo_core::db::{
    self,
    items::{find_item_by_id, query_items_page},
    models::{ClipboardApp, ClipboardItemQuery, ClipboardKind, ClipboardSubKind, Platform},
};
use ecopaste_neo_core::paths::{DataPaths, Environment, Storage};
use ecopaste_neo_core::writeback::WritebackGuard;
use sqlx::SqlitePool;
use tempfile::TempDir;

/// Each test owns an isolated Neo directory and the actual migrated file database.
async fn database() -> (TempDir, DataPaths, SqlitePool, ImageStore) {
    let dir = TempDir::new().unwrap();
    let paths = Storage::new(dir.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let pool = db::open(&paths).await.unwrap();
    let images = ImageStore::new(&paths);
    (dir, paths, pool, images)
}

fn text(value: &str) -> ClipboardPayload {
    ClipboardPayload::Text(TextPayload {
        text: value.to_owned(),
        html: None,
        rtf: None,
    })
}

fn source() -> ClipboardApp {
    let now = Utc::now();
    ClipboardApp {
        id: "test.source".to_owned(),
        name: "Test Source".to_owned(),
        icon_file: None,
        platform: Platform::Macos,
        created_at: now,
        updated_at: now,
    }
}

fn prepared(store: &ImageStore, payload: &ClipboardPayload) -> PreparedCapture {
    prepare_capture(
        store,
        payload,
        &CaptureSettings::default(),
        Some(source()),
        None,
    )
    .unwrap()
    .unwrap()
}

fn png() -> ImagePayload {
    let image = image::RgbaImage::from_pixel(640, 360, image::Rgba([10, 20, 30, 255]));
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    ImagePayload {
        bytes: bytes.into_inner(),
        width: 640,
        height: 360,
    }
}

#[tokio::test]
async fn text_rich_text_and_files_persist_deduplicate_and_reopen() {
    let (dir, paths, pool, images) = database().await;
    let file = dir.path().join("fixture.txt");
    std::fs::write(&file, b"fixture").unwrap();
    let payloads = [
        text("指定文字 Clipboard capture fixture"),
        ClipboardPayload::Text(TextPayload {
            text: "HTML 可检索".to_owned(),
            html: Some("<b>HTML 可检索</b>".to_owned()),
            rtf: None,
        }),
        ClipboardPayload::Text(TextPayload {
            text: "RTF 可检索".to_owned(),
            html: None,
            rtf: Some(r"{\rtf1 RTF source}".to_owned()),
        }),
        ClipboardPayload::Files(vec![file.to_str().unwrap().to_owned()]),
    ];
    let mut ids = Vec::new();
    for payload in &payloads {
        let capture = prepared(&images, payload);
        let first = persist_capture(&pool, &capture).await.unwrap();
        assert!(!first.deduplicated);
        // A second independently prepared capture must find the first persisted ID.
        let second = persist_capture(&pool, &prepared(&images, payload))
            .await
            .unwrap();
        assert!(second.deduplicated);
        assert_eq!(second.id, first.id);
        let item = find_item_by_id(&pool, &first.id).await.unwrap().unwrap();
        assert_eq!(item.content, capture.item.content);
        assert_eq!(item.source_app_id.as_deref(), Some("test.source"));
        assert_eq!(item.use_count, 2);
        ids.push(first.id);
    }
    assert_eq!(
        find_item_by_id(&pool, &ids[1])
            .await
            .unwrap()
            .unwrap()
            .sub_kind,
        Some(ClipboardSubKind::Html)
    );
    assert_eq!(
        find_item_by_id(&pool, &ids[2])
            .await
            .unwrap()
            .unwrap()
            .sub_kind,
        Some(ClipboardSubKind::Rtf)
    );
    pool.close().await;

    let reopened = db::open(&paths).await.unwrap();
    let all = query_items_page(&reopened, &ClipboardItemQuery::default())
        .await
        .unwrap();
    assert_eq!(all.total, 4);
    assert!(all
        .list
        .iter()
        .all(|item| item.source_app_name.as_deref() == Some("Test Source")));
    let query = ClipboardItemQuery {
        keyword: Some("可检索".to_owned()),
        ..Default::default()
    };
    assert_eq!(query_items_page(&reopened, &query).await.unwrap().total, 2);
    let file_item = find_item_by_id(&reopened, &ids[3]).await.unwrap().unwrap();
    assert_eq!(file_item.kind, ClipboardKind::Files);
    assert_eq!(file_item.file_types.as_deref(), Some("f"));
    reopened.close().await;
}

#[tokio::test]
async fn image_origin_and_lazy_thumbnail_survive_reopening_neo_storage() {
    let (dir, paths, pool, images) = database().await;
    let image = png();
    let capture = prepared(&images, &ClipboardPayload::Image(image.clone()));
    let event = persist_capture(&pool, &capture).await.unwrap();
    let origin = images.origin_path(&capture.item.content);
    let thumbnail = images.ensure_thumbnail(&capture.item.content).unwrap();
    assert!(origin.starts_with(paths.resources_dir()));
    assert!(thumbnail.starts_with(paths.resources_dir()));
    assert_eq!(std::fs::read(&origin).unwrap(), image.bytes);
    let thumb = image::open(&thumbnail).unwrap();
    assert!(thumb.width().max(thumb.height()) <= 300);
    let thumbnail_bytes = std::fs::read(&thumbnail).unwrap();
    pool.close().await;
    drop(images);

    let reopened_paths = Storage::new(dir.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let reopened_pool = db::open(&reopened_paths).await.unwrap();
    let reopened_images = ImageStore::new(&reopened_paths);
    let item = find_item_by_id(&reopened_pool, &event.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened_images.origin_path(&item.content), origin);
    assert_eq!(
        reopened_images.ensure_thumbnail(&item.content).unwrap(),
        thumbnail
    );
    assert_eq!(std::fs::read(&thumbnail).unwrap(), thumbnail_bytes);
    // A missing cache is regenerated from the persisted origin after restart too.
    std::fs::remove_file(&thumbnail).unwrap();
    assert!(reopened_images
        .ensure_thumbnail(&item.content)
        .unwrap()
        .exists());
    reopened_pool.close().await;
}

#[tokio::test]
async fn concurrent_manual_and_listener_captures_create_only_one_row() {
    let (_dir, _paths, pool, images) = database().await;
    let payload = text("concurrent fixture");
    let first = prepare_capture(&images, &payload, &CaptureSettings::default(), None, None)
        .unwrap()
        .unwrap();
    let second = prepare_capture(&images, &payload, &CaptureSettings::default(), None, None)
        .unwrap()
        .unwrap();
    let (a, b) = tokio::join!(
        persist_capture(&pool, &first),
        persist_capture(&pool, &second)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.id, b.id);
    assert_ne!(a.deduplicated, b.deduplicated);
    assert_eq!(
        query_items_page(&pool, &ClipboardItemQuery::default())
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        find_item_by_id(&pool, &a.id)
            .await
            .unwrap()
            .unwrap()
            .use_count,
        2
    );
    pool.close().await;
}

#[tokio::test]
async fn source_failure_preserves_content_without_a_dangling_foreign_key() {
    let (_dir, _paths, pool, images) = database().await;
    sqlx::query("CREATE TRIGGER reject_source BEFORE INSERT ON clipboard_apps BEGIN SELECT RAISE(FAIL, 'fixture source failure'); END")
        .execute(&pool).await.unwrap();
    let capture = prepared(&images, &text("source failure fixture"));
    let event = persist_capture(&pool, &capture).await.unwrap();
    let item = find_item_by_id(&pool, &event.id).await.unwrap().unwrap();
    assert_eq!(item.content, "source failure fixture");
    assert_eq!(item.source_app_id, None);
    pool.close().await;
}

#[tokio::test]
async fn exclusion_privacy_and_writeback_suppression_prevent_persistence() {
    let (_dir, paths, pool, images) = database().await;
    let payload = ClipboardPayload::Image(png());
    let mut settings = CaptureSettings::default();
    settings.excluded_app_ids.push(source().id);
    assert!(
        prepare_capture(&images, &payload, &settings, Some(source()), None)
            .unwrap()
            .is_none()
    );
    assert!(!paths.resources_dir().join("clipboard-images").exists());
    settings.excluded_app_ids.clear();
    settings.sensitive.collect_secrets = false;
    let token = text(&["ghp", "_abcdefghijklmnopqrstuvwxyzABCDE1234567890"].concat());
    assert!(prepare_capture(&images, &token, &settings, None, None)
        .unwrap()
        .is_none());
    settings.sensitive.collect_secrets = true;
    let collected = prepare_capture(&images, &token, &settings, None, None)
        .unwrap()
        .unwrap();
    let event = persist_capture(&pool, &collected).await.unwrap();
    assert!(
        find_item_by_id(&pool, &event.id)
            .await
            .unwrap()
            .unwrap()
            .is_sensitive
    );
    let guard = WritebackGuard::new();
    let payload = text("writeback fixture");
    let item = prepared(&images, &payload);
    guard.suppress(item.item.content_hash.clone());
    assert!(
        prepare_capture(&images, &payload, &settings, None, Some(&guard))
            .unwrap()
            .is_none()
    );
    assert!(
        prepare_capture(&images, &payload, &settings, None, Some(&guard))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        query_items_page(&pool, &ClipboardItemQuery::default())
            .await
            .unwrap()
            .total,
        1
    );
    pool.close().await;
}

#[test]
fn settings_keep_legacy_defaults_serialization_and_payloads_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<PreparedCapture>();
    let settings: CaptureSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings, CaptureSettings::default());
    let json = serde_json::to_value(&settings).unwrap();
    assert_eq!(json["capture"]["maxTextMb"], 4);
    assert_eq!(json["capture"]["maxImageMb"], 100);
    assert_eq!(json["sensitive"]["collectSecrets"], true);
    assert_eq!(json["sensitive"]["redactSecrets"], true);
}
