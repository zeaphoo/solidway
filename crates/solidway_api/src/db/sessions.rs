use sqlx::SqlitePool;

use crate::db::users::User;
use crate::error::Result;

pub async fn create(
    pool: &SqlitePool,
    token_hash: &str,
    user_id: &str,
    created_at: i64,
    expires_at: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(created_at)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// 按会话令牌哈希查找未过期且未被禁用的用户。
pub async fn find_user(pool: &SqlitePool, token_hash: &str, now: i64) -> Result<Option<User>> {
    let user = sqlx::query_as::<_, User>(
        "SELECT u.* FROM sessions s
         JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ?1 AND s.expires_at > ?2
           AND u.disabled = 0 AND u.archived_at IS NULL",
    )
    .bind(token_hash)
    .bind(now)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn delete(pool: &SqlitePool, token_hash: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?1")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// 删除某用户的全部会话（重置口令后让旧 Cookie 一律失效）。
pub async fn delete_for_user(pool: &SqlitePool, user_id: &str) -> Result<u64> {
    let result = sqlx::query("DELETE FROM sessions WHERE user_id = ?1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
