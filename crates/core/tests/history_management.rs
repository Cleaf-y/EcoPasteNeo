#![cfg(feature = "sqlx")]

use ecopaste_neo_core::{
    db::{self, items::*, models::ClipboardItemQuery},
    paths::{Environment, Storage},
};
use sqlx::SqlitePool;

const TIME: &str = "2026-01-01T00:00:00Z";

async fn insert(pool: &SqlitePool, id: &str, kind: &str, content: &str) {
    sqlx::query("INSERT INTO clipboard_items (id,kind,content,content_hash,search_text,summary,platform,created_at,updated_at) VALUES (?,?,?,?,?,?,'macos',?,?)")
        .bind(id).bind(kind).bind(content).bind(id).bind(content).bind(content).bind(TIME).bind(TIME)
        .execute(pool).await.unwrap();
}

async fn search(pool: &SqlitePool, keyword: &str) -> Vec<String> {
    query_items_page(
        pool,
        &ClipboardItemQuery {
            keyword: Some(keyword.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .list
    .into_iter()
    .map(|item| item.id)
    .collect()
}

#[tokio::test]
async fn metadata_targets_ids_preserves_recency_updates_search_and_survives_reopen() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = Storage::new(temporary.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let pool = db::open(&paths).await.unwrap();
    insert(&pool, "a", "text", "alpha").await;
    insert(&pool, "b", "text", "beta").await;
    assert!(toggle_item_favorite(&pool, "b").await.unwrap());
    assert!(toggle_item_pinned(&pool, "b").await.unwrap());
    update_item_note(&pool, "b", Some("needle008 中文备注"))
        .await
        .unwrap();
    assert_eq!(search(&pool, "needle008").await, ["b"]);
    assert_eq!(search(&pool, "中文").await, ["b"]);
    let page = query_items_page(&pool, &ClipboardItemQuery::default())
        .await
        .unwrap();
    assert_eq!(page.list[0].id, "b");
    for id in ["a", "b"] {
        let item = find_item_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(item.updated_at.to_rfc3339(), "2026-01-01T00:00:00+00:00");
        assert_eq!(item.created_at, item.updated_at);
        assert_eq!(item.is_favorite, id == "b");
        assert_eq!(item.is_pinned, id == "b");
        assert_eq!(
            item.note.as_deref(),
            (id == "b").then_some("needle008 中文备注")
        );
    }
    pool.close().await;
    let pool = db::open(&paths).await.unwrap();
    let b = find_item_by_id(&pool, "b").await.unwrap().unwrap();
    assert!(b.is_favorite && b.is_pinned);
    assert_eq!(search(&pool, "needle008").await, ["b"]);
    assert!(!toggle_item_favorite(&pool, "b").await.unwrap());
    assert!(!toggle_item_pinned(&pool, "b").await.unwrap());
    update_item_note(&pool, "b", None).await.unwrap();
    assert!(search(&pool, "needle008").await.is_empty());
    assert!(search(&pool, "中文").await.is_empty());
    assert!(toggle_item_favorite(&pool, "missing").await.is_err());
    assert!(toggle_item_pinned(&pool, "missing").await.is_err());
    pool.close().await;
    let pool = db::open(&paths).await.unwrap();
    let b = find_item_by_id(&pool, "b").await.unwrap().unwrap();
    assert!(!b.is_favorite && !b.is_pinned && b.note.is_none());
    assert_eq!(b.updated_at.to_rfc3339(), "2026-01-01T00:00:00+00:00");
    pool.close().await;
}

#[tokio::test]
async fn single_delete_returns_only_image_resources_and_never_removes_user_files() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = Storage::new(temporary.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let pool = db::open(&paths).await.unwrap();
    let user_file = temporary.path().join("keep.txt");
    std::fs::write(&user_file, b"keep me").unwrap();
    insert(&pool, "a", "text", "alpha").await;
    insert(&pool, "b", "text", "delete needle008").await;
    insert(&pool, "files", "files", user_file.to_str().unwrap()).await;
    let image_name = format!("{}.png", "ab".repeat(32));
    insert(&pool, "image", "image", &image_name).await;
    assert_eq!(delete_item(&pool, "b").await.unwrap(), None);
    assert!(search(&pool, "needle008").await.is_empty());
    assert!(find_item_by_id(&pool, "a").await.unwrap().is_some());
    assert_eq!(delete_item(&pool, "files").await.unwrap(), None);
    assert_eq!(std::fs::read(user_file).unwrap(), b"keep me");
    assert_eq!(delete_item(&pool, "image").await.unwrap(), Some(image_name));
    assert_eq!(delete_item(&pool, "missing").await.unwrap(), None);
    pool.close().await;
    let pool = db::open(&paths).await.unwrap();
    let page = query_items_page(&pool, &ClipboardItemQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.list[0].id, "a");
    pool.close().await;
}
