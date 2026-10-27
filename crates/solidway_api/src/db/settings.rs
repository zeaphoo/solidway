use sqlx::SqlitePool;

use crate::error::Result;

pub async fn get(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let value = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(value)
}

pub async fn upsert(
    pool: &SqlitePool,
    key: &str,
    value: &str,
    updated_by: &str,
    updated_at: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at, updated_by) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at,
            updated_by = excluded.updated_by",
    )
    .bind(key)
    .bind(value)
    .bind(updated_at)
    .bind(updated_by)
    .execute(pool)
    .await?;
    Ok(())
}
