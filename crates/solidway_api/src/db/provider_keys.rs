use sqlx::SqlitePool;

use crate::error::Result;

/// provider 凭据的数据库行（含密文，不直接序列化给前端）。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProviderKeyRow {
    pub id: String,
    pub route_id: String,
    pub provider: String,
    pub name: Option<String>,
    pub base_url: Option<String>,
    /// 无转换可服务的入站 API 格式（JSON 字符串数组）。
    pub api_formats: String,
    /// 鉴权方式：bearer / x-api-key / api-key / none / google_service_account / sigv4。
    pub auth_scheme: String,
    /// 凭据类型：api_key / google_service_account。
    pub credential_kind: String,
    pub credential_ciphertext: String,
    pub credential_masked: String,
    pub credential_hash: String,
    pub options: Option<String>,
    pub models: String,
    /// 选号档位：`0` = 不参与选号。
    pub priority: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub async fn list_by_route(pool: &SqlitePool, route_id: &str) -> Result<Vec<ProviderKeyRow>> {
    let rows = sqlx::query_as::<_, ProviderKeyRow>(
        "SELECT * FROM provider_keys WHERE route_id = ?1 ORDER BY priority DESC, created_at ASC",
    )
    .bind(route_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<ProviderKeyRow>> {
    let row = sqlx::query_as::<_, ProviderKeyRow>("SELECT * FROM provider_keys WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// 全部 provider 凭据（供控制面构造 core 配置）。
pub async fn list_all(pool: &SqlitePool) -> Result<Vec<ProviderKeyRow>> {
    let rows = sqlx::query_as::<_, ProviderKeyRow>(
        "SELECT k.* FROM provider_keys k
         JOIN routes r ON r.id = k.route_id
         WHERE r.archived_at IS NULL
         ORDER BY k.priority DESC, k.created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 查找同一路由下相同 `base_url` + `credential_hash` 的已有凭据（用于重复告警）。
pub async fn find_duplicate(
    pool: &SqlitePool,
    route_id: &str,
    base_url: &str,
    credential_hash: &str,
    exclude_id: Option<&str>,
) -> Result<Option<String>> {
    let row = sqlx::query_scalar::<_, String>(
        "SELECT id FROM provider_keys
         WHERE route_id = ?1 AND base_url = ?2 AND credential_hash = ?3
           AND (?4 IS NULL OR id != ?4)
         LIMIT 1",
    )
    .bind(route_id)
    .bind(base_url)
    .bind(credential_hash)
    .bind(exclude_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn insert(pool: &SqlitePool, row: &ProviderKeyRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO provider_keys
         (id, route_id, provider, name, base_url, api_formats, auth_scheme,
          credential_kind, credential_ciphertext, credential_masked, credential_hash,
          options, priority, status, created_at, updated_at, models)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17)",
    )
    .bind(&row.id)
    .bind(&row.route_id)
    .bind(&row.provider)
    .bind(&row.name)
    .bind(&row.base_url)
    .bind(&row.api_formats)
    .bind(&row.auth_scheme)
    .bind(&row.credential_kind)
    .bind(&row.credential_ciphertext)
    .bind(&row.credential_masked)
    .bind(&row.credential_hash)
    .bind(&row.options)
    .bind(row.priority)
    .bind(&row.status)
    .bind(row.created_at)
    .bind(row.updated_at)
    .bind(&row.models)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update(pool: &SqlitePool, row: &ProviderKeyRow) -> Result<()> {
    sqlx::query(
        "UPDATE provider_keys SET
            provider = ?1, name = ?2, base_url = ?3, api_formats = ?4,
            auth_scheme = ?5, credential_kind = ?6, credential_ciphertext = ?7,
            credential_masked = ?8, credential_hash = ?9,
            options = ?10, status = ?11, updated_at = ?12,
            models = ?13, priority = ?14
         WHERE id = ?15",
    )
    .bind(&row.provider)
    .bind(&row.name)
    .bind(&row.base_url)
    .bind(&row.api_formats)
    .bind(&row.auth_scheme)
    .bind(&row.credential_kind)
    .bind(&row.credential_ciphertext)
    .bind(&row.credential_masked)
    .bind(&row.credential_hash)
    .bind(&row.options)
    .bind(&row.status)
    .bind(row.updated_at)
    .bind(&row.models)
    .bind(row.priority)
    .bind(&row.id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<u64> {
    let result = sqlx::query("DELETE FROM provider_keys WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
