//! 故障告警：把 incident 事件通过 Webhook 推送出去。
//!
//! 约束：异步、不阻塞转发；5 秒超时；最多 3 次退避重试；
//! 同一 provider + 原因在最小间隔内不重复；请求体不含任何凭据明文。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

use crate::db::incidents;
use crate::settings::{self, AlertSettings};
use crate::state::{AppState, now_unix};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(4),
    Duration::from_secs(9),
];

/// 最近一次发送时刻（内存去抖键：provider + kind）。
static LAST_SENT: Mutex<Option<HashMap<String, i64>>> = Mutex::new(None);

fn last_sent_map() -> std::sync::MutexGuard<'static, Option<HashMap<String, i64>>> {
    LAST_SENT
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

#[derive(Debug, Clone, Serialize)]
struct WebhookPayload {
    event: String,
    id: String,
    provider_id: String,
    route_id: String,
    kind: String,
    severity: String,
    started_at: i64,
    ended_at: Option<i64>,
    attempts: i64,
    last_status: Option<i64>,
    last_error: Option<String>,
    at: i64,
}

impl WebhookPayload {
    fn from_row(event: &str, row: &incidents::IncidentRow) -> Self {
        Self {
            event: event.to_string(),
            id: row.id.clone(),
            provider_id: row.provider_id.clone(),
            route_id: row.route_id.clone(),
            kind: row.kind.clone(),
            severity: row.severity.clone(),
            started_at: row.started_at,
            ended_at: row.ended_at,
            attempts: row.attempts,
            last_status: row.last_status,
            last_error: row.last_error.clone(),
            at: now_unix(),
        }
    }
}

/// 新开故障：`severity != transient` 时推送。
pub async fn notify_opened(state: &AppState, incident_id: &str) {
    let alert = match settings::load_alert(&state.pool).await {
        Ok(alert) => alert,
        Err(error) => {
            tracing::warn!(%error, "failed to load alert settings");
            return;
        }
    };
    let Some(url) = webhook_url(&alert) else {
        return;
    };

    let row: Option<incidents::IncidentRow> = sqlx::query_as(
        "SELECT id, provider_id, route_id, owner_id, kind, severity, state,
                started_at, last_seen_at, ended_at, attempts, last_status,
                last_error, resolved_by, notified_at,
                NULL AS provider, NULL AS provider_name, NULL AS route_name,
                NULL AS owner_name, 0 AS route_archived, 0 AS route_deleted,
                0 AS provider_deleted
           FROM provider_incident WHERE id = ?1",
    )
    .bind(incident_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else {
        return;
    };
    if row.severity == "transient" {
        return;
    }
    let Some(sent_at) = should_send(&row.provider_id, &row.kind, alert.min_interval_minutes) else {
        tracing::debug!(
            provider = %row.provider_id,
            kind = %row.kind,
            "alert suppressed by minimum interval"
        );
        return;
    };

    let payload = WebhookPayload::from_row("provider_incident_open", &row);
    let pool = state.pool.clone();
    let id = row.id.clone();
    let provider_id = row.provider_id.clone();
    let kind = row.kind.clone();
    tokio::spawn(async move {
        if send(&url, &payload).await {
            let _ = incidents::mark_notified(&pool, &id, now_unix()).await;
        } else {
            // 发送失败：释放去抖键，让后续同故障仍能尝试告警，而不是静默丢一个窗口。
            rollback_send(&provider_id, &kind, sent_at);
        }
    });
}

/// 故障关闭：按设置决定是否推送恢复通知。
pub async fn notify_closed(state: &AppState, incident_ids: &[String]) {
    let alert = match settings::load_alert(&state.pool).await {
        Ok(alert) => alert,
        Err(_) => return,
    };
    if !alert.notify_recovery {
        return;
    }
    let Some(url) = webhook_url(&alert) else {
        return;
    };
    for id in incident_ids {
        let row: Option<incidents::IncidentRow> = sqlx::query_as(
            "SELECT id, provider_id, route_id, owner_id, kind, severity, state,
                    started_at, last_seen_at, ended_at, attempts, last_status,
                    last_error, resolved_by, notified_at,
                    NULL AS provider, NULL AS provider_name, NULL AS route_name,
                    NULL AS owner_name, 0 AS route_archived, 0 AS route_deleted,
                    0 AS provider_deleted
               FROM provider_incident WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();
        let Some(row) = row else { continue };
        // 恢复通知不依赖开告警是否成功：非瞬时故障即使开告警发送失败，
        // 关闭时也要补发，避免整段故障（开 + 关）静默。瞬时故障本就不告警。
        if !should_notify_recovery(&row.severity) {
            continue;
        }
        let payload = WebhookPayload::from_row("provider_incident_closed", &row);
        let url = url.clone();
        tokio::spawn(async move {
            let _ = send(&url, &payload).await;
        });
    }
}

fn webhook_url(alert: &AlertSettings) -> Option<String> {
    alert
        .webhook_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

/// 判断是否只对非瞬时故障发恢复通知。
fn should_notify_recovery(severity: &str) -> bool {
    severity != "transient"
}

/// 去抖：同 provider + kind 在 `min_interval_minutes` 内不重复发送。
///
/// 返回本次占用去抖键的时刻；被抑制时返回 `None`。发送失败可据此精确回滚。
fn should_send(provider_id: &str, kind: &str, min_interval_minutes: u64) -> Option<i64> {
    let now = now_unix();
    let interval = (min_interval_minutes as i64) * 60;
    let mut guard = last_sent_map();
    let map = guard.get_or_insert_with(HashMap::new);
    let key = format!("{provider_id}|{kind}");
    if let Some(last) = map.get(&key)
        && now - *last < interval
    {
        return None;
    }
    map.insert(key, now);
    Some(now)
}

/// 发送失败时回滚去抖键；仅当键值仍是本次占用时刻才删除，避免误删更新的告警。
fn rollback_send(provider_id: &str, kind: &str, sent_at: i64) {
    let mut guard = last_sent_map();
    let Some(map) = guard.as_mut() else {
        return;
    };
    let key = format!("{provider_id}|{kind}");
    if map.get(&key) == Some(&sent_at) {
        map.remove(&key);
    }
}

/// 告警 HTTP client 复用（R2-A-6①）：连接池 / TLS 配置只建一次。
static ALERT_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn alert_client() -> Option<&'static reqwest::Client> {
    if let Some(client) = ALERT_CLIENT.get() {
        return Some(client);
    }
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| tracing::warn!(%error, "failed to build alert HTTP client"))
        .ok()?;
    // 并发首建时可能有人先 set 成功；以已存在的实例为准。
    let _ = ALERT_CLIENT.set(client);
    ALERT_CLIENT.get()
}

/// 发送一次（含退避重试）；返回是否成功。
async fn send(url: &str, payload: &WebhookPayload) -> bool {
    let Some(client) = alert_client() else {
        return false;
    };
    for (attempt, delay) in RETRY_DELAYS.iter().enumerate() {
        let body = match serde_json::to_vec(payload) {
            Ok(body) => body,
            Err(error) => {
                tracing::warn!(%error, "failed to serialize alert payload");
                return false;
            }
        };
        match client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => return true,
            Ok(response) => {
                tracing::warn!(status = %response.status(), attempt = attempt + 1, "alert delivery failed");
            }
            Err(error) => {
                tracing::warn!(%error, attempt = attempt + 1, "alert delivery error");
            }
        }
        tokio::time::sleep(*delay).await;
    }
    false
}

/// 测试用：清空去抖状态。
pub fn reset_dedup_for_test() {
    *last_sent_map() = Some(HashMap::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_blocks_within_interval() {
        // 各用例使用不同的 provider 键，避免并行执行时共享静态去抖表互相干扰。
        assert!(should_send("p1", "http_401", 30).is_some());
        assert!(should_send("p1", "http_401", 30).is_none());
        // 不同原因互不影响。
        assert!(should_send("p1", "insufficient_quota", 30).is_some());
        // 间隔为 0 时不去抖。
        assert!(should_send("p1", "http_401", 0).is_some());
    }

    #[test]
    fn failed_send_reopens_dedup_for_same_incident() {
        let sent_at = should_send("p2", "http_401", 30).expect("首次应允许发送");
        // 发送窗口内同故障被去抖。
        assert!(should_send("p2", "http_401", 30).is_none());

        // 模拟发送失败：回滚后同故障再开仍应告警。
        rollback_send("p2", "http_401", sent_at);
        assert!(
            should_send("p2", "http_401", 30).is_some(),
            "发送失败回滚去抖键后，同故障再开必须仍能告警"
        );
    }

    #[test]
    fn rollback_keeps_newer_dedup_entry() {
        let old = should_send("p3", "http_401", 30).expect("首次应允许发送");
        // 模拟同一键已被另一次发送更新（时间戳不同）。
        {
            let mut guard = last_sent_map();
            guard
                .as_mut()
                .expect("去抖表")
                .insert("p3|http_401".to_string(), old + 100);
        }
        rollback_send("p3", "http_401", old);
        assert!(
            should_send("p3", "http_401", 30).is_none(),
            "回滚旧时刻不得误删更新的去抖记录"
        );
    }

    #[test]
    fn recovery_notification_skips_only_transient() {
        assert!(should_notify_recovery("degraded"));
        assert!(should_notify_recovery("manual_action"));
        assert!(!should_notify_recovery("transient"));
    }
}
