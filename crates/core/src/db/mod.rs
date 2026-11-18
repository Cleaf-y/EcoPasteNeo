//! SQLite initialization and history queries, independent of either UI framework.

pub mod items;
pub mod models;

use std::path::Path;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{ConnectOptions, SqlitePool};
use thiserror::Error;

use crate::paths::DataPaths;

pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
}

pub type Result<T> = std::result::Result<T, DatabaseError>;

/// Opens a database only after Storage has established Neo directory ownership.
pub async fn open(paths: &DataPaths) -> Result<SqlitePool> {
    open_at(&paths.db_path()).await
}

/// Opens an application-resolved path using shared SQLite options and migrations.
/// The Tauri adapter supplies its legacy path; Neo callers should use `open`.
pub async fn open_at(path: &Path) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .disable_statement_logging();
    let pool = SqlitePoolOptions::new().connect_with(options).await?;

    if let Err(error) = MIGRATOR.run(&pool).await {
        pool.close().await;
        return Err(error.into());
    }

    Ok(pool)
}
