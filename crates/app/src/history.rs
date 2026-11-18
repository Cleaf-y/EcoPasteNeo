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

    /// Fetch full content by ID; list summaries must never reach the writer.
    pub fn prepare_write(
        &self,
        id: String,
        plain: bool,
    ) -> tokio::task::JoinHandle<Result<ecopaste_neo_core::clipboard::write::PreparedWrite>> {
        let pool = self.pool.clone();
        let store = ecopaste_neo_core::clipboard::ImageStore::new(&self.paths);
        self.runtime.spawn(async move {
            let item = db::items::find_item_by_id(&pool, &id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("History item no longer exists"))?;
            tokio::task::spawn_blocking(move || {
                Ok(ecopaste_neo_core::clipboard::write::PreparedWrite::new(
                    &store, &item, plain,
                )?)
            })
            .await?
        })
    }

    /// Keep the listener separate from database work and notify only after commit.
    pub fn start_capture(
        &self,
        guard: Arc<ecopaste_neo_core::writeback::WritebackGuard>,
    ) -> Result<CaptureSession> {
        use ecopaste_neo_core::clipboard::{self, CaptureSettings, ImageStore};
        let (send, mut receive) = tokio::sync::mpsc::channel(16);
        let (updates, events) = std::sync::mpsc::sync_channel(32);
        let errors = updates.clone();
        let watcher = clipboard::native::start_watcher(
            ImageStore::new(&self.paths),
            guard,
            || Some(CaptureSettings::default()),
            move |result| {
                if send.try_send(result).is_err() {
                    let _ = errors.try_send(false);
                }
            },
        )?;
        let pool = self.pool.clone();
        let worker = self.runtime.spawn(async move {
            while let Some(prepared) = receive.recv().await {
                let success = match prepared {
                    Ok(prepared) => clipboard::persist_capture(&pool, &prepared).await.is_ok(),
                    Err(_) => false,
                };
                let _ = updates.try_send(success);
            }
        });
        Ok(CaptureSession {
            watcher: Some(watcher),
            worker: Some(worker),
            runtime: self.runtime.clone(),
            events,
        })
    }

    /// Read complete content only for the requested ID, with masking and resource validation.
    pub fn preview(&self, id: String) -> tokio::task::JoinHandle<Result<preview::PreviewContent>> {
        let pool = self.pool.clone();
        let paths = self.paths.clone();
        self.runtime.spawn(async move {
            let item = db::items::find_item_by_id(&pool, &id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("History item no longer exists"))?;
            Ok(tokio::task::spawn_blocking(move || preview::record_content(&paths, &item)).await?)
        })
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
            capture: false,
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
        let prepared = source
            .runtime
            .block_on(source.prepare_write("0054".into(), false))
            .unwrap()
            .unwrap();
        assert_eq!(
            prepared.payload,
            ecopaste_neo_core::clipboard::write::WritePayload::Text {
                plain: "text 54".into(),
                rich: None
            }
        );
        assert!(
            source
                .runtime
                .block_on(source.prepare_write("missing".into(), false))
                .unwrap()
                .is_err()
        );
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

/// Session lifetime owns both native resources and the persistence consumer.
pub struct CaptureSession {
    watcher: Option<ecopaste_neo_core::clipboard::native::WatcherHandle>,
    worker: Option<tokio::task::JoinHandle<()>>,
    runtime: Arc<Runtime>,
    pub events: std::sync::mpsc::Receiver<bool>,
}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        self.watcher.take();
        // Stopping the watcher closes its sender; drain accepted copies before exit.
        if let Some(worker) = self.worker.take() {
            let _ = self.runtime.block_on(worker);
        }
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use crate::locale::Locale;
    use ecopaste_neo_core::{paths::Environment, preview::PreviewContent};

    fn source(parent: &std::path::Path) -> HistorySource {
        HistorySource::open(&Options {
            data_parent: parent.to_path_buf(),
            environment: Environment::Dev,
            locale: Locale::ZhCn,
            show: false,
            capture: false,
        })
        .unwrap()
    }

    fn insert(
        source: &HistorySource,
        id: &str,
        kind: &str,
        sub_kind: Option<&str>,
        content: &str,
        plain: Option<&str>,
        sensitive: bool,
    ) {
        source.runtime.block_on(async {
            sqlx::query("INSERT INTO clipboard_items (id,kind,sub_kind,content,content_hash,search_text,summary,is_sensitive,platform,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,'macos','2026-11-18T00:00:00Z','2026-11-18T00:00:00Z')")
                .bind(id).bind(kind).bind(sub_kind).bind(content).bind(id).bind(plain).bind(plain.map(|text| text.chars().take(512).collect::<String>())).bind(sensitive).execute(&source.pool).await.unwrap();
        });
    }

    fn preview(source: &HistorySource, id: &str) -> PreviewContent {
        source
            .runtime
            .block_on(source.preview(id.to_owned()))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn full_plain_and_rich_text_preview_preserves_paste_content_and_masks_sensitive_data() {
        let temporary = tempfile::tempdir().unwrap();
        let source = source(temporary.path());
        let long = format!("{}\nPREVIEW-LONG-END", "完整文本 🌳\n".repeat(1000));
        insert(&source, "plain", "text", None, &long, Some(&long), false);
        insert(
            &source,
            "html",
            "text",
            Some("html"),
            "<script>not display content</script><b>HTML</b>",
            Some("HTML plain tail"),
            false,
        );
        insert(
            &source,
            "rtf",
            "text",
            Some("rtf"),
            "{\\rtf1 raw markup}",
            Some("RTF plain tail"),
            false,
        );
        insert(
            &source,
            "no-plain",
            "text",
            Some("html"),
            "<b>raw markup must stay hidden</b>",
            None,
            false,
        );
        insert(
            &source,
            "sensitive",
            "text",
            None,
            "private test fixture",
            Some("private test fixture"),
            true,
        );
        assert!(matches!(preview(&source, "plain"), PreviewContent::Text(text) if text == long));
        assert!(
            matches!(preview(&source, "html"), PreviewContent::Text(text) if text == "HTML plain tail")
        );
        assert!(
            matches!(preview(&source, "rtf"), PreviewContent::Text(text) if text == "RTF plain tail")
        );
        assert!(
            matches!(preview(&source, "no-plain"), PreviewContent::Text(text) if text.is_empty())
        );
        assert!(matches!(
            preview(&source, "sensitive"),
            PreviewContent::Hidden
        ));
        let page = source
            .runtime
            .block_on(source.query(String::new(), 0))
            .unwrap()
            .unwrap();
        let row = page.rows.iter().find(|row| row.item.id == "plain").unwrap();
        assert!(row.item.content.is_empty());
        assert_eq!(row.item.summary.as_ref().unwrap().chars().count(), 512);
        let write = source
            .runtime
            .block_on(source.prepare_write("plain".into(), false))
            .unwrap()
            .unwrap();
        assert!(
            matches!(write.payload, ecopaste_neo_core::clipboard::write::WritePayload::Text { plain, .. } if plain == long)
        );
        assert!(
            source
                .runtime
                .block_on(source.preview("missing".into()))
                .unwrap()
                .is_err()
        );
        source.runtime.block_on(source.pool.close());
    }

    #[test]
    fn original_image_and_full_file_list_are_rechecked_on_every_open() {
        let temporary = tempfile::tempdir().unwrap();
        let source = source(temporary.path());
        let name = format!("{}.png", "ab".repeat(32));
        let image = source
            .paths
            .resources_dir()
            .join("clipboard-images/origin/ab")
            .join(&name);
        std::fs::create_dir_all(image.parent().unwrap()).unwrap();
        image::RgbImage::new(1600, 900).save(&image).unwrap();
        insert(&source, "image", "image", None, &name, None, false);
        insert(&source, "hidden-image", "image", None, &name, None, true);
        let file = temporary.path().join("existing.txt");
        std::fs::write(&file, b"fixture").unwrap();
        let content = format!(
            "{}\n{}\n{}\n{}",
            file.display(),
            temporary.path().display(),
            temporary.path().join("missing.txt").display(),
            temporary.path().join("also-missing.txt").display()
        );
        insert(&source, "files", "files", None, &content, None, false);
        insert(&source, "hidden-files", "files", None, &content, None, true);
        let page = source
            .runtime
            .block_on(source.query(String::new(), 0))
            .unwrap()
            .unwrap();
        let thumbnail = page
            .rows
            .iter()
            .find(|row| row.item.id == "image")
            .unwrap()
            .thumbnail
            .as_ref()
            .unwrap();
        assert_eq!(image::image_dimensions(thumbnail).unwrap(), (300, 169));
        assert!(
            matches!(preview(&source, "image"), PreviewContent::Image(Some(original)) if original.path == image && original.width == 1600 && original.height == 900)
        );
        let PreviewContent::Files(files) = preview(&source, "files") else {
            panic!("expected files");
        };
        assert_eq!(files.len(), 4);
        assert!(files[0].exists);
        assert!(files[1].is_directory);
        assert!(!files[2].exists);
        assert!(matches!(
            preview(&source, "hidden-image"),
            PreviewContent::Hidden
        ));
        assert!(matches!(
            preview(&source, "hidden-files"),
            PreviewContent::Hidden
        ));
        std::fs::rename(file, temporary.path().join("moved.txt")).unwrap();
        let PreviewContent::Files(files) = preview(&source, "files") else {
            panic!("expected files");
        };
        assert!(!files[0].exists);
        std::fs::remove_file(image).unwrap();
        assert!(thumbnail.exists());
        assert!(matches!(
            preview(&source, "image"),
            PreviewContent::Image(None)
        ));
        source.runtime.block_on(source.pool.close());
    }
}
