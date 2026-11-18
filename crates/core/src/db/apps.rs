use super::{models::ClipboardApp, Result};
use chrono::Utc;
use sqlx::SqlitePool;

/// upsert：id 已存在则只刷新 name / icon_file / updated_at；不存在则全量插入。
/// 显式分两路而非依赖 `INSERT OR REPLACE`：后者会重置 created_at 与（潜在的）外键级联。
pub async fn upsert_app(pool: &SqlitePool, app: &ClipboardApp) -> Result<()> {
    let now = Utc::now();
    let updated = sqlx::query(
        "UPDATE clipboard_apps SET name = ?, icon_file = ?, updated_at = ? WHERE id = ?",
    )
    .bind(app.name.as_str())
    .bind(app.icon_file.as_deref())
    .bind(now)
    .bind(app.id.as_str())
    .execute(pool)
    .await?;

    if updated.rows_affected() > 0 {
        return Ok(());
    }

    sqlx::query(
        "INSERT INTO clipboard_apps (id, name, icon_file, platform, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(app.id.as_str())
    .bind(app.name.as_str())
    .bind(app.icon_file.as_deref())
    .bind(app.platform)
    .bind(app.created_at)
    .bind(app.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}
