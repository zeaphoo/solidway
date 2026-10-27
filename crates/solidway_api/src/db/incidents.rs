//! 故障事件（incident）：`open` → `closed` 的故障生命周期记录。
//!
//! 与 `provider_health`（当前态）互补：状态回答"现在能不能用"，
//! 事件回答"什么时候坏过、为什么、多久、怎么恢复的"。

use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::Result;
use crate::state::new_id;

/// 同类失败的合并窗口：1 小时内视为同一次故障。
pub const MERGE_WINDOW_SECS: i64 = 3600;

/// 原因码 → 严重级别。
pub fn severity_for(kind: &str) -> &'static str {
    match kind {
        "http_401" => "manual_action",
        "insufficient_quota" | "http_403" => "degraded",
        _ => "transient",
    }
}

/// 一条故障记录。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct IncidentRow {
    pub id: String,
    pub provider_id: String,
    pub route_id: String,
    /// 故障发生时的路由 owner 快照；Route 转移后历史仍归原 owner。
    pub owner_id: String,
    pub kind: String,
    pub severity: String,
    pub state: String,
    pub started_at: i64,
    pub last_seen_at: i64,
    pub ended_at: Option<i64>,
    pub attempts: i64,
    pub last_status: Option<i64>,
    pub last_error: Option<String>,
    pub resolved_by: Option<String>,
    pub notified_at: Option<i64>,
    // 以下为查询时 join 补出的展示字段
    #[sqlx(default)]
    pub provider: Option<String>,
    #[sqlx(default)]
    pub provider_name: Option<String>,
    #[sqlx(default)]
    pub route_name: Option<String>,
    #[sqlx(default)]
    pub owner_name: Option<String>,
    #[sqlx(default)]
    pub route_archived: i64,
    #[sqlx(default)]
    pub route_deleted: i64,
    #[sqlx(default)]
    pub provider_deleted: i64,
}

/// 记录一次故障：同 provider + kind 的 open 记录在合并窗口内 → 累加；否则新开。
///
/// **并发前提（R2-A-4）**：仅限单消费者（core 事件上收任务）调用。读取-合并-写入
/// 不是事务，多任务并发调用同一 provider 会丢更新；本模块不为该场景加锁 / 事务。
///
/// 返回被写入/更新的记录 id（供告警判断"是否新开"）。
pub async fn record(
    pool: &SqlitePool,
    provider_id: &str,
    route_id: &str,
    kind: &str,
    now: i64,
    last_status: Option<i64>,
    last_error: Option<&str>,
) -> Result<(String, bool)> {
    let severity = severity_for(kind);
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM provider_incident
         WHERE provider_id = ?1 AND kind = ?2 AND state = 'open' AND last_seen_at >= ?3
         ORDER BY last_seen_at DESC LIMIT 1",
    )
    .bind(provider_id)
    .bind(kind)
    .bind(now - MERGE_WINDOW_SECS)
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = existing {
        sqlx::query(
            "UPDATE provider_incident
                SET attempts = attempts + 1,
                    last_seen_at = ?2,
                    last_status = COALESCE(?3, last_status),
                    last_error = COALESCE(?4, last_error),
                    severity = ?5
              WHERE id = ?1
                AND EXISTS (SELECT 1 FROM provider_keys k WHERE k.id = ?6)",
        )
        .bind(&id)
        .bind(now)
        .bind(last_status)
        .bind(last_error)
        .bind(severity)
        .bind(provider_id)
        .execute(pool)
        .await?;
        return Ok((id, false));
    }

    let id = new_id();
    let inserted = sqlx::query(
        "INSERT INTO provider_incident
            (id, provider_id, route_id, owner_id, kind, severity, state, started_at, last_seen_at,
             attempts, last_status, last_error)
         SELECT ?1, ?2, ?3,
                COALESCE((SELECT owner_id FROM routes WHERE id = ?3), ''),
                ?4, ?5, 'open', ?6, ?6, 1, ?7, ?8
          WHERE EXISTS (SELECT 1 FROM provider_keys k WHERE k.id = ?2)",
    )
    .bind(&id)
    .bind(provider_id)
    .bind(route_id)
    .bind(kind)
    .bind(severity)
    .bind(now)
    .bind(last_status)
    .bind(last_error)
    .execute(pool)
    .await?;
    if inserted.rows_affected() == 0 {
        // Provider 已被删除：不复活故障。
        return Ok((id, false));
    }
    Ok((id, true))
}

/// 关闭某 Provider 的所有 open 记录，返回被关闭的 id 列表。
///
/// **并发前提（R2-A-4）**：仅限单消费者调用（见 [`record`]）。
pub async fn close_open(
    pool: &SqlitePool,
    provider_id: &str,
    now: i64,
    resolved_by: &str,
) -> Result<Vec<String>> {
    let ids: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM provider_incident WHERE provider_id = ?1 AND state = 'open'",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query(
        "UPDATE provider_incident
            SET state = 'closed', ended_at = ?2, resolved_by = ?3
          WHERE provider_id = ?1 AND state = 'open'",
    )
    .bind(provider_id)
    .bind(now)
    .bind(resolved_by)
    .execute(pool)
    .await?;
    Ok(ids.into_iter().map(|(id,)| id).collect())
}

/// 关闭某 Route 下所有 open 记录，返回被关闭的 id 列表。
pub async fn close_open_for_route(
    pool: &SqlitePool,
    route_id: &str,
    now: i64,
    resolved_by: &str,
) -> Result<Vec<String>> {
    let ids: Vec<(String,)> =
        sqlx::query_as("SELECT id FROM provider_incident WHERE route_id = ?1 AND state = 'open'")
            .bind(route_id)
            .fetch_all(pool)
            .await?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query(
        "UPDATE provider_incident
            SET state = 'closed', ended_at = ?2, resolved_by = ?3
          WHERE route_id = ?1 AND state = 'open'",
    )
    .bind(route_id)
    .bind(now)
    .bind(resolved_by)
    .execute(pool)
    .await?;
    Ok(ids.into_iter().map(|(id,)| id).collect())
}

/// 按 incident id 关闭一条记录（不依赖 Provider 是否仍存在）。
pub async fn close_one(pool: &SqlitePool, id: &str, now: i64, resolved_by: &str) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE provider_incident
            SET state = 'closed', ended_at = ?2, resolved_by = ?3
          WHERE id = ?1 AND state = 'open'",
    )
    .bind(id)
    .bind(now)
    .bind(resolved_by)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// 记录告警发送时刻（去抖与排障）。
pub async fn mark_notified(pool: &SqlitePool, id: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE provider_incident SET notified_at = ?2 WHERE id = ?1")
        .bind(id)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

/// 查询过滤条件。
#[derive(Debug, Default, Clone)]
pub struct IncidentFilter<'a> {
    /// 当前用户名下的路由 id（普通用户必填，admin 为 `None`）。
    pub owner_route_ids: Option<Vec<String>>,
    pub route_id: Option<&'a str>,
    pub provider_id: Option<&'a str>,
    pub state: Option<&'a str>,
    pub since: Option<i64>,
}

/// 列表：未解决优先，其次按最近发生时间倒序。
pub async fn list(
    pool: &SqlitePool,
    filter: &IncidentFilter<'_>,
    limit: i64,
) -> Result<Vec<IncidentRow>> {
    let owner_ids_json = filter
        .owner_route_ids
        .as_ref()
        .map(|ids| serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_string()));
    let rows = sqlx::query_as::<_, IncidentRow>(
        "SELECT i.id, i.provider_id, i.route_id, i.owner_id, i.kind, i.severity, i.state,
                i.started_at, i.last_seen_at, i.ended_at, i.attempts,
                i.last_status, i.last_error, i.resolved_by, i.notified_at,
                pi.provider AS provider, pi.label AS provider_name,
                ri.label AS route_name,
                COALESCE(ui.label, '未归属') AS owner_name,
                CASE WHEN ri.archived_at IS NOT NULL THEN 1 ELSE 0 END AS route_archived,
                CASE WHEN ri.tombstone = 1 THEN 1 ELSE 0 END AS route_deleted,
                CASE WHEN pi.tombstone = 1 THEN 1 ELSE 0 END AS provider_deleted
           FROM provider_incident i
           LEFT JOIN provider_identity pi ON pi.id = i.provider_id
           LEFT JOIN route_identity ri ON ri.id = i.route_id
           LEFT JOIN user_identity ui ON ui.id = COALESCE(NULLIF(i.owner_id, ''), ri.owner_id)
          WHERE (?1 IS NULL OR i.route_id IN (SELECT value FROM json_each(?1)))
            AND (?2 IS NULL OR i.route_id = ?2)
            AND (?3 IS NULL OR i.provider_id = ?3)
            AND (?4 IS NULL OR i.state = ?4)
            AND (?5 IS NULL OR i.last_seen_at >= ?5)
          ORDER BY CASE i.state WHEN 'open' THEN 0 ELSE 1 END,
                   CASE i.severity WHEN 'manual_action' THEN 0 WHEN 'degraded' THEN 1 ELSE 2 END,
                   i.last_seen_at DESC
          LIMIT ?6",
    )
    .bind(owner_ids_json)
    .bind(filter.route_id)
    .bind(filter.provider_id)
    .bind(filter.state)
    .bind(filter.since)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 每个 provider 近 `since` 以来的故障次数与累计时长（Provider 列表列用）。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct IncidentStat {
    pub provider_id: String,
    pub count: i64,
    /// 累计故障时长（秒）；未解决记录算到 `now`。
    pub duration_secs: i64,
}

pub async fn stats_since(
    pool: &SqlitePool,
    since: i64,
    now: i64,
    owner_route_ids: Option<&[String]>,
) -> Result<Vec<IncidentStat>> {
    let owner_ids_json =
        owner_route_ids.map(|ids| serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_string()));
    let rows = sqlx::query_as::<_, IncidentStat>(
        "SELECT provider_id,
                COUNT(*) AS count,
                COALESCE(SUM(COALESCE(ended_at, ?2) - started_at), 0) AS duration_secs
           FROM provider_incident
          WHERE last_seen_at >= ?1
            AND (?3 IS NULL OR route_id IN (SELECT value FROM json_each(?3)))
          GROUP BY provider_id",
    )
    .bind(since)
    .bind(now)
    .bind(owner_ids_json)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 该 Provider 是否存在未解决的 `manual_action`（界面高亮用）。
pub async fn has_open_manual(pool: &SqlitePool, provider_id: &str) -> Result<bool> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM provider_incident
          WHERE provider_id = ?1 AND state = 'open' AND severity = 'manual_action' LIMIT 1",
    )
    .bind(provider_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}
