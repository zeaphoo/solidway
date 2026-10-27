//! 运行期状态快照（单行表 `runtime_snapshot`）：当前只存会话粘性。

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::Result;

/// 快照载荷。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapshotPayload {
    pub affinity: Vec<solidway_core::AffinitySeed>,
}

/// 读取快照；不存在、读取失败或内容非法时视为空快照（绝不阻塞启动）。
pub async fn load(pool: &SqlitePool) -> SnapshotPayload {
    let row = sqlx::query_as::<_, (String,)>("SELECT payload FROM runtime_snapshot WHERE id = 'current'")
        .fetch_optional(pool)
        .await;
    let Ok(Some((payload,))) = row else {
        return SnapshotPayload::default();
    };
    serde_json::from_str(&payload).unwrap_or_else(|error| {
        tracing::warn!(%error, "failed to parse runtime snapshot; starting empty");
        SnapshotPayload::default()
    })
}

/// 写入快照（覆盖单行）。
pub async fn save(pool: &SqlitePool, payload: &SnapshotPayload, now: i64) -> Result<()> {
    let raw = serde_json::to_string(payload)
        .map_err(|error| crate::ApiError::internal(format!("运行期快照序列化失败: {error}")))?;
    sqlx::query(
        "INSERT INTO runtime_snapshot (id, payload, updated_at) VALUES ('current', ?1, ?2)
         ON CONFLICT (id) DO UPDATE SET payload = excluded.payload, updated_at = excluded.updated_at",
    )
    .bind(raw)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}
