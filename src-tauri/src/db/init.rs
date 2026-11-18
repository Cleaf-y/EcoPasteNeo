use sqlx::SqlitePool;
use tauri::AppHandle;

use crate::core::Result;
use crate::db::path::db_path;

/// Opens the existing Tauri application's own path using the shared database setup.
pub async fn init(app: &AppHandle) -> Result<SqlitePool> {
    let path = db_path(app)?;
    let pool = ecopaste_neo_core::db::open_at(&path).await?;

    log::info!("sqlite pool ready at {path:?}");
    Ok(pool)
}
