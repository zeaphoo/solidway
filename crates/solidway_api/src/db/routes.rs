use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Route {
    pub id: String,
    /// 路由 owner；历史数据在启动回填前可能为 NULL。
    pub owner_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub enabled: i64,
    pub created_at: i64,
    pub updated_at: i64,
    /// 选号策略（`round_robin` / `uptime` / `latency` / `auto`）。
    pub selection_strategy: String,
    /// 选号覆盖项 JSON（`{}` = 全默认）。
    pub selection_config: String,
    /// 归档时刻；`NULL` = 未归档。
    pub archived_at: Option<i64>,
    /// 执行归档的用户 id。
    pub archived_by: Option<String>,
}

impl Route {
    /// 归档路由不参与数据面，也不允许新增子资源。
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

/// 活跃路由（仅用于控制面构造 core 配置）。
pub async fn list(pool: &SqlitePool) -> Result<Vec<Route>> {
    let rows = sqlx::query_as::<_, Route>(
        "SELECT * FROM routes WHERE archived_at IS NULL ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 当前用户拥有的未归档路由（管理面查询）。
pub async fn list_by_owner(pool: &SqlitePool, owner_id: &str) -> Result<Vec<Route>> {
    let rows = sqlx::query_as::<_, Route>(
        "SELECT * FROM routes WHERE owner_id = ?1 AND archived_at IS NULL ORDER BY created_at ASC",
    )
    .bind(owner_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 当前用户拥有的全部路由（含归档），按归档状态与创建时间排序。
pub async fn list_by_owner_including_archived(
    pool: &SqlitePool,
    owner_id: &str,
) -> Result<Vec<Route>> {
    let rows = sqlx::query_as::<_, Route>(
        "SELECT * FROM routes WHERE owner_id = ?1
         ORDER BY CASE WHEN archived_at IS NULL THEN 0 ELSE 1 END, created_at ASC",
    )
    .bind(owner_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 当前用户拥有的路由 id（含归档），供历史明细 / 故障的作用域过滤。
pub async fn list_ids_by_owner(
    pool: &SqlitePool,
    owner_id: &str,
    include_archived: bool,
) -> Result<Vec<String>> {
    let rows = if include_archived {
        sqlx::query_scalar::<_, String>("SELECT id FROM routes WHERE owner_id = ?1")
            .bind(owner_id)
            .fetch_all(pool)
            .await?
    } else {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM routes WHERE owner_id = ?1 AND archived_at IS NULL",
        )
        .bind(owner_id)
        .fetch_all(pool)
        .await?
    };
    Ok(rows)
}

/// 路由 id → owner 映射（含归档），供用量采集器固化历史归属。
pub async fn owner_map(pool: &SqlitePool) -> Result<std::collections::HashMap<String, String>> {
    let rows = sqlx::query_as::<_, (String, Option<String>)>("SELECT id, owner_id FROM routes")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, owner)| (id, owner.unwrap_or_default()))
        .collect())
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Route>> {
    let row = sqlx::query_as::<_, Route>("SELECT * FROM routes WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// 按 id + owner 查询；非 owner 与不存在同样返回 None。
pub async fn get_by_owner(pool: &SqlitePool, id: &str, owner_id: &str) -> Result<Option<Route>> {
    let row = sqlx::query_as::<_, Route>("SELECT * FROM routes WHERE id = ?1 AND owner_id = ?2")
        .bind(id)
        .bind(owner_id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn count_by_owner(pool: &SqlitePool, owner_id: &str) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM routes WHERE owner_id = ?1")
        .bind(owner_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// 未归档路由数量（用户归档前置条件）。
pub async fn count_active_by_owner(pool: &SqlitePool, owner_id: &str) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM routes WHERE owner_id = ?1 AND archived_at IS NULL",
    )
    .bind(owner_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn create(pool: &SqlitePool, route: &Route) -> Result<()> {
    sqlx::query(
        "INSERT INTO routes (id, owner_id, name, description, enabled, created_at, updated_at,
                             selection_strategy, selection_config, archived_at, archived_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )
    .bind(&route.id)
    .bind(&route.owner_id)
    .bind(&route.name)
    .bind(&route.description)
    .bind(route.enabled)
    .bind(route.created_at)
    .bind(route.updated_at)
    .bind(&route.selection_strategy)
    .bind(&route.selection_config)
    .bind(route.archived_at)
    .bind(&route.archived_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// 只更新选号策略（`PATCH` 之外独立写入，避免与路由 CRUD 互相覆盖）。
pub async fn update_selection(
    pool: &SqlitePool,
    id: &str,
    owner_id: &str,
    strategy: &str,
    config: &str,
    updated_at: i64,
) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE routes SET selection_strategy = ?1, selection_config = ?2, updated_at = ?3
         WHERE id = ?4 AND owner_id = ?5",
    )
    .bind(strategy)
    .bind(config)
    .bind(updated_at)
    .bind(id)
    .bind(owner_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn update(pool: &SqlitePool, route: &Route) -> Result<()> {
    sqlx::query(
        "UPDATE routes SET name = ?1, description = ?2, enabled = ?3, updated_at = ?4
         WHERE id = ?5 AND owner_id = ?6",
    )
    .bind(&route.name)
    .bind(&route.description)
    .bind(route.enabled)
    .bind(route.updated_at)
    .bind(&route.id)
    .bind(&route.owner_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// admin 转移归属；不改变路由 id 与子资源。
pub async fn transfer(pool: &SqlitePool, id: &str, to_owner: &str, now: i64) -> Result<bool> {
    let result = sqlx::query("UPDATE routes SET owner_id = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(to_owner)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// 物理删除（保留给未来的运维 purge；控制台不使用）。
pub async fn delete(pool: &SqlitePool, id: &str, owner_id: &str) -> Result<u64> {
    let result = sqlx::query("DELETE FROM routes WHERE id = ?1 AND owner_id = ?2")
        .bind(id)
        .bind(owner_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// 历史路由回填：把所有无 owner 的路由归给最早的 admin。
///
/// 幂等；没有 admin 或没有历史路由时为空操作。
pub async fn backfill_owners(pool: &SqlitePool) -> Result<u64> {
    let result = sqlx::query(
        "UPDATE routes
         SET owner_id = (
             SELECT id FROM users
             WHERE role = 'admin'
             ORDER BY created_at ASC, id ASC
             LIMIT 1
         )
         WHERE owner_id IS NULL",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
