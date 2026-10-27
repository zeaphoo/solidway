use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: String,
    pub disabled: i64,
    pub display_name: Option<String>,
    pub title: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// 归档时刻；`NULL` = 未归档。
    pub archived_at: Option<i64>,
    /// 执行归档的用户 id。
    pub archived_by: Option<String>,
}

impl User {
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

pub async fn count_admins(pool: &SqlitePool) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// 除指定用户外的**可用** admin 数量（用于「最后一个 admin」保护）。
///
/// 停用或归档的 admin 不算可用；否则可能出现没有任何可登录 admin 的状态。
pub async fn count_active_admins_excluding(pool: &SqlitePool, id: &str) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users
         WHERE role = 'admin' AND disabled = 0 AND archived_at IS NULL AND id != ?1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn find_by_username(pool: &SqlitePool, username: &str) -> Result<Option<User>> {
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE username = ?1 AND archived_at IS NULL",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<User>> {
    let users = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE archived_at IS NULL ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(users)
}

/// 全部用户（含归档），供 admin 的“显示已归档”视图。
pub async fn list_including_archived(pool: &SqlitePool) -> Result<Vec<User>> {
    let users = sqlx::query_as::<_, User>(
        "SELECT * FROM users
         ORDER BY CASE WHEN archived_at IS NULL THEN 0 ELSE 1 END, created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(users)
}

/// 目标用户是否可承接路由（存在、未停用、未归档）。
pub async fn find_active(pool: &SqlitePool, id: &str) -> Result<Option<User>> {
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE id = ?1 AND disabled = 0 AND archived_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn insert(pool: &SqlitePool, user: &User) -> Result<()> {
    sqlx::query(
        "INSERT INTO users
         (id, username, password_hash, role, disabled, display_name, title,
          created_at, updated_at, archived_at, archived_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )
    .bind(&user.id)
    .bind(&user.username)
    .bind(&user.password_hash)
    .bind(&user.role)
    .bind(user.disabled)
    .bind(&user.display_name)
    .bind(&user.title)
    .bind(user.created_at)
    .bind(user.updated_at)
    .bind(user.archived_at)
    .bind(&user.archived_by)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_profile(
    pool: &SqlitePool,
    id: &str,
    display_name: Option<&str>,
    title: Option<&str>,
    updated_at: i64,
) -> Result<()> {
    sqlx::query("UPDATE users SET display_name = ?1, title = ?2, updated_at = ?3 WHERE id = ?4")
        .bind(display_name)
        .bind(title)
        .bind(updated_at)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_password(
    pool: &SqlitePool,
    id: &str,
    hash: &str,
    updated_at: i64,
) -> Result<()> {
    sqlx::query("UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(hash)
        .bind(updated_at)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_admin_fields(pool: &SqlitePool, user: &User) -> Result<()> {
    sqlx::query(
        "UPDATE users SET role = ?1, disabled = ?2, display_name = ?3, title = ?4, updated_at = ?5
         WHERE id = ?6",
    )
    .bind(&user.role)
    .bind(user.disabled)
    .bind(&user.display_name)
    .bind(&user.title)
    .bind(user.updated_at)
    .bind(&user.id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 归档用户：设置归档与停用标记（幂等）。
pub async fn archive(pool: &SqlitePool, id: &str, archived_by: &str, now: i64) -> Result<u64> {
    let result = sqlx::query(
        "UPDATE users
            SET archived_at = COALESCE(archived_at, ?1),
                archived_by = COALESCE(archived_by, ?2),
                disabled = 1,
                updated_at = ?1
          WHERE id = ?3 AND archived_at IS NULL",
    )
    .bind(now)
    .bind(archived_by)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<u64> {
    let result = sqlx::query("DELETE FROM users WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
