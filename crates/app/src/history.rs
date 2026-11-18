use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use ecopaste_neo_core::{
    db::{
        self,
        models::{ClipboardItem, ClipboardItemPage, ClipboardItemQuery, ClipboardKind},
    },
    paths::{DataPaths, Storage},
    preview::{self, FilePreview},
};
use sqlx::SqlitePool;
use tokio::runtime::Runtime;

use crate::options::Options;

pub const PAGE_SIZE: i64 = 50;

/// Current Neo pool and its resource paths live together for the app session.
#[derive(Clone)]
pub struct HistorySource {
    runtime: Arc<Runtime>,
    pool: SqlitePool,
    paths: DataPaths,
}

#[derive(Debug)]
pub struct HistoryRow {
    pub item: ClipboardItem,
    pub thumbnail: Option<PathBuf>,
    pub files: Vec<FilePreview>,
    pub file_count: usize,
}

pub struct HistoryPage {
    pub rows: Vec<HistoryRow>,
    pub total: i64,
    pub has_more: bool,
}

impl HistorySource {
    /// Validate storage identity before opening a database or touching resources.
    pub fn open(options: &Options) -> Result<Self> {
        let paths = Storage::new(&options.data_parent, options.environment)?.open()?;
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?,
        );
        let pool = runtime.block_on(db::open(&paths))?;
        Ok(Self {
            runtime,
            pool,
            paths,
        })
    }

    pub fn paths(&self) -> &DataPaths {
        &self.paths
    }

    /// SQL and filesystem preparation run outside the UI executor.
    pub fn query(
        &self,
        keyword: String,
        offset: i64,
    ) -> tokio::task::JoinHandle<Result<HistoryPage>> {
        let pool = self.pool.clone();
        let paths = self.paths.clone();
        self.runtime.spawn(async move {
            let query = ClipboardItemQuery {
                keyword: Some(keyword),
                offset,
                limit: PAGE_SIZE,
                ..Default::default()
            };
            let ClipboardItemPage {
                list,
                total,
                has_more,
            } = db::items::query_items_page(&pool, &query).await?;
            let rows = tokio::task::spawn_blocking(move || {
                list.into_iter()
                    .map(|item| {
                        let thumbnail = (item.kind == ClipboardKind::Image)
                            .then(|| preview::image_thumbnail(&paths, &item.content))
                            .flatten();
                        let (files, file_count) = if item.kind == ClipboardKind::Files {
                            (
                                preview::file_previews(
                                    &item.content,
                                    item.file_types.as_deref(),
                                    3,
                                ),
                                item.content.lines().filter(|line| !line.is_empty()).count(),
                            )
                        } else {
                            (Vec::new(), 0)
                        };
                        HistoryRow {
                            item,
                            thumbnail,
                            files,
                            file_count,
                        }
                    })
                    .collect()
            })
            .await?;
            Ok(HistoryPage {
                rows,
                total,
                has_more,
            })
        })
    }
}

/// A request belongs to exactly one search and one contiguous page position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    generation: u64,
    offset: usize,
}

/// Pagination owns acceptance rules so delayed responses cannot replace a new search.
#[derive(Default)]
pub struct HistoryState {
    pub rows: Vec<HistoryRow>,
    pub keyword: String,
    pub total: i64,
    pub has_more: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub selected: usize,
    generation: u64,
}

impl HistoryState {
    /// Invalidate every pending response before starting a fresh search.
    pub fn reset(&mut self, keyword: String) {
        self.generation = self.generation.wrapping_add(1);
        self.keyword = keyword;
        self.rows.clear();
        self.total = 0;
        self.selected = 0;
        self.has_more = true;
        self.loading = false;
        self.error = None;
    }

    /// Reserve the next contiguous page; loading and exhausted searches cannot overlap.
    pub fn begin(&mut self) -> Option<Request> {
        if self.loading || !self.has_more {
            return None;
        }
        self.loading = true;
        self.error = None;
        Some(Request {
            generation: self.generation,
            offset: self.rows.len(),
        })
    }

    pub fn offset(request: Request) -> i64 {
        request.offset as i64
    }

    /// Return false for stale responses, including failures from an older keyword.
    pub fn complete(&mut self, request: Request, result: Result<HistoryPage>) -> bool {
        if request.generation != self.generation
            || request.offset != self.rows.len()
            || !self.loading
        {
            return false;
        }
        self.loading = false;
        match result {
            Ok(page) => {
                self.total = page.total;
                self.has_more = page.has_more && !page.rows.is_empty();
                self.rows.extend(page.rows);
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(total: i64) -> Result<HistoryPage> {
        Ok(HistoryPage {
            rows: vec![],
            total,
            has_more: false,
        })
    }

    #[test]
    fn delayed_success_and_failure_cannot_replace_a_new_search() {
        let mut state = HistoryState::default();
        state.reset("first".into());
        let old = state.begin().unwrap();
        state.reset("second".into());
        let current = state.begin().unwrap();
        assert!(!state.complete(old, page(1000)));
        assert!(!state.complete(old, Err(anyhow::anyhow!("old error"))));
        assert!(state.loading);
        assert_eq!(state.total, 0);
        assert!(state.error.is_none());
        assert!(state.complete(current, page(7)));
        assert_eq!(state.total, 7);
        assert!(!state.loading);
        assert!(state.begin().is_none());
    }

    #[test]
    fn only_one_page_is_pending_and_failed_page_can_retry() {
        let mut state = HistoryState::default();
        state.reset(String::new());
        let first = state.begin().unwrap();
        assert!(state.begin().is_none());
        assert!(state.complete(first, Err(anyhow::anyhow!("database unavailable"))));
        assert_eq!(state.error.as_deref(), Some("database unavailable"));
        let retry = state.begin().unwrap();
        assert_eq!(HistoryState::offset(retry), 0);
        assert!(state.error.is_none());
        assert!(state.complete(retry, page(0)));
        assert!(!state.complete(retry, page(10)));
    }
}

#[cfg(test)]
mod database_tests {
    use super::*;
    use crate::locale::Locale;
    use ecopaste_neo_core::paths::Environment;

    #[test]
    fn real_pages_search_and_reopen_share_the_same_neo_database() {
        let temporary = tempfile::tempdir().unwrap();
        let options = Options {
            data_parent: temporary.path().to_path_buf(),
            environment: Environment::Dev,
            locale: Locale::ZhCn,
            show: false,
        };
        let source = HistorySource::open(&options).unwrap();
        source.runtime.block_on(async {
            for index in 0..55 {
                sqlx::query("INSERT INTO clipboard_items (id,kind,content,content_hash,search_text,summary,platform,created_at,updated_at) VALUES (?,'text',?,?,?,?,'macos','2026-11-18T00:00:00Z','2026-11-18T00:00:00Z')")
                    .bind(format!("{index:04}")).bind(format!("text {index}")).bind(format!("hash {index}")).bind(format!("search {index}")).bind(format!("summary {index}"))
                    .execute(&source.pool).await.unwrap();
            }
        });
        let mut state = HistoryState::default();
        state.reset(String::new());
        let first = state.begin().unwrap();
        let page = source
            .runtime
            .block_on(source.query(String::new(), 0))
            .unwrap()
            .unwrap();
        assert_eq!(page.rows.len(), 50);
        assert!(page.rows.iter().all(|row| row.item.content.is_empty()));
        assert!(state.complete(first, Ok(page)));
        let second = state.begin().unwrap();
        assert_eq!(HistoryState::offset(second), 50);
        let page = source
            .runtime
            .block_on(source.query(String::new(), 50))
            .unwrap()
            .unwrap();
        assert_eq!(page.rows.len(), 5);
        assert!(state.complete(second, Ok(page)));
        assert_eq!(state.rows.len(), 55);
        assert_eq!(
            state
                .rows
                .iter()
                .map(|row| &row.item.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            55
        );
        assert!(state.begin().is_none());
        let database = source.paths.db_path();
        source.runtime.block_on(source.pool.close());
        drop(source);
        let reopened = HistorySource::open(&options).unwrap();
        assert_eq!(reopened.paths.db_path(), database);
        let page = reopened
            .runtime
            .block_on(reopened.query("search 54".into(), 0))
            .unwrap()
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].item.id, "0054");
        let empty = reopened
            .runtime
            .block_on(reopened.query("nonexistent".into(), 0))
            .unwrap()
            .unwrap();
        assert_eq!(empty.total, 0);
        assert!(!empty.has_more);
        reopened.runtime.block_on(reopened.pool.close());
    }
}
