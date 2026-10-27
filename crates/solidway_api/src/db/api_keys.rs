use sqlx::SqlitePool;

use crate::error::Result;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApiKeyRow {
    pub id: String,
    pub route_id: String,
    pub name: Option<String>,
    pub key_prefix: String,
    pub key_masked: String,
    pub key_hash: String,
    pub status: String,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 列表用：附带所属路由名。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApiKeyWithRoute {
    pub id: String,
    pub route_id: String,
    pub route_name: Option<String>,
    pub name: Option<String>,
    pub key_prefix: String,
    pub key_masked: String,
    pub status: String,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<ApiKeyWithRoute>> {
    let rows = sqlx::query_as::<_, ApiKeyWithRoute>(
        "SELECT k.id, k.route_id, r.name AS route_name, k.name, k.key_prefix, k.key_masked,
                k.status, k.expires_at, k.last_used_at, k.created_at, k.updated_at
         FROM api_keys k
         JOIN routes r ON r.id = k.route_id
         WHERE r.archived_at IS NULL
         ORDER BY k.created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 当前用户拥有的路由下的 API Key。
pub async fn list_by_owner(pool: &SqlitePool, owner_id: &str) -> Result<Vec<ApiKeyWithRoute>> {
    let rows = sqlx::query_as::<_, ApiKeyWithRoute>(
        "SELECT k.id, k.route_id, r.name AS route_name, k.name, k.key_prefix, k.key_masked,
                k.status, k.expires_at, k.last_used_at, k.created_at, k.updated_at
         FROM api_keys k
         JOIN routes r ON r.id = k.route_id
         WHERE r.owner_id = ?1 AND r.archived_at IS NULL
         ORDER BY k.created_at DESC",
    )
    .bind(owner_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_by_route(pool: &SqlitePool, route_id: &str) -> Result<Vec<ApiKeyRow>> {
    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT * FROM api_keys WHERE route_id = ?1 ORDER BY created_at DESC",
    )
    .bind(route_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<ApiKeyRow>> {
    let row = sqlx::query_as::<_, ApiKeyRow>("SELECT * FROM api_keys WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// 全部 API Key 原始行（含 `key_hash`，供控制面构造 core 配置）。
pub async fn list_all_rows(pool: &SqlitePool) -> Result<Vec<ApiKeyRow>> {
    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT k.* FROM api_keys k
         JOIN routes r ON r.id = k.route_id
         WHERE r.archived_at IS NULL
         ORDER BY k.created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn insert(pool: &SqlitePool, row: &ApiKeyRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO api_keys
         (id, route_id, name, key_prefix, key_masked, key_hash, status, expires_at,
          last_used_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )
    .bind(&row.id)
    .bind(&row.route_id)
    .bind(&row.name)
    .bind(&row.key_prefix)
    .bind(&row.key_masked)
    .bind(&row.key_hash)
    .bind(&row.status)
    .bind(row.expires_at)
    .bind(row.last_used_at)
    .bind(row.created_at)
    .bind(row.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update(pool: &SqlitePool, row: &ApiKeyRow) -> Result<()> {
    sqlx::query(
        "UPDATE api_keys SET name = ?1, status = ?2, expires_at = ?3, updated_at = ?4 WHERE id = ?5",
    )
    .bind(&row.name)
    .bind(&row.status)
    .bind(row.expires_at)
    .bind(row.updated_at)
    .bind(&row.id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<u64> {
    let result = sqlx::query("DELETE FROM api_keys WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
