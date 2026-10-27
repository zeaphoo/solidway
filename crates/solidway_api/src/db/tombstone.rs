//! 轻资源 / 物理清除资源的身份墓碑。
//!
//! 墓碑**不设外键**：必须比活资源活得更久。写墓碑时不保存任何密钥材料。

use sqlx::SqliteConnection;

use crate::error::Result;

/// 写入（或幂等更新）一条墓碑。
#[allow(clippy::too_many_arguments)]
pub async fn upsert(
    conn: &mut SqliteConnection,
    kind: &str,
    resource_id: &str,
    parent_id: Option<&str>,
    owner_id: Option<&str>,
    owner_name: Option<&str>,
    label: Option<&str>,
    provider: Option<&str>,
    key_prefix: Option<&str>,
    deleted_at: i64,
    deleted_by: Option<&str>,
    reason: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO resource_tombstone
            (kind, resource_id, parent_id, owner_id, owner_name, label,
             provider, key_prefix, deleted_at, deleted_by, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT (kind, resource_id) DO UPDATE SET
            parent_id = excluded.parent_id,
            owner_id = excluded.owner_id,
            owner_name = excluded.owner_name,
            label = excluded.label,
            provider = excluded.provider,
            key_prefix = excluded.key_prefix,
            deleted_at = excluded.deleted_at,
            deleted_by = excluded.deleted_by,
            reason = excluded.reason",
    )
    .bind(kind)
    .bind(resource_id)
    .bind(parent_id)
    .bind(owner_id)
    .bind(owner_name)
    .bind(label)
    .bind(provider)
    .bind(key_prefix)
    .bind(deleted_at)
    .bind(deleted_by)
    .bind(reason)
    .execute(conn)
    .await?;
    Ok(())
}
