use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::Result;

/// 延迟直方图桶上界（毫秒）；最后一个是 `+inf`。
pub const HIST_BUCKETS: [u64; 11] = [10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000, u64::MAX];

/// 聚合/明细查询的可选过滤条件。
#[derive(Debug, Clone, Default)]
pub struct UsageFilter<'a> {
    /// 路由 owner；普通统计必须填写，全局统计可选。
    pub owner_id: Option<&'a str>,
    pub route_id: Option<&'a str>,
    pub api_key_id: Option<&'a str>,
    pub model: Option<&'a str>,
    pub provider_id: Option<&'a str>,
}

/// 待落盘的请求聚合增量。
#[derive(Debug, Clone, Default)]
pub struct BucketDelta {
    pub requests: i64,
    pub completed: i64,
    pub errors: i64,
    pub client_errors: i64,
    pub server_errors: i64,
    pub aborted: i64,
    pub stream_errors: i64,
    pub usage_missing: i64,
    pub failovers: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_write_tokens: i64,
    pub latency_sum_ms: i64,
    pub latency_count: i64,
    pub ttft_sum_ms: i64,
    pub ttft_count: i64,
    pub duration_sum_ms: i64,
    pub duration_count: i64,
    pub bytes_in: i64,
    pub bytes_out: i64,
    pub hist: [i64; 11],
}

impl BucketDelta {
    /// 合并另一份增量（落盘失败回灌用）：逐字段相加。
    pub fn merge(&mut self, other: Self) {
        self.requests += other.requests;
        self.completed += other.completed;
        self.errors += other.errors;
        self.client_errors += other.client_errors;
        self.server_errors += other.server_errors;
        self.aborted += other.aborted;
        self.stream_errors += other.stream_errors;
        self.usage_missing += other.usage_missing;
        self.failovers += other.failovers;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cached_tokens += other.cached_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.latency_sum_ms += other.latency_sum_ms;
        self.latency_count += other.latency_count;
        self.ttft_sum_ms += other.ttft_sum_ms;
        self.ttft_count += other.ttft_count;
        self.duration_sum_ms += other.duration_sum_ms;
        self.duration_count += other.duration_count;
        self.bytes_in += other.bytes_in;
        self.bytes_out += other.bytes_out;
        for (index, value) in other.hist.iter().enumerate() {
            self.hist[index] += value;
        }
    }
}

const UPSERT_SQL: &str = "INSERT INTO usage_bucket
    (granularity, bucket_ts, route_id, api_key_id, model, provider_id,
     requests, completed, errors, client_errors, server_errors, aborted,
     stream_errors, usage_missing, failovers,
     input_tokens, output_tokens, cached_tokens, reasoning_tokens, cache_write_tokens,
     latency_sum_ms, latency_count, ttft_sum_ms, ttft_count, duration_sum_ms, duration_count,
     bytes_in, bytes_out,
     hist_0, hist_1, hist_2, hist_3, hist_4, hist_5, hist_6, hist_7, hist_8, hist_9, hist_10,
     owner_id)
    VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,
            ?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36,?37,?38,?39,?40)
    ON CONFLICT(granularity, bucket_ts, route_id, api_key_id, model, provider_id, owner_id) DO UPDATE SET
      requests = requests + excluded.requests,
      completed = completed + excluded.completed,
      errors = errors + excluded.errors,
      client_errors = client_errors + excluded.client_errors,
      server_errors = server_errors + excluded.server_errors,
      aborted = aborted + excluded.aborted,
      stream_errors = stream_errors + excluded.stream_errors,
      usage_missing = usage_missing + excluded.usage_missing,
      failovers = failovers + excluded.failovers,
      input_tokens = input_tokens + excluded.input_tokens,
      output_tokens = output_tokens + excluded.output_tokens,
      cached_tokens = cached_tokens + excluded.cached_tokens,
      reasoning_tokens = reasoning_tokens + excluded.reasoning_tokens,
      cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens,
      latency_sum_ms = latency_sum_ms + excluded.latency_sum_ms,
      latency_count = latency_count + excluded.latency_count,
      ttft_sum_ms = ttft_sum_ms + excluded.ttft_sum_ms,
      ttft_count = ttft_count + excluded.ttft_count,
      duration_sum_ms = duration_sum_ms + excluded.duration_sum_ms,
      duration_count = duration_count + excluded.duration_count,
      bytes_in = bytes_in + excluded.bytes_in,
      bytes_out = bytes_out + excluded.bytes_out,
      hist_0 = hist_0 + excluded.hist_0,
      hist_1 = hist_1 + excluded.hist_1,
      hist_2 = hist_2 + excluded.hist_2,
      hist_3 = hist_3 + excluded.hist_3,
      hist_4 = hist_4 + excluded.hist_4,
      hist_5 = hist_5 + excluded.hist_5,
      hist_6 = hist_6 + excluded.hist_6,
      hist_7 = hist_7 + excluded.hist_7,
      hist_8 = hist_8 + excluded.hist_8,
      hist_9 = hist_9 + excluded.hist_9,
      hist_10 = hist_10 + excluded.hist_10";

#[allow(clippy::too_many_arguments)]
pub async fn add_bucket(
    pool: &SqlitePool,
    granularity: &str,
    bucket_ts: i64,
    route_id: &str,
    api_key_id: &str,
    model: &str,
    provider_id: &str,
    owner_id: &str,
    delta: &BucketDelta,
) -> Result<()> {
    add_bucket_on(
        pool,
        granularity,
        bucket_ts,
        route_id,
        api_key_id,
        model,
        provider_id,
        owner_id,
        delta,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn add_bucket_on<'e, E>(
    executor: E,
    granularity: &str,
    bucket_ts: i64,
    route_id: &str,
    api_key_id: &str,
    model: &str,
    provider_id: &str,
    owner_id: &str,
    delta: &BucketDelta,
) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let mut query = sqlx::query(UPSERT_SQL)
        .bind(granularity)
        .bind(bucket_ts)
        .bind(route_id)
        .bind(api_key_id)
        .bind(model)
        .bind(provider_id)
        .bind(delta.requests)
        .bind(delta.completed)
        .bind(delta.errors)
        .bind(delta.client_errors)
        .bind(delta.server_errors)
        .bind(delta.aborted)
        .bind(delta.stream_errors)
        .bind(delta.usage_missing)
        .bind(delta.failovers)
        .bind(delta.input_tokens)
        .bind(delta.output_tokens)
        .bind(delta.cached_tokens)
        .bind(delta.reasoning_tokens)
        .bind(delta.cache_write_tokens)
        .bind(delta.latency_sum_ms)
        .bind(delta.latency_count)
        .bind(delta.ttft_sum_ms)
        .bind(delta.ttft_count)
        .bind(delta.duration_sum_ms)
        .bind(delta.duration_count)
        .bind(delta.bytes_in)
        .bind(delta.bytes_out);
    for value in delta.hist {
        query = query.bind(value);
    }
    query = query.bind(owner_id);
    query.execute(executor).await?;
    Ok(())
}

/// 一次 provider 尝试的聚合增量。
#[derive(Debug, Clone, Default)]
pub struct AttemptDelta {
    pub attempts: i64,
    pub success: i64,
    pub errors: i64,
    pub status_4xx: i64,
    pub status_5xx: i64,
    pub latency_sum_ms: i64,
    pub latency_count: i64,
}

impl AttemptDelta {
    /// 合并另一份增量（落盘失败回灌用）：逐字段相加。
    pub fn merge(&mut self, other: Self) {
        self.attempts += other.attempts;
        self.success += other.success;
        self.errors += other.errors;
        self.status_4xx += other.status_4xx;
        self.status_5xx += other.status_5xx;
        self.latency_sum_ms += other.latency_sum_ms;
        self.latency_count += other.latency_count;
    }
}

pub async fn add_attempt_bucket(
    pool: &SqlitePool,
    granularity: &str,
    bucket_ts: i64,
    route_id: &str,
    provider_id: &str,
    model: &str,
    delta: &AttemptDelta,
) -> Result<()> {
    add_attempt_bucket_on(pool, granularity, bucket_ts, route_id, provider_id, model, delta).await
}

pub(crate) async fn add_attempt_bucket_on<'e, E>(
    executor: E,
    granularity: &str,
    bucket_ts: i64,
    route_id: &str,
    provider_id: &str,
    model: &str,
    delta: &AttemptDelta,
) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query(
        "INSERT INTO usage_attempt_bucket
         (granularity, bucket_ts, route_id, provider_id, model,
          attempts, success, errors, status_4xx, status_5xx, latency_sum_ms, latency_count)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         ON CONFLICT(granularity, bucket_ts, route_id, provider_id, model) DO UPDATE SET
           attempts = attempts + excluded.attempts,
           success = success + excluded.success,
           errors = errors + excluded.errors,
           status_4xx = status_4xx + excluded.status_4xx,
           status_5xx = status_5xx + excluded.status_5xx,
           latency_sum_ms = latency_sum_ms + excluded.latency_sum_ms,
           latency_count = latency_count + excluded.latency_count",
    )
    .bind(granularity)
    .bind(bucket_ts)
    .bind(route_id)
    .bind(provider_id)
    .bind(model)
    .bind(delta.attempts)
    .bind(delta.success)
    .bind(delta.errors)
    .bind(delta.status_4xx)
    .bind(delta.status_5xx)
    .bind(delta.latency_sum_ms)
    .bind(delta.latency_count)
    .execute(executor)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct UsageTotals {
    pub requests: i64,
    pub completed: i64,
    pub errors: i64,
    pub client_errors: i64,
    pub server_errors: i64,
    pub aborted: i64,
    pub stream_errors: i64,
    pub usage_missing: i64,
    pub failovers: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_write_tokens: i64,
    pub latency_sum_ms: i64,
    pub latency_count: i64,
    pub ttft_sum_ms: i64,
    pub ttft_count: i64,
    pub duration_sum_ms: i64,
    pub duration_count: i64,
    pub bytes_in: i64,
    pub bytes_out: i64,
    pub hist_0: i64,
    pub hist_1: i64,
    pub hist_2: i64,
    pub hist_3: i64,
    pub hist_4: i64,
    pub hist_5: i64,
    pub hist_6: i64,
    pub hist_7: i64,
    pub hist_8: i64,
    pub hist_9: i64,
    pub hist_10: i64,
}

impl UsageTotals {
    pub fn hist(&self) -> [i64; 11] {
        [
            self.hist_0,
            self.hist_1,
            self.hist_2,
            self.hist_3,
            self.hist_4,
            self.hist_5,
            self.hist_6,
            self.hist_7,
            self.hist_8,
            self.hist_9,
            self.hist_10,
        ]
    }
}

const TOTALS_BASE: &str = "SELECT
        COALESCE(SUM(b.requests),0) AS requests,
        COALESCE(SUM(b.completed),0) AS completed,
        COALESCE(SUM(b.errors),0) AS errors,
        COALESCE(SUM(b.client_errors),0) AS client_errors,
        COALESCE(SUM(b.server_errors),0) AS server_errors,
        COALESCE(SUM(b.aborted),0) AS aborted,
        COALESCE(SUM(b.stream_errors),0) AS stream_errors,
        COALESCE(SUM(b.usage_missing),0) AS usage_missing,
        COALESCE(SUM(b.failovers),0) AS failovers,
        COALESCE(SUM(b.input_tokens),0) AS input_tokens,
        COALESCE(SUM(b.output_tokens),0) AS output_tokens,
        COALESCE(SUM(b.cached_tokens),0) AS cached_tokens,
        COALESCE(SUM(b.reasoning_tokens),0) AS reasoning_tokens,
        COALESCE(SUM(b.cache_write_tokens),0) AS cache_write_tokens,
        COALESCE(SUM(b.latency_sum_ms),0) AS latency_sum_ms,
        COALESCE(SUM(b.latency_count),0) AS latency_count,
        COALESCE(SUM(b.ttft_sum_ms),0) AS ttft_sum_ms,
        COALESCE(SUM(b.ttft_count),0) AS ttft_count,
        COALESCE(SUM(b.duration_sum_ms),0) AS duration_sum_ms,
        COALESCE(SUM(b.duration_count),0) AS duration_count,
        COALESCE(SUM(b.bytes_in),0) AS bytes_in,
        COALESCE(SUM(b.bytes_out),0) AS bytes_out,
        COALESCE(SUM(b.hist_0),0) AS hist_0, COALESCE(SUM(b.hist_1),0) AS hist_1,
        COALESCE(SUM(b.hist_2),0) AS hist_2, COALESCE(SUM(b.hist_3),0) AS hist_3,
        COALESCE(SUM(b.hist_4),0) AS hist_4, COALESCE(SUM(b.hist_5),0) AS hist_5,
        COALESCE(SUM(b.hist_6),0) AS hist_6, COALESCE(SUM(b.hist_7),0) AS hist_7,
        COALESCE(SUM(b.hist_8),0) AS hist_8, COALESCE(SUM(b.hist_9),0) AS hist_9,
        COALESCE(SUM(b.hist_10),0) AS hist_10
     FROM usage_bucket b
     LEFT JOIN routes r ON r.id = b.route_id
     WHERE b.granularity = ?1 AND b.bucket_ts >= ?2 AND b.bucket_ts < ?3
       AND (?4 IS NULL OR COALESCE(NULLIF(b.owner_id, ''), r.owner_id) = ?4)
       AND (?5 IS NULL OR b.route_id = ?5)
       AND (?6 IS NULL OR b.api_key_id = ?6)
       AND (?7 IS NULL OR b.model = ?7)
       AND (?8 IS NULL OR b.provider_id = ?8)";

pub async fn totals(
    pool: &SqlitePool,
    granularity: &str,
    from: i64,
    to: i64,
    filter: &UsageFilter<'_>,
) -> Result<UsageTotals> {
    let row = sqlx::query_as::<_, UsageTotals>(TOTALS_BASE)
        .bind(granularity)
        .bind(from)
        .bind(to)
        .bind(filter.owner_id)
        .bind(filter.route_id)
        .bind(filter.api_key_id)
        .bind(filter.model)
        .bind(filter.provider_id)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

/// 时间序列点。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TimePoint {
    pub ts: i64,
    pub requests: i64,
    pub completed: i64,
    pub errors: i64,
    pub failovers: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub latency_sum_ms: i64,
    pub latency_count: i64,
    pub ttft_sum_ms: i64,
    pub ttft_count: i64,
    pub bytes_in: i64,
    pub bytes_out: i64,
}

pub async fn timeseries(
    pool: &SqlitePool,
    granularity: &str,
    from: i64,
    to: i64,
    group_seconds: i64,
    filter: &UsageFilter<'_>,
) -> Result<Vec<TimePoint>> {
    let ts_expr = if group_seconds > 0 {
        "(b.bucket_ts / ?4) * ?4"
    } else {
        "b.bucket_ts + (?4 * 0)"
    };
    let sql = format!(
        "SELECT {ts_expr} AS ts,
                COALESCE(SUM(b.requests),0) AS requests,
                COALESCE(SUM(b.completed),0) AS completed,
                COALESCE(SUM(b.errors),0) AS errors,
                COALESCE(SUM(b.failovers),0) AS failovers,
                COALESCE(SUM(b.input_tokens),0) AS input_tokens,
                COALESCE(SUM(b.output_tokens),0) AS output_tokens,
                COALESCE(SUM(b.cached_tokens),0) AS cached_tokens,
                COALESCE(SUM(b.latency_sum_ms),0) AS latency_sum_ms,
                COALESCE(SUM(b.latency_count),0) AS latency_count,
                COALESCE(SUM(b.ttft_sum_ms),0) AS ttft_sum_ms,
                COALESCE(SUM(b.ttft_count),0) AS ttft_count,
                COALESCE(SUM(b.bytes_in),0) AS bytes_in,
                COALESCE(SUM(b.bytes_out),0) AS bytes_out
         FROM usage_bucket b
         LEFT JOIN routes r ON r.id = b.route_id
         WHERE b.granularity = ?1 AND b.bucket_ts >= ?2 AND b.bucket_ts < ?3
           AND (?5 IS NULL OR COALESCE(NULLIF(b.owner_id, ''), r.owner_id) = ?5)
           AND (?6 IS NULL OR b.route_id = ?6)
           AND (?7 IS NULL OR b.api_key_id = ?7)
           AND (?8 IS NULL OR b.model = ?8)
           AND (?9 IS NULL OR b.provider_id = ?9)
         GROUP BY ts ORDER BY ts ASC"
    );
    let rows = sqlx::query_as::<_, TimePoint>(sqlx::AssertSqlSafe(sql))
        .bind(granularity)
        .bind(from)
        .bind(to)
        .bind(group_seconds)
        .bind(filter.owner_id)
        .bind(filter.route_id)
        .bind(filter.api_key_id)
        .bind(filter.model)
        .bind(filter.provider_id)
        .fetch_all(pool)
        .await?;
    Ok(downsample(rows, 500))
}

/// 顺序合并相邻点，保证时间序列不超过 `max_points`。
fn downsample(rows: Vec<TimePoint>, max_points: usize) -> Vec<TimePoint> {
    if rows.len() <= max_points || max_points == 0 {
        return rows;
    }
    let chunk = rows.len().div_ceil(max_points);
    let mut out = Vec::with_capacity(max_points);
    for part in rows.chunks(chunk) {
        let mut point = TimePoint {
            ts: part[0].ts,
            requests: 0,
            completed: 0,
            errors: 0,
            failovers: 0,
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: 0,
            latency_sum_ms: 0,
            latency_count: 0,
            ttft_sum_ms: 0,
            ttft_count: 0,
            bytes_in: 0,
            bytes_out: 0,
        };
        for row in part {
            point.requests += row.requests;
            point.completed += row.completed;
            point.errors += row.errors;
            point.failovers += row.failovers;
            point.input_tokens += row.input_tokens;
            point.output_tokens += row.output_tokens;
            point.cached_tokens += row.cached_tokens;
            point.latency_sum_ms += row.latency_sum_ms;
            point.latency_count += row.latency_count;
            point.ttft_sum_ms += row.ttft_sum_ms;
            point.ttft_count += row.ttft_count;
            point.bytes_in += row.bytes_in;
            point.bytes_out += row.bytes_out;
        }
        out.push(point);
    }
    out
}

/// 分组聚合行。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct BreakdownRow {
    pub key: String,
    pub label: String,
    pub route_id: Option<String>,
    pub route_name: Option<String>,
    pub owner_id: Option<String>,
    pub owner_name: Option<String>,
    /// 分组维度对应的资源已被物理删除（墓碑）。
    pub deleted: i64,
    /// 关联路由已归档。
    pub archived: i64,
    pub requests: i64,
    pub completed: i64,
    pub errors: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub latency_sum_ms: i64,
    pub latency_count: i64,
    pub hist_0: i64,
    pub hist_1: i64,
    pub hist_2: i64,
    pub hist_3: i64,
    pub hist_4: i64,
    pub hist_5: i64,
    pub hist_6: i64,
    pub hist_7: i64,
    pub hist_8: i64,
    pub hist_9: i64,
    pub hist_10: i64,
}

impl BreakdownRow {
    pub fn hist(&self) -> [i64; 11] {
        [
            self.hist_0,
            self.hist_1,
            self.hist_2,
            self.hist_3,
            self.hist_4,
            self.hist_5,
            self.hist_6,
            self.hist_7,
            self.hist_8,
            self.hist_9,
            self.hist_10,
        ]
    }
}

pub async fn breakdown(
    pool: &SqlitePool,
    granularity: &str,
    from: i64,
    to: i64,
    by: &str,
    filter: &UsageFilter<'_>,
) -> Result<Vec<BreakdownRow>> {
    let (select_key, group_by, deleted_expr) = match by {
        "api_key" => (
            "b.api_key_id AS key, COALESCE(k.name, k.key_prefix, kt.label, kt.key_prefix, b.api_key_id) AS label",
            "b.api_key_id, k.name, k.key_prefix, kt.label, kt.key_prefix",
            "CASE WHEN kt.resource_id IS NOT NULL THEN 1 ELSE 0 END",
        ),
        "model" => ("b.model AS key, b.model AS label", "b.model", "0"),
        "provider" => (
            "b.provider_id AS key, COALESCE(pk.name, pk.provider, pt.label, pt.provider, b.provider_id) AS label",
            "b.provider_id, pk.name, pk.provider, pt.label, pt.provider",
            "CASE WHEN pt.resource_id IS NOT NULL THEN 1 ELSE 0 END",
        ),
        "owner" => (
            "COALESCE(NULLIF(b.owner_id, ''), r.owner_id, '') AS key, COALESCE(ui.label, '未归属') AS label",
            "COALESCE(NULLIF(b.owner_id, ''), r.owner_id), ui.label",
            "0",
        ),
        _ => (
            "b.route_id AS key, COALESCE(r.name, rt.label, '已删除路由') AS label",
            "b.route_id, r.name, rt.label",
            "CASE WHEN rt.resource_id IS NOT NULL THEN 1 ELSE 0 END",
        ),
    };
    let sql = format!(
        "SELECT {select_key},
                b.route_id AS route_id,
                COALESCE(r.name, rt.label) AS route_name,
                COALESCE(NULLIF(b.owner_id, ''), r.owner_id) AS owner_id,
                COALESCE(ui.label, '未归属') AS owner_name,
                {deleted_expr} AS deleted,
                CASE WHEN r.archived_at IS NOT NULL THEN 1 ELSE 0 END AS archived,
                COALESCE(SUM(b.requests),0) AS requests,
                COALESCE(SUM(b.completed),0) AS completed,
                COALESCE(SUM(b.errors),0) AS errors,
                COALESCE(SUM(b.input_tokens),0) AS input_tokens,
                COALESCE(SUM(b.output_tokens),0) AS output_tokens,
                COALESCE(SUM(b.cached_tokens),0) AS cached_tokens,
                COALESCE(SUM(b.reasoning_tokens),0) AS reasoning_tokens,
                COALESCE(SUM(b.latency_sum_ms),0) AS latency_sum_ms,
                COALESCE(SUM(b.latency_count),0) AS latency_count,
                COALESCE(SUM(b.hist_0),0) AS hist_0, COALESCE(SUM(b.hist_1),0) AS hist_1,
                COALESCE(SUM(b.hist_2),0) AS hist_2, COALESCE(SUM(b.hist_3),0) AS hist_3,
                COALESCE(SUM(b.hist_4),0) AS hist_4, COALESCE(SUM(b.hist_5),0) AS hist_5,
                COALESCE(SUM(b.hist_6),0) AS hist_6, COALESCE(SUM(b.hist_7),0) AS hist_7,
                COALESCE(SUM(b.hist_8),0) AS hist_8, COALESCE(SUM(b.hist_9),0) AS hist_9,
                COALESCE(SUM(b.hist_10),0) AS hist_10
         FROM usage_bucket b
         LEFT JOIN routes r ON r.id = b.route_id
         LEFT JOIN resource_tombstone rt ON rt.kind = 'route' AND rt.resource_id = b.route_id
         LEFT JOIN users u ON u.id = r.owner_id
         LEFT JOIN user_identity ui ON ui.id = COALESCE(NULLIF(b.owner_id, ''), r.owner_id)
         LEFT JOIN api_keys k ON k.id = b.api_key_id
         LEFT JOIN resource_tombstone kt ON kt.kind = 'api_key' AND kt.resource_id = b.api_key_id
         LEFT JOIN provider_keys pk ON pk.id = b.provider_id
         LEFT JOIN resource_tombstone pt ON pt.kind = 'provider_key' AND pt.resource_id = b.provider_id
         WHERE b.granularity = ?1 AND b.bucket_ts >= ?2 AND b.bucket_ts < ?3
           AND (?4 IS NULL OR COALESCE(NULLIF(b.owner_id, ''), r.owner_id) = ?4)
           AND (?5 IS NULL OR b.route_id = ?5)
           AND (?6 IS NULL OR b.api_key_id = ?6)
           AND (?7 IS NULL OR b.model = ?7)
           AND (?8 IS NULL OR b.provider_id = ?8)
         GROUP BY {group_by} ORDER BY requests DESC"
    );
    let rows = sqlx::query_as::<_, BreakdownRow>(sqlx::AssertSqlSafe(sql))
        .bind(granularity)
        .bind(from)
        .bind(to)
        .bind(filter.owner_id)
        .bind(filter.route_id)
        .bind(filter.api_key_id)
        .bind(filter.model)
        .bind(filter.provider_id)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// 一次尝试的明细（由 `attempts_json` 解析而来，供界面展开查看换号过程）。
/// 保留策略清理。
/// 聚合桶保留清理（明细在独立库，见 `db::detail::delete_before`）。
const DELETE_BATCH: i64 = 5_000;

/// 分批删除过期聚合桶，避免一次 `DELETE` 长时间持有 SQLite 写锁。
pub async fn delete_old(pool: &SqlitePool, hour_before: i64, day_before: i64) -> Result<()> {
    loop {
        let result = sqlx::query(
            "DELETE FROM usage_bucket
             WHERE rowid IN (
                 SELECT rowid FROM usage_bucket
                  WHERE (granularity = 'hour' AND bucket_ts < ?1)
                     OR (granularity = 'day' AND bucket_ts < ?2)
                  LIMIT ?3
             )",
        )
        .bind(hour_before)
        .bind(day_before)
        .bind(DELETE_BATCH)
        .execute(pool)
        .await?;
        if result.rows_affected() == 0 {
            break;
        }
    }
    loop {
        let result = sqlx::query(
            "DELETE FROM usage_attempt_bucket
             WHERE rowid IN (
                 SELECT rowid FROM usage_attempt_bucket
                  WHERE (granularity = 'hour' AND bucket_ts < ?1)
                     OR (granularity = 'day' AND bucket_ts < ?2)
                  LIMIT ?3
             )",
        )
        .bind(hour_before)
        .bind(day_before)
        .bind(DELETE_BATCH)
        .execute(pool)
        .await?;
        if result.rows_affected() == 0 {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_delta_merge_is_additive() {
        let mut left = BucketDelta {
            requests: 2,
            input_tokens: 10,
            hist: [1; 11],
            ..Default::default()
        };
        let right = BucketDelta {
            requests: 3,
            input_tokens: 5,
            hist: [2; 11],
            ..Default::default()
        };
        left.merge(right);
        assert_eq!(left.requests, 5);
        assert_eq!(left.input_tokens, 15);
        assert!(left.hist.iter().all(|value| *value == 3));
    }

    #[test]
    fn attempt_delta_merge_is_additive() {
        let mut left = AttemptDelta {
            attempts: 1,
            success: 1,
            latency_sum_ms: 100,
            latency_count: 1,
            ..Default::default()
        };
        let right = AttemptDelta {
            attempts: 2,
            errors: 2,
            status_5xx: 2,
            latency_sum_ms: 300,
            latency_count: 2,
            ..Default::default()
        };
        left.merge(right);
        assert_eq!(left.attempts, 3);
        assert_eq!(left.errors, 2);
        assert_eq!(left.status_5xx, 2);
        assert_eq!(left.latency_sum_ms, 400);
        assert_eq!(left.latency_count, 3);
    }
}
