#![cfg(feature = "sqlx")]

use std::collections::HashSet;

use ecopaste_neo_core::db::items::{
    find_item_by_content_hash, find_item_by_id, find_item_for_list_by_id, query_items_page,
};
use ecopaste_neo_core::db::models::{
    ClipboardGroupFilter, ClipboardItem, ClipboardItemQuery, ClipboardItemSort, ClipboardKind,
    ClipboardSubKind,
};
use ecopaste_neo_core::db::{self, MIGRATOR};
use ecopaste_neo_core::paths::{Environment, Storage};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tempfile::TempDir;

const TIMESTAMP: &str = "2026-01-01T00:00:00Z";

/// Run the real migration on one connection so every query sees the same memory DB.
async fn memory_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .in_memory(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();

    pool
}

/// Only fixture inputs are configurable; SQL defaults fill unrelated metadata.
struct Record<'a> {
    id: &'a str,
    content: &'a str,
    search_text: &'a str,
    summary: &'a str,
    kind: ClipboardKind,
    sub_kind: Option<ClipboardSubKind>,
    note: Option<&'a str>,
    group_id: Option<&'a str>,
    source_app_id: Option<&'a str>,
    favorite: bool,
    pinned: bool,
    created_at: &'a str,
    updated_at: &'a str,
    use_count: i64,
}

impl<'a> Record<'a> {
    fn text(id: &'a str, text: &'a str) -> Self {
        Self {
            id,
            content: text,
            search_text: text,
            summary: text,
            kind: ClipboardKind::Text,
            sub_kind: None,
            note: None,
            group_id: None,
            source_app_id: None,
            favorite: false,
            pinned: false,
            created_at: TIMESTAMP,
            updated_at: TIMESTAMP,
            use_count: 1,
        }
    }

    async fn insert(&self, pool: &SqlitePool) {
        sqlx::query(
            "INSERT INTO clipboard_items \
             (id, kind, sub_kind, content, content_hash, search_text, summary, note, \
              group_id, source_app_id, is_favorite, is_pinned, platform, created_at, updated_at, use_count) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'macos', ?, ?, ?)",
        )
        .bind(self.id)
        .bind(self.kind)
        .bind(self.sub_kind)
        .bind(self.content)
        .bind(self.id)
        .bind(self.search_text)
        .bind(self.summary)
        .bind(self.note)
        .bind(self.group_id)
        .bind(self.source_app_id)
        .bind(self.favorite)
        .bind(self.pinned)
        .bind(self.created_at)
        .bind(self.updated_at)
        .bind(self.use_count)
        .execute(pool)
        .await
        .unwrap();
    }
}

fn ids(items: &[ClipboardItem]) -> Vec<&str> {
    items.iter().map(|item| item.id.as_str()).collect()
}

#[tokio::test]
async fn empty_migrated_database_returns_an_empty_page_and_missing_details() {
    let pool = memory_pool().await;
    let page = query_items_page(&pool, &ClipboardItemQuery::default())
        .await
        .unwrap();

    assert!(page.list.is_empty());
    assert_eq!(page.total, 0);
    assert!(!page.has_more);
    assert!(find_item_by_id(&pool, "missing").await.unwrap().is_none());
    assert!(find_item_for_list_by_id(&pool, "missing")
        .await
        .unwrap()
        .is_none());
    assert!(find_item_by_content_hash(&pool, "missing")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn empty_short_and_fts_keywords_search_text_and_notes() {
    let pool = memory_pool().await;
    Record::text("a", "xyz 中 中文 搜索内容")
        .insert(&pool)
        .await;
    let mut note_match = Record::text("b", "different");
    note_match.note = Some("xyz 中 中文 搜索内容");
    note_match.insert(&pool).await;
    Record::text("c", "other").insert(&pool).await;

    for keyword in [None, Some(""), Some("   ")] {
        let q = ClipboardItemQuery {
            keyword: keyword.map(str::to_owned),
            ..Default::default()
        };
        let page = query_items_page(&pool, &q).await.unwrap();
        assert_eq!(ids(&page.list), ["c", "b", "a"]);
        assert_eq!(page.total, 3);
        assert!(!page.has_more);
    }

    for keyword in [
        "x",
        "xy",
        "中",
        "中文",
        "xyz",
        "搜索内",
        "xyz 搜索内",
        "xyz 中",
        "  xyz  ",
    ] {
        let q = ClipboardItemQuery {
            keyword: Some(keyword.to_owned()),
            ..Default::default()
        };
        let page = query_items_page(&pool, &q).await.unwrap();
        assert_eq!(ids(&page.list), ["b", "a"], "keyword: {keyword}");
        assert_eq!(page.total, 2, "keyword: {keyword}");
        assert!(!page.has_more);
    }

    // Literal quoting must not turn a user keyword into FTS operators or invalid SQL.
    for keyword in ["unmatched", "xyz\"NOT\"missing"] {
        let q = ClipboardItemQuery {
            keyword: Some(keyword.to_owned()),
            ..Default::default()
        };
        let page = query_items_page(&pool, &q).await.unwrap();
        assert!(page.list.is_empty());
        assert_eq!(page.total, 0);
        assert!(!page.has_more);
    }
}

#[tokio::test]
async fn short_search_treats_like_wildcards_as_literal_characters() {
    let pool = memory_pool().await;
    Record::text("a", "literal % _ \\").insert(&pool).await;
    Record::text("b", "ordinary text").insert(&pool).await;

    for keyword in ["%", "_", "\\"] {
        let q = ClipboardItemQuery {
            keyword: Some(keyword.to_owned()),
            ..Default::default()
        };
        let page = query_items_page(&pool, &q).await.unwrap();
        assert_eq!(ids(&page.list), ["a"]);
        assert_eq!(page.total, 1);
    }
}

/// Fixtures share a keyword, allowing every filter to be checked across all search routes.
async fn filtered_pool() -> SqlitePool {
    let pool = memory_pool().await;
    for id in ["g1", "g2"] {
        sqlx::query("INSERT INTO clipboard_groups (id, name, icon, created_at, updated_at) VALUES (?, ?, 'test', ?, ?)")
            .bind(id).bind(id).bind(TIMESTAMP).bind(TIMESTAMP).execute(&pool).await.unwrap();
    }
    for (id, kind, group, favorite, pinned) in [
        ("pinned", ClipboardKind::Text, "g1", true, true),
        ("text", ClipboardKind::Text, "g1", true, false),
        ("image", ClipboardKind::Image, "g2", false, false),
        ("files", ClipboardKind::Files, "g1", false, true),
    ] {
        let mut record = Record::text(id, "xyz fixture");
        record.kind = kind;
        record.group_id = Some(group);
        record.favorite = favorite;
        record.pinned = pinned;
        record.insert(&pool).await;
    }

    pool
}

#[tokio::test]
async fn filters_and_counts_agree_with_and_without_short_or_fts_search() {
    let pool = filtered_pool().await;
    let cases: Vec<(ClipboardItemQuery, Vec<&str>)> = vec![
        (
            ClipboardItemQuery {
                kind: Some(ClipboardKind::Text),
                ..Default::default()
            },
            vec!["pinned", "text"],
        ),
        (
            ClipboardItemQuery {
                favorite: Some(true),
                ..Default::default()
            },
            vec!["pinned", "text"],
        ),
        (
            ClipboardItemQuery {
                favorite: Some(false),
                ..Default::default()
            },
            vec!["files", "image"],
        ),
        (
            ClipboardItemQuery {
                pinned: Some(true),
                ..Default::default()
            },
            vec!["pinned", "files"],
        ),
        (
            ClipboardItemQuery {
                pinned: Some(false),
                ..Default::default()
            },
            vec!["text", "image"],
        ),
        (
            ClipboardItemQuery {
                group_id: Some("g1".to_owned()),
                ..Default::default()
            },
            vec!["pinned", "files", "text"],
        ),
        (
            ClipboardItemQuery {
                kind: Some(ClipboardKind::Text),
                favorite: Some(true),
                pinned: Some(false),
                group_id: Some("g1".to_owned()),
                ..Default::default()
            },
            vec!["text"],
        ),
        (
            ClipboardItemQuery {
                group_id: Some("missing".to_owned()),
                ..Default::default()
            },
            vec![],
        ),
        (
            ClipboardItemQuery {
                group: Some(ClipboardGroupFilter::All),
                kind: Some(ClipboardKind::Image),
                favorite: Some(false),
                ..Default::default()
            },
            vec!["pinned", "files", "text", "image"],
        ),
        (
            ClipboardItemQuery {
                group: Some(ClipboardGroupFilter::Text),
                kind: Some(ClipboardKind::Image),
                favorite: Some(false),
                ..Default::default()
            },
            vec!["pinned", "text"],
        ),
        (
            ClipboardItemQuery {
                group: Some(ClipboardGroupFilter::Image),
                ..Default::default()
            },
            vec!["image"],
        ),
        (
            ClipboardItemQuery {
                group: Some(ClipboardGroupFilter::Files),
                ..Default::default()
            },
            vec!["files"],
        ),
        (
            ClipboardItemQuery {
                group: Some(ClipboardGroupFilter::Favorite),
                favorite: Some(false),
                ..Default::default()
            },
            vec!["pinned", "text"],
        ),
    ];

    for (query, expected) in cases {
        for keyword in [None, Some("x"), Some("xyz")] {
            let mut query = query.clone();
            query.keyword = keyword.map(str::to_owned);
            let page = query_items_page(&pool, &query).await.unwrap();
            assert_eq!(ids(&page.list), expected, "query: {query:?}");
            assert_eq!(page.total, expected.len() as i64);
            assert!(!page.has_more);

            query.limit = 1;
            let page = query_items_page(&pool, &query).await.unwrap();
            assert_eq!(
                ids(&page.list),
                expected.iter().copied().take(1).collect::<Vec<_>>()
            );
            assert_eq!(page.total, expected.len() as i64);
            assert_eq!(page.has_more, expected.len() > 1);
        }
    }
}

#[tokio::test]
async fn pinned_first_and_unique_tiebreaker_make_pages_stable_for_every_sort() {
    let pool = memory_pool().await;
    // Deliberately insert equal sort keys out of ID order.
    for id in ["b", "a"] {
        let mut record = Record::text(id, id);
        record.use_count = 5;
        record.updated_at = "2026-02-01T00:00:00Z";
        record.insert(&pool).await;
    }
    let mut latest_created = Record::text("c", "c");
    latest_created.created_at = "2026-03-01T00:00:00Z";
    latest_created.insert(&pool).await;
    let mut pinned = Record::text("pin", "pin");
    pinned.pinned = true;
    pinned.created_at = "2025-01-01T00:00:00Z";
    pinned.updated_at = "2025-01-01T00:00:00Z";
    pinned.insert(&pool).await;

    for (sort, expected) in [
        (ClipboardItemSort::CreatedAt, vec!["pin", "c", "b", "a"]),
        (ClipboardItemSort::UpdatedAt, vec!["pin", "b", "a", "c"]),
        (ClipboardItemSort::UseCount, vec!["pin", "b", "a", "c"]),
    ] {
        let mut seen = Vec::new();
        for offset in [0, 2, 4, 20] {
            let query = ClipboardItemQuery {
                sort,
                limit: 2,
                offset,
                ..Default::default()
            };
            let page = query_items_page(&pool, &query).await.unwrap();
            assert_eq!(page.total, 4);
            assert_eq!(page.has_more, offset == 0);
            seen.extend(page.list.into_iter().map(|item| item.id));
        }
        assert_eq!(seen, expected);
        assert_eq!(seen.iter().collect::<HashSet<_>>().len(), 4);
    }

    let query = ClipboardItemQuery {
        limit: 3,
        offset: 3,
        ..Default::default()
    };
    let page = query_items_page(&pool, &query).await.unwrap();
    assert_eq!(page.list.len(), 1);
    assert_eq!(page.total, 4);
    assert!(!page.has_more);
}

#[tokio::test]
async fn long_text_returns_summary_for_lists_and_complete_content_for_details() {
    let pool = memory_pool().await;
    sqlx::query("INSERT INTO clipboard_apps (id, name, icon_file, platform, created_at, updated_at) VALUES ('app', 'Fixture App', 'app.png', 'macos', ?, ?)")
        .bind(TIMESTAMP).bind(TIMESTAMP).execute(&pool).await.unwrap();
    let plain_text = "长文本 xyz ".repeat(5000);
    let html = format!("<p>{plain_text}</p>");
    let summary: String = plain_text.chars().take(512).collect();
    let mut record = Record::text("long", &plain_text);
    record.content = &html;
    record.summary = &summary;
    record.sub_kind = Some(ClipboardSubKind::Html);
    record.source_app_id = Some("app");
    record.note = Some("test note");
    record.insert(&pool).await;

    let page = query_items_page(&pool, &ClipboardItemQuery::default())
        .await
        .unwrap();
    let item = &page.list[0];
    assert!(item.content.is_empty());
    assert!(item.search_text.is_none());
    assert_eq!(item.summary.as_deref(), Some(summary.as_str()));
    assert_eq!(item.source_app_name.as_deref(), Some("Fixture App"));
    assert_eq!(item.source_app_icon_file.as_deref(), Some("app.png"));
    assert_eq!(item.sub_kind, Some(ClipboardSubKind::Html));
    let refreshed = find_item_for_list_by_id(&pool, "long")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(refreshed).unwrap(),
        serde_json::to_value(item).unwrap()
    );

    let complete = find_item_by_id(&pool, "long").await.unwrap().unwrap();
    assert_eq!(complete.content, html);
    assert_eq!(complete.search_text.as_deref(), Some(plain_text.as_str()));
    assert_eq!(complete.note.as_deref(), Some("test note"));
    assert_eq!(complete.source_app_id.as_deref(), Some("app"));
    assert_eq!(complete.sub_kind, Some(ClipboardSubKind::Html));
    assert_eq!(
        find_item_by_content_hash(&pool, "long")
            .await
            .unwrap()
            .unwrap()
            .content,
        html
    );

    let response = serde_json::to_value(page).unwrap();
    assert_eq!(response["total"], 1);
    assert_eq!(response["hasMore"], false);
    assert_eq!(response["list"][0]["sourceAppName"], "Fixture App");
    assert_eq!(response["list"][0]["sourceAppIconFile"], "app.png");
    assert_eq!(response["list"][0]["subKind"], "html");
    assert_eq!(response["list"][0]["content"], "");
}

#[tokio::test]
async fn image_and_file_lists_keep_their_content_references() {
    let pool = memory_pool().await;
    for (id, kind, reference) in [
        ("image", ClipboardKind::Image, "fixture.png"),
        ("files", ClipboardKind::Files, "/fixture/one\n/fixture/two"),
    ] {
        let mut record = Record::text(id, "searchable fixture");
        record.kind = kind;
        record.content = reference;
        record.insert(&pool).await;

        let item = find_item_for_list_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(item.content, reference);
        assert_eq!(item.search_text.as_deref(), Some("searchable fixture"));
    }
}

#[tokio::test]
async fn neo_file_database_reopens_with_records_and_applied_migration() {
    let temp = TempDir::new().unwrap();
    let platform = temp.path().join("platform");
    let storage = Storage::new(&platform, Environment::Dev).unwrap();
    let paths = storage.open_custom(&temp.path().join("chosen")).unwrap();
    let pool = db::open(&paths).await.unwrap();
    Record::text("saved", "persistent xyz 记录")
        .insert(&pool)
        .await;
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(mode, "wal");
    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(foreign_keys, 1);
    pool.close().await;

    let reopened_paths = Storage::new(&platform, Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    assert_eq!(reopened_paths, paths);
    let pool = db::open(&reopened_paths).await.unwrap();
    let query = ClipboardItemQuery {
        keyword: Some("xyz".to_owned()),
        ..Default::default()
    };
    let page = query_items_page(&pool, &query).await.unwrap();
    assert_eq!(ids(&page.list), ["saved"]);
    assert_eq!(page.total, 1);
    assert!(!page.has_more);
    assert_eq!(
        find_item_by_id(&pool, "saved")
            .await
            .unwrap()
            .unwrap()
            .content,
        "persistent xyz 记录"
    );
    let applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 1 AND success = 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(applied, 1);
    pool.close().await;
}
