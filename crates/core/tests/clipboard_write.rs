#![cfg(feature = "capture")]

use ecopaste_neo_core::{
    clipboard::{
        payload::{ClipboardPayload, ImagePayload, TextPayload},
        prepare_capture,
        write::{should_write_plain_for_paste, PreparedWrite, WritePayload},
        CaptureSettings, ImageStore,
    },
    db::{self, items::find_item_by_id, models::ClipboardKind},
    paths::{Environment, Storage},
    writeback::WritebackGuard,
};

/// Roundtrip through the actual ingest rules without reading or writing a pasteboard.
#[test]
fn rich_and_plain_writes_suppress_the_selected_representation_only() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Storage::new(dir.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let store = ImageStore::new(&paths);
    let payloads = [
        TextPayload {
            text: "  完整文本\n尾行  ".into(),
            html: None,
            rtf: None,
        },
        TextPayload {
            text: "HTML fallback".into(),
            html: Some("<b>HTML fallback</b>".into()),
            rtf: None,
        },
        TextPayload {
            text: "RTF fallback".into(),
            html: None,
            rtf: Some(r"{\rtf1 RTF fallback}".into()),
        },
    ];
    for payload in payloads {
        let item = prepare_capture(
            &store,
            &ClipboardPayload::Text(payload),
            &CaptureSettings::default(),
            None,
            None,
        )
        .unwrap()
        .unwrap()
        .item;
        for plain in [false, true] {
            let plan = PreparedWrite::new(&store, &item, plain).unwrap();
            let WritePayload::Text { plain: text, rich } = &plan.payload else {
                panic!()
            };
            if plain {
                assert!(rich.is_none());
                assert_eq!(text, item.search_text.as_ref().unwrap());
            }
            let feedback = ClipboardPayload::Text(TextPayload {
                text: text.clone(),
                html: rich
                    .as_ref()
                    .filter(|(kind, _)| *kind == ecopaste_neo_core::content::ClipboardSubKind::Html)
                    .map(|(_, text)| text.clone()),
                rtf: rich
                    .as_ref()
                    .filter(|(kind, _)| *kind == ecopaste_neo_core::content::ClipboardSubKind::Rtf)
                    .map(|(_, text)| text.clone()),
            });
            let guard = WritebackGuard::new();
            plan.suppress(&guard);
            assert!(prepare_capture(
                &store,
                &feedback,
                &CaptureSettings::default(),
                None,
                Some(&guard)
            )
            .unwrap()
            .is_none());
            let external = ClipboardPayload::Text(TextPayload {
                text: "紧接着的外部复制".into(),
                html: None,
                rtf: None,
            });
            assert!(prepare_capture(
                &store,
                &external,
                &CaptureSettings::default(),
                None,
                Some(&guard)
            )
            .unwrap()
            .is_some());
            // The plain fallback is independently suppressible if rich capture is disabled.
            if rich.is_some() {
                let mut settings = CaptureSettings::default();
                settings.capture.html = false;
                settings.capture.rtf = false;
                assert!(
                    prepare_capture(&store, &feedback, &settings, None, Some(&guard))
                        .unwrap()
                        .is_none()
                );
            }
        }
    }
}

#[tokio::test]
async fn reopened_detail_writes_full_content_instead_of_summary() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Storage::new(dir.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let store = ImageStore::new(&paths);
    let full = format!("开始{}\n结尾", "长文本".repeat(300));
    let payload = ClipboardPayload::Text(TextPayload {
        text: full.clone(),
        html: None,
        rtf: None,
    });
    let capture = prepare_capture(&store, &payload, &CaptureSettings::default(), None, None)
        .unwrap()
        .unwrap();
    let pool = db::open(&paths).await.unwrap();
    let update = ecopaste_neo_core::clipboard::persist_capture(&pool, &capture)
        .await
        .unwrap();
    pool.close().await;
    let pool = db::open(&paths).await.unwrap();
    let detail = find_item_by_id(&pool, &update.id).await.unwrap().unwrap();
    assert!(detail.summary.as_ref().unwrap().len() < full.len());
    let plan = PreparedWrite::new(&store, &detail, false).unwrap();
    assert_eq!(
        plan.payload,
        WritePayload::Text {
            plain: full,
            rich: None
        }
    );
    pool.close().await;
}

#[test]
fn files_image_and_invalid_resource_names_are_prepared_without_native_access() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Storage::new(dir.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let store = ImageStore::new(&paths);
    let file = dir.path().join("fixture.txt");
    std::fs::write(&file, b"fixture").unwrap();
    let payload = ClipboardPayload::Files(vec![file.to_string_lossy().into_owned()]);
    let capture = prepare_capture(&store, &payload, &CaptureSettings::default(), None, None)
        .unwrap()
        .unwrap();
    assert!(matches!(
        PreparedWrite::new(&store, &capture.item, false)
            .unwrap()
            .payload,
        WritePayload::Files(_)
    ));
    let plan = PreparedWrite::new(&store, &capture.item, true).unwrap();
    let WritePayload::Text { plain, rich: None } = plan.payload else {
        panic!()
    };
    let guard = WritebackGuard::new();
    PreparedWrite::new(&store, &capture.item, true)
        .unwrap()
        .suppress(&guard);
    let feedback = ClipboardPayload::Text(TextPayload {
        text: plain,
        html: None,
        rtf: None,
    });
    assert!(prepare_capture(
        &store,
        &feedback,
        &CaptureSettings::default(),
        None,
        Some(&guard)
    )
    .unwrap()
    .is_none());

    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        2,
        3,
        image::Rgba([1, 2, 3, 255]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    let bytes = bytes.into_inner();
    let payload = ClipboardPayload::Image(ImagePayload {
        bytes: bytes.clone(),
        width: 2,
        height: 3,
    });
    let mut item = prepare_capture(&store, &payload, &CaptureSettings::default(), None, None)
        .unwrap()
        .unwrap()
        .item;
    let plan = PreparedWrite::new(&store, &item, false).unwrap();
    assert_eq!(plan.payload, WritePayload::Image(bytes));
    plan.suppress(&guard);
    assert!(prepare_capture(
        &store,
        &payload,
        &CaptureSettings::default(),
        None,
        Some(&guard)
    )
    .unwrap()
    .is_none());
    for name in [
        "../outside.png".to_owned(),
        format!("{}.png", "中".repeat(21) + "a"),
        format!("{}.png", "a".repeat(64)),
    ] {
        item.content = name;
        assert!(PreparedWrite::new(&store, &item, false).is_err());
    }
    for kind in [
        ClipboardKind::Text,
        ClipboardKind::Files,
        ClipboardKind::Image,
    ] {
        assert!(should_write_plain_for_paste(true, kind, false, false));
        assert_eq!(
            should_write_plain_for_paste(false, kind, true, false),
            kind == ClipboardKind::Text
        );
        assert_eq!(
            should_write_plain_for_paste(false, kind, false, true),
            kind == ClipboardKind::Files
        );
    }
}
