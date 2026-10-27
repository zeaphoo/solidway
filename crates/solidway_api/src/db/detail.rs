//! 逐条明细的独立存储（`<data_root>/usage_detail.db`）。
//!
//! 与主库分离的目的：明细是"每请求一次写"的高频路径，慢下来不能拖累
//! 健康持久化 / 会话校验 / 设置读取。写入走内存缓冲 + 单事务批量（见 `usage`）。

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::error::Result;

/// 一次请求的明细（`Access` 侧字段）。
#[derive(Debug, Clone)]
pub struct DetailInsert {
    pub request_id: String,
    pub at: i64,
    pub route_id: String,
    pub api_key_id: String,
    pub owner_id: String,
    pub model: String,
    pub provider_id: String,
    pub api_format: String,
    pub is_stream: bool,
    pub status: i64,
    pub attempts: i64,
    pub failovers: i64,
    pub latency_ms: i64,
    pub ttft_ms: Option<i64>,
    pub bytes_in: i64,
    pub attempts_json: String,
}

/// 一次请求的完成信息（`Completion` 侧字段）。
#[derive(Debug, Clone, Default)]
pub struct DetailCompletion {
    pub outcome: String,
    pub duration_ms: i64,
    pub bytes_out: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_write_tokens: i64,
}

/// 明细库句柄。
#[derive(Debug, Clone)]
pub struct DetailStore {
    pool: SqlitePool,
}

impl DetailStore {
    /// 打开（或创建）明细库并跑明细迁移。
    ///
    /// 打开失败返回 `None`：明细是"可丢"的能力，不能因此阻塞服务启动。
    pub async fn open(data_root: &Path) -> Option<Self> {
        let path = data_root.join("usage_detail.db");
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = match SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
        {
            Ok(pool) => pool,
            Err(error) => {
                tracing::error!(%error, path = %path.display(), "failed to open detail database; request details disabled");
                return None;
            }
        };
        if let Err(error) = sqlx::migrate!("./migrations_detail").run(&pool).await {
            tracing::error!(%error, "detail database migration failed; request details disabled");
            return None;
        }
        Some(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

/// 明细列表行（名称字段由控制面按页内 id 从主库补齐）。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DetailListRow {
    pub request_id: String,
    pub at: i64,
    pub route_id: String,
    pub api_key_id: String,
    pub owner_id: String,
    pub model: String,
    pub provider_id: String,
    pub api_format: String,
    pub is_stream: i64,
    pub status: i64,
    pub attempts: i64,
    pub failovers: i64,
    pub latency_ms: i64,
    pub ttft_ms: Option<i64>,
    pub duration_ms: i64,
    pub bytes_in: i64,
    pub bytes_out: i64,
    pub outcome: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub attempts_json: String,
    #[sqlx(skip)]
    pub route_name: Option<String>,
    #[sqlx(skip)]
    pub owner_name: Option<String>,
    #[sqlx(skip)]
    pub api_key_label: Option<String>,
    #[sqlx(skip)]
    pub route_archived: bool,
    #[sqlx(skip)]
    pub route_deleted: bool,
    #[sqlx(skip)]
    pub api_key_deleted: bool,
    #[sqlx(skip)]
    pub provider_name: Option<String>,
    #[sqlx(skip)]
    pub provider_deleted: bool,
}

/// 一次尝试的明细。
#[derive(Debug, Clone, Serialize, Default)]
pub struct AttemptView {
    pub provider_id: String,
    pub status: Option<i64>,
    pub latency_ms: i64,
    pub error: Option<String>,
}

/// 对外返回的明细行。
#[derive(Debug, Clone, Serialize)]
pub struct DetailView {
    pub request_id: String,
    pub at: i64,
    pub route_id: String,
    pub route_name: Option<String>,
    pub owner_name: Option<String>,
    pub route_archived: bool,
    pub route_deleted: bool,
    pub api_key_id: String,
    pub api_key_label: Option<String>,
    pub api_key_deleted: bool,
    pub model: String,
    pub provider_id: String,
    pub provider_name: Option<String>,
    pub provider_deleted: bool,
    pub api_format: String,
    pub is_stream: i64,
    pub status: i64,
    pub attempts: i64,
    pub failovers: i64,
    pub latency_ms: i64,
    pub ttft_ms: Option<i64>,
    pub duration_ms: i64,
    pub bytes_in: i64,
    pub bytes_out: i64,
    pub outcome: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub attempts_detail: Vec<AttemptView>,
}

/// 明细查询过滤条件（`None` 表示不过滤）。
#[derive(Debug, Default, Clone)]
pub struct DetailFilter<'a> {
    /// 归属过滤：历史快照 owner（新数据）。
    pub owner_id: Option<&'a str>,
    /// 归属过滤：该用户拥有的路由 id 集合（旧数据回退；跨库无法 join）。
    pub owner_route_ids: Option<Vec<String>>,
    pub route_id: Option<&'a str>,
    pub api_key_id: Option<&'a str>,
    pub model: Option<&'a str>,
    pub provider_id: Option<&'a str>,
}

/// 批量 upsert（单事务）：`insert` 与可选的 completion 合并成一行。
pub async fn upsert_batch(
    pool: &SqlitePool,
    rows: &[(DetailInsert, Option<DetailCompletion>)],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for (row, completion) in rows {
        // 有 completion 时整行覆盖；没有时只写 Access 侧字段，避免把已回填的 token 清零。
        let sql = if completion.is_some() {
            "INSERT INTO usage_request_detail
             (request_id, at, route_id, api_key_id, model, provider_id, api_format, is_stream,
              status, attempts, failovers, latency_ms, ttft_ms, bytes_in, bytes_out,
              attempts_json, outcome, duration_ms, input_tokens, output_tokens,
              cached_tokens, reasoning_tokens, cache_write_tokens, owner_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,
                     ?16,?17,?18,?19,?20,?21,?22,?23,?24)
             ON CONFLICT (request_id) DO UPDATE SET
                 status = excluded.status,
                 attempts = excluded.attempts,
                 failovers = excluded.failovers,
                 latency_ms = excluded.latency_ms,
                 ttft_ms = excluded.ttft_ms,
                 bytes_in = excluded.bytes_in,
                 bytes_out = excluded.bytes_out,
                 attempts_json = excluded.attempts_json,
                 outcome = excluded.outcome,
                 duration_ms = excluded.duration_ms,
                 input_tokens = excluded.input_tokens,
                 output_tokens = excluded.output_tokens,
                 cached_tokens = excluded.cached_tokens,
                 reasoning_tokens = excluded.reasoning_tokens,
                 cache_write_tokens = excluded.cache_write_tokens"
        } else {
            "INSERT INTO usage_request_detail
             (request_id, at, route_id, api_key_id, model, provider_id, api_format, is_stream,
              status, attempts, failovers, latency_ms, ttft_ms, bytes_in, bytes_out,
              attempts_json, owner_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,0,?15,?16)
             ON CONFLICT (request_id) DO UPDATE SET
                 status = excluded.status,
                 attempts = excluded.attempts,
                 failovers = excluded.failovers,
                 latency_ms = excluded.latency_ms,
                 ttft_ms = excluded.ttft_ms,
                 bytes_in = excluded.bytes_in,
                 attempts_json = excluded.attempts_json"
        };
        let done = completion.clone().unwrap_or_default();
        let mut query = sqlx::query(sql)
            .bind(&row.request_id)
            .bind(row.at)
            .bind(&row.route_id)
            .bind(&row.api_key_id)
            .bind(&row.model)
            .bind(&row.provider_id)
            .bind(&row.api_format)
            .bind(row.is_stream as i64)
            .bind(row.status)
            .bind(row.attempts)
            .bind(row.failovers)
            .bind(row.latency_ms)
            .bind(row.ttft_ms)
            .bind(row.bytes_in);
        if completion.is_some() {
            query = query.bind(done.bytes_out);
        }
        query = query.bind(&row.attempts_json);
        if completion.is_some() {
            query = query
                .bind(&done.outcome)
                .bind(done.duration_ms)
                .bind(done.input_tokens)
                .bind(done.output_tokens)
                .bind(done.cached_tokens)
                .bind(done.reasoning_tokens)
                .bind(done.cache_write_tokens);
        }
        query = query.bind(&row.owner_id);
        query.execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// 仅补 completion（该行已在之前的批次落库）。
pub async fn update_completion_batch(
    pool: &SqlitePool,
    rows: &[(String, DetailCompletion)],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for (request_id, done) in rows {
        sqlx::query(
            "UPDATE usage_request_detail SET
                 bytes_out = ?2, outcome = ?3, duration_ms = ?4,
                 input_tokens = ?5, output_tokens = ?6, cached_tokens = ?7,
                 reasoning_tokens = ?8, cache_write_tokens = ?9
             WHERE request_id = ?1",
        )
        .bind(request_id)
        .bind(done.bytes_out)
        .bind(&done.outcome)
        .bind(done.duration_ms)
        .bind(done.input_tokens)
        .bind(done.output_tokens)
        .bind(done.cached_tokens)
        .bind(done.reasoning_tokens)
        .bind(done.cache_write_tokens)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// 按时间范围分页读取明细（不带名称；名称由 [`enrich_names`] 补）。
pub async fn list(
    pool: &SqlitePool,
    from: i64,
    to: i64,
    limit: i64,
    // 键集游标 `(at, request_id)`；`request_id` 为空表示旧式「严格早于 at」。
    cursor: Option<(i64, &str)>,
    filter: &DetailFilter<'_>,
) -> Result<Vec<DetailListRow>> {
    // 归属过滤跨库无法 join：调用方先把"该用户的路由 id"解析成 JSON 数组传进来，
    // 用 SQLite 的 json_each 做集合过滤（保持单条静态 SQL，避免拼接注入风险）。
    let owner_ids_json = filter
        .owner_route_ids
        .as_ref()
        .map(|ids| serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_string()));
    let rows = sqlx::query_as::<_, DetailListRow>(
        "SELECT request_id, at, route_id, api_key_id, owner_id, model, provider_id, api_format,
                is_stream, status, attempts, failovers, latency_ms, ttft_ms, duration_ms,
                bytes_in, bytes_out, outcome, input_tokens, output_tokens,
                cached_tokens, reasoning_tokens, attempts_json
         FROM usage_request_detail
         WHERE at >= ?1 AND at < ?2
           AND (?3 IS NULL OR (
                 (owner_id <> '' AND owner_id = ?3)
                 OR (owner_id = '' AND route_id IN (SELECT value FROM json_each(?4)))
           ))
           AND (?5 IS NULL OR route_id = ?5)
           AND (?6 IS NULL OR api_key_id = ?6)
           AND (?7 IS NULL OR model = ?7)
           AND (?8 IS NULL OR provider_id = ?8)
           AND (?9 IS NULL OR at < ?9 OR (at = ?9 AND request_id < ?10))
         ORDER BY at DESC, request_id DESC LIMIT ?11",
    )
    .bind(from)
    .bind(to)
    .bind(filter.owner_id)
    .bind(owner_ids_json)
    .bind(filter.route_id)
    .bind(filter.api_key_id)
    .bind(filter.model)
    .bind(filter.provider_id)
    .bind(cursor.map(|(at, _)| at))
    .bind(cursor.map(|(_, id)| id))
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 用主库身份视图补齐路由 / 归属用户 / API Key / Provider 标签（只补当前页 id）。
pub async fn enrich_names(main_pool: &SqlitePool, rows: &mut [DetailListRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let route_ids: Vec<String> = unique(rows.iter().map(|row| row.route_id.clone()));
    let key_ids: Vec<String> = unique(rows.iter().map(|row| row.api_key_id.clone()));
    let provider_ids: Vec<String> = unique(rows.iter().map(|row| row.provider_id.clone()));

    let mut route_names: HashMap<String, (String, Option<String>, bool, bool)> = HashMap::new();
    if !route_ids.is_empty() {
        let sql = format!(
            "SELECT id, label, owner_name,
                    CASE WHEN archived_at IS NOT NULL THEN 1 ELSE 0 END,
                    tombstone
             FROM route_identity WHERE id IN ({})",
            placeholders(route_ids.len())
        );
        let mut query = sqlx::query_as::<_, (String, Option<String>, Option<String>, i64, i64)>(
            sqlx::AssertSqlSafe(sql),
        );
        for id in &route_ids {
            query = query.bind(id);
        }
        for (id, label, owner, archived, tombstone) in query.fetch_all(main_pool).await? {
            route_names.insert(
                id,
                (
                    label.unwrap_or_default(),
                    owner,
                    archived != 0,
                    tombstone != 0,
                ),
            );
        }
    }

    let mut key_labels: HashMap<String, (Option<String>, bool)> = HashMap::new();
    if !key_ids.is_empty() {
        let sql = format!(
            "SELECT id, label, tombstone FROM api_key_identity WHERE id IN ({})",
            placeholders(key_ids.len())
        );
        let mut query =
            sqlx::query_as::<_, (String, Option<String>, i64)>(sqlx::AssertSqlSafe(sql));
        for id in &key_ids {
            query = query.bind(id);
        }
        for (id, label, tombstone) in query.fetch_all(main_pool).await? {
            key_labels.insert(id, (label, tombstone != 0));
        }
    }

    let mut provider_labels: HashMap<String, (Option<String>, bool)> = HashMap::new();
    if !provider_ids.is_empty() {
        let sql = format!(
            "SELECT id, label, tombstone FROM provider_identity WHERE id IN ({})",
            placeholders(provider_ids.len())
        );
        let mut query =
            sqlx::query_as::<_, (String, Option<String>, i64)>(sqlx::AssertSqlSafe(sql));
        for id in &provider_ids {
            query = query.bind(id);
        }
        for (id, label, tombstone) in query.fetch_all(main_pool).await? {
            provider_labels.insert(id, (label, tombstone != 0));
        }
    }

    for row in rows.iter_mut() {
        if let Some((label, owner, archived, tombstone)) = route_names.get(&row.route_id) {
            if !label.is_empty() {
                row.route_name = Some(label.clone());
            }
            row.owner_name = owner.clone();
            row.route_archived = *archived;
            row.route_deleted = *tombstone;
        }
        if let Some((label, tombstone)) = key_labels.get(&row.api_key_id) {
            row.api_key_label = label.clone();
            row.api_key_deleted = *tombstone;
        }
        if let Some((label, tombstone)) = provider_labels.get(&row.provider_id) {
            row.provider_name = label.clone();
            row.provider_deleted = *tombstone;
        }
    }
    Ok(())
}

fn unique(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = HashMap::new();
    let mut out = Vec::new();
    for item in items {
        if seen.insert(item.clone(), ()).is_none() {
            out.push(item);
        }
    }
    out
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(",")
}

/// 分批删除过期明细行，避免一次 `DELETE` 长时间持有明细库写锁。
pub async fn delete_before(pool: &SqlitePool, before: i64) -> Result<()> {
    const DELETE_BATCH: i64 = 5_000;
    loop {
        let result = sqlx::query(
            "DELETE FROM usage_request_detail
             WHERE rowid IN (
                 SELECT rowid FROM usage_request_detail WHERE at < ?1 LIMIT ?2
             )",
        )
        .bind(before)
        .bind(DELETE_BATCH)
        .execute(pool)
        .await?;
        if result.rows_affected() == 0 {
            break;
        }
    }
    Ok(())
}

/// 把明细行转成对外视图（解析尝试列表）。
pub fn to_view(row: DetailListRow) -> DetailView {
    let attempts_detail = parse_attempts(&row.attempts_json);
    DetailView {
        request_id: row.request_id,
        at: row.at,
        route_id: row.route_id,
        route_name: row.route_name,
        owner_name: row.owner_name,
        route_archived: row.route_archived,
        route_deleted: row.route_deleted,
        api_key_id: row.api_key_id,
        api_key_label: row.api_key_label,
        api_key_deleted: row.api_key_deleted,
        model: row.model,
        provider_id: row.provider_id,
        provider_name: row.provider_name,
        provider_deleted: row.provider_deleted,
        api_format: row.api_format,
        is_stream: row.is_stream,
        status: row.status,
        attempts: row.attempts,
        failovers: row.failovers,
        latency_ms: row.latency_ms,
        ttft_ms: row.ttft_ms,
        duration_ms: row.duration_ms,
        bytes_in: row.bytes_in,
        bytes_out: row.bytes_out,
        outcome: row.outcome,
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        cached_tokens: row.cached_tokens,
        reasoning_tokens: row.reasoning_tokens,
        attempts_detail,
    }
}

/// 解析尝试列表；坏数据返回空列表。
fn parse_attempts(raw: &str) -> Vec<AttemptView> {
    let items: Vec<serde_json::Value> = serde_json::from_str(raw).unwrap_or_default();
    items
        .into_iter()
        .map(|item| AttemptView {
            provider_id: item
                .get("provider_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            status: item.get("status").and_then(|v| v.as_i64()),
            latency_ms: item.get("latency_ms").and_then(|v| v.as_i64()).unwrap_or(0),
            error: item
                .get("error")
                .and_then(|v| v.as_str())
                .filter(|text| !text.is_empty())
                .map(str::to_string),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn memory_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open memory db");
        sqlx::migrate!("./migrations_detail")
            .run(&pool)
            .await
            .expect("run detail migrations");
        pool
    }

    fn empty_filter() -> DetailFilter<'static> {
        DetailFilter {
            owner_id: None,
            owner_route_ids: None,
            route_id: None,
            api_key_id: None,
            model: None,
            provider_id: None,
        }
    }

    #[tokio::test]
    async fn keyset_pagination_does_not_lose_same_second_rows() {
        // R3-F8：同秒跨页必须靠 (at, request_id) tiebreaker 稳定推进。
        let pool = memory_pool().await;
        for (id, at) in [("c", 100i64), ("b", 100), ("a", 100), ("d", 99)] {
            sqlx::query(
                "INSERT INTO usage_request_detail (request_id, at, route_id, api_key_id, status)
                 VALUES (?1, ?2, 'r1', 'k1', 200)",
            )
            .bind(id)
            .bind(at)
            .execute(&pool)
            .await
            .expect("insert detail row");
        }
        let filter = empty_filter();

        let page1 = list(&pool, 0, 200, 2, None, &filter).await.expect("page1");
        let ids1: Vec<&str> = page1.iter().map(|row| row.request_id.as_str()).collect();
        assert_eq!(ids1, ["c", "b"]);

        let last = page1.last().expect("row");
        let page2 = list(
            &pool,
            0,
            200,
            2,
            Some((last.at, last.request_id.as_str())),
            &filter,
        )
        .await
        .expect("page2");
        let ids2: Vec<&str> = page2.iter().map(|row| row.request_id.as_str()).collect();
        assert_eq!(ids2, ["a", "d"], "同秒剩余行与下一行都不得丢失/重复");

        // 旧式纯 at 游标等价于「严格早于该秒」。
        let legacy = list(&pool, 0, 200, 10, Some((100, "")), &filter)
            .await
            .expect("legacy");
        let ids3: Vec<&str> = legacy.iter().map(|row| row.request_id.as_str()).collect();
        assert_eq!(ids3, ["d"]);
    }
}
