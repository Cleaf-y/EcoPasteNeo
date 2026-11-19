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

/// The same synthetic payload is blocked only for a matching copied-from identifier.
#[tokio::test]
async fn saved_exclusions_gate_sources_and_removal_applies_to_next_capture() {
    use ecopaste_neo_core::settings::{SettingsChange, SettingsStore};
    let (_dir, paths, pool, images) = database().await;
    let store = SettingsStore::open(&paths);
    store
        .update(SettingsChange::AddExcludedApp(source().id))
        .unwrap();
    let payload = text("EXCLUDED12-SAME-PAYLOAD");
    let settings = store.capture_settings().unwrap();
    assert!(
        prepare_capture(&images, &payload, &settings, Some(source()), None)
            .unwrap()
            .is_none()
    );
    let mut browser = source();
    browser.id = "test.browser".into();
    // Identical display names must not cause a match.
    let prepared = prepare_capture(&images, &payload, &settings, Some(browser.clone()), None)
        .unwrap()
        .unwrap();
    let update = persist_capture(&pool, &prepared).await.unwrap();
    assert_eq!(
        find_item_by_id(&pool, &update.id)
            .await
            .unwrap()
            .unwrap()
            .source_app_id,
        Some(browser.id)
    );
    assert!(prepare_capture(&images, &payload, &settings, None, None)
        .unwrap()
        .is_some());
    let reopened = SettingsStore::open(&paths);
    assert!(!reopened
        .capture_settings()
        .unwrap()
        .accepts_source(Some(&source())));
    reopened
        .update(SettingsChange::RemoveExcludedApp(source().id))
        .unwrap();
    let settings = reopened.capture_settings().unwrap();
    assert!(
        prepare_capture(&images, &payload, &settings, Some(source()), None)
            .unwrap()
            .is_some()
    );
    // Exclusions never delete an application's existing history.
    reopened
        .update(SettingsChange::AddExcludedApp("test.browser".into()))
        .unwrap();
    assert!(find_item_by_id(&pool, &update.id).await.unwrap().is_some());
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

/// Synthetic payloads exercise the persisted live choices without the system clipboard.
#[tokio::test]
async fn saved_switches_gate_all_five_formats_and_rich_text_falls_back() {
    use ecopaste_neo_core::{
        clipboard::settings::CaptureKind,
        settings::{SettingsChange, SettingsStore},
    };
    let (_dir, paths, pool, images) = database().await;
    let store = SettingsStore::open(&paths);
    let formats = [
        (CaptureKind::Text, text("CONTROL11-TEXT")),
        (
            CaptureKind::Html,
            ClipboardPayload::Text(TextPayload {
                text: "CONTROL11-HTML".into(),
                html: Some("<b>CONTROL11-HTML</b>".into()),
                rtf: None,
            }),
        ),
        (
            CaptureKind::Rtf,
            ClipboardPayload::Text(TextPayload {
                text: "CONTROL11-RTF".into(),
                html: None,
                rtf: Some(r"{\rtf1 CONTROL11-RTF}".into()),
            }),
        ),
        (CaptureKind::Image, ClipboardPayload::Image(png())),
        (
            CaptureKind::Files,
            ClipboardPayload::Files(vec!["/tmp/CONTROL11-FILE".into()]),
        ),
    ];
    for (kind, payload) in &formats {
        for candidate in [
            CaptureKind::Text,
            CaptureKind::Html,
            CaptureKind::Rtf,
            CaptureKind::Image,
            CaptureKind::Files,
        ] {
            store
                .update(SettingsChange::CaptureType(candidate, candidate == *kind))
                .unwrap();
        }
        let settings = store.capture_settings().unwrap();
        let prepared = prepare_capture(&images, payload, &settings, None, None)
            .unwrap()
            .unwrap();
        persist_capture(&pool, &prepared).await.unwrap();
        store
            .update(SettingsChange::CaptureType(*kind, false))
            .unwrap();
        let disabled = store.capture_settings().unwrap();
        assert!(prepare_capture(&images, payload, &disabled, None, None)
            .unwrap()
            .is_none());
    }
    assert_eq!(
        query_items_page(&pool, &ClipboardItemQuery::default())
            .await
            .unwrap()
            .total,
        5
    );
    store
        .update(SettingsChange::CaptureType(CaptureKind::Text, true))
        .unwrap();
    let settings = store.capture_settings().unwrap();
    let fallback = prepare_capture(&images, &formats[1].1, &settings, None, None)
        .unwrap()
        .unwrap();
    assert_eq!(fallback.item.kind, ClipboardKind::Text);
    assert_ne!(fallback.item.sub_kind, Some(ClipboardSubKind::Html));
    assert_eq!(fallback.item.content, "CONTROL11-HTML");
    store.update(SettingsChange::CaptureEnabled(false)).unwrap();
    assert!(store.capture_settings().is_none());
    let reopened = SettingsStore::open(&paths);
    assert!(reopened.capture_settings().is_none());
    reopened
        .update(SettingsChange::CaptureEnabled(true))
        .unwrap();
    assert!(reopened.capture_settings().unwrap().capture.text);
    assert!(!reopened.capture_settings().unwrap().capture.image);
    pool.close().await;
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

#[cfg(feature = "preview")]
/// Synthetic labeled values exercise detection without any provider credential.
#[tokio::test]
async fn saved_sensitive_combinations_keep_originals_and_respect_source_exclusion() {
    use ecopaste_neo_core::{
        preview::{record_content, PreviewContent},
        settings::{SettingsChange, SettingsStore},
    };
    let (_dir, paths, pool, images) = database().await;
    let store = SettingsStore::open(&paths);
    let mut saved = Vec::new();
    for collect in [false, true] {
        for redact in [false, true] {
            store
                .update(SettingsChange::CollectSecrets(collect))
                .unwrap();
            store.update(SettingsChange::RedactSecrets(redact)).unwrap();
            let value = format!(
                "client_secret = dummy_without_permissions_{}_{}",
                collect, redact
            );
            let payload = text(&value);
            let settings = store.capture_settings().unwrap();
            let prepared =
                prepare_capture(&images, &payload, &settings, Some(source()), None).unwrap();
            if collect {
                let prepared = prepared.unwrap();
                let update = persist_capture(&pool, &prepared).await.unwrap();
                saved.push(update.id);
            } else {
                assert!(prepared.is_none());
            }
            for id in &saved {
                let item = find_item_by_id(&pool, id).await.unwrap().unwrap();
                assert!(item.is_sensitive);
                if redact {
                    assert!(matches!(
                        record_content(&paths, &item, redact),
                        PreviewContent::Hidden
                    ));
                } else {
                    assert!(
                        matches!(record_content(&paths, &item, redact), PreviewContent::Text(value) if value == item.content)
                    );
                }
            }
            let ordinary = prepare_capture(
                &images,
                &text("ordinary harmless note"),
                &settings,
                Some(source()),
                None,
            )
            .unwrap()
            .unwrap();
            let update = persist_capture(&pool, &ordinary).await.unwrap();
            let item = find_item_by_id(&pool, &update.id).await.unwrap().unwrap();
            assert!(!item.is_sensitive);
            assert!(matches!(
                record_content(&paths, &item, redact),
                PreviewContent::Text(_)
            ));
        }
    }
    store.update(SettingsChange::CollectSecrets(true)).unwrap();
    store
        .update(SettingsChange::AddExcludedApp(source().id))
        .unwrap();
    let settings = store.capture_settings().unwrap();
    let payload = text("client_secret = dummy_without_permissions_excluded");
    assert!(
        prepare_capture(&images, &payload, &settings, Some(source()), None)
            .unwrap()
            .is_none()
    );
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM clipboard_items WHERE is_sensitive = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 2);
}
