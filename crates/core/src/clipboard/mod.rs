//! Clipboard payload preparation and persistence without a UI runtime.

mod ingest;
#[cfg(feature = "system-clipboard")]
pub mod native;
pub mod payload;
#[cfg(feature = "system-clipboard")]
mod read;
mod secrets;
pub mod settings;
#[cfg(feature = "system-clipboard")]
pub mod source;
mod storage;

pub use ingest::{build_item_with_settings, SUMMARY_MAX_CHARS};
pub use storage::ImageStore;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;

use crate::db::items::{upsert_item, UpsertResult};
use crate::db::models::{ClipboardApp, ClipboardItem, ClipboardKind};
use crate::writeback::WritebackGuard;
use payload::ClipboardPayload;
use settings::{Capture, Sensitive};

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("{0}")]
    Clipboard(String),
    #[error(transparent)]
    Database(#[from] crate::db::DatabaseError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, CaptureError>;

/// A settings snapshot for one capture, supplied by the application's settings owner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct CaptureSettings {
    pub capture: Capture,
    pub sensitive: Sensitive,
    pub plain_only: bool,
    pub excluded_app_ids: Vec<String>,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            capture: Capture::default(),
            sensitive: Sensitive::default(),
            plain_only: false,
            excluded_app_ids: settings::Filters::default().excluded_app_ids,
        }
    }
}

impl CaptureSettings {
    pub fn accepts_source(&self, source: Option<&ClipboardApp>) -> bool {
        !source.is_some_and(|source| self.excluded_app_ids.contains(&source.id))
    }
}

/// Owned data only: safe to send from the native listener into an async runtime.
pub struct PreparedCapture {
    pub item: ClipboardItem,
    pub source_app: Option<ClipboardApp>,
}

/// Delivered after commit; a panel should re-query its current database on this update.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureUpdate {
    pub id: String,
    pub kind: ClipboardKind,
    pub deduplicated: bool,
}

/// Apply source exclusion, capture/privacy settings and optional writeback suppression.
/// Manual reads may omit the guard; native change notifications must supply it.
pub fn prepare_capture(
    store: &ImageStore,
    payload: &ClipboardPayload,
    settings: &CaptureSettings,
    source_app: Option<ClipboardApp>,
    guard: Option<&WritebackGuard>,
) -> Result<Option<PreparedCapture>> {
    if !settings.accepts_source(source_app.as_ref()) {
        return Ok(None);
    }
    let Some(mut item) = build_item_with_settings(
        store,
        payload,
        &settings.capture,
        &settings.sensitive,
        settings.plain_only,
    )?
    else {
        return Ok(None);
    };
    if guard.is_some_and(|guard| guard.should_skip(&item.content_hash)) {
        return Ok(None);
    }
    item.source_app_id = source_app.as_ref().map(|source| source.id.clone());
    Ok(Some(PreparedCapture { item, source_app }))
}

/// Persist on the caller's current pool; source failure must not discard the content.
/// UI notification and sound stay in the application adapter, after this returns.
pub async fn persist_item(
    pool: &SqlitePool,
    item: &ClipboardItem,
    source_app: Option<&ClipboardApp>,
) -> Result<UpsertResult> {
    let mut item = item.clone();
    if let Some(source) = source_app {
        if let Err(error) = crate::db::apps::upsert_app(pool, source).await {
            log::warn!(
                "clipboard source app upsert failed ({}): {error}",
                source.id
            );
            item.source_app_id = None;
        }
    }
    Ok(upsert_item(pool, &item).await?)
}

/// Persist a prepared payload and return the refresh data only after SQLite commits.
pub async fn persist_capture(
    pool: &SqlitePool,
    capture: &PreparedCapture,
) -> Result<CaptureUpdate> {
    let result = persist_item(pool, &capture.item, capture.source_app.as_ref()).await?;
    Ok(CaptureUpdate {
        id: result.id,
        kind: capture.item.kind,
        deduplicated: result.deduplicated,
    })
}
