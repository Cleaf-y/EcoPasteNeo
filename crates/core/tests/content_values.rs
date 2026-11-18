use ecopaste_neo_core::content::ClipboardSubKind;

const SUB_KIND_VALUES: [(ClipboardSubKind, &str); 6] = [
    (ClipboardSubKind::Rtf, "rtf"),
    (ClipboardSubKind::Html, "html"),
    (ClipboardSubKind::Url, "url"),
    (ClipboardSubKind::Email, "email"),
    (ClipboardSubKind::Color, "color"),
    (ClipboardSubKind::Path, "path"),
];

#[test]
fn preserves_published_json_values() {
    for (kind, value) in SUB_KIND_VALUES {
        let json = serde_json::Value::String(value.to_owned());
        assert_eq!(serde_json::to_value(kind).unwrap(), json);
        assert_eq!(
            serde_json::from_value::<ClipboardSubKind>(json).unwrap(),
            kind
        );
    }
}

#[cfg(feature = "sqlx")]
#[tokio::test]
async fn preserves_published_sqlite_values() {
    use sqlx::{Connection, SqliteConnection};

    let mut connection = SqliteConnection::connect(":memory:").await.unwrap();

    for (kind, value) in SUB_KIND_VALUES {
        let encoded: String = sqlx::query_scalar("SELECT ?")
            .bind(kind)
            .fetch_one(&mut connection)
            .await
            .unwrap();
        assert_eq!(encoded, value);

        let decoded: ClipboardSubKind = sqlx::query_scalar("SELECT ?")
            .bind(value)
            .fetch_one(&mut connection)
            .await
            .unwrap();
        assert_eq!(decoded, kind);
    }

    let absent: Option<ClipboardSubKind> = sqlx::query_scalar("SELECT ?")
        .bind(Option::<ClipboardSubKind>::None)
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(absent, None);
}
