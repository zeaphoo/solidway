use sqlx::SqlitePool;

use crate::error::Result;

/// provider 健康状态行（health 行不存在 = 健康）。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProviderHealthRow {
    pub provider_id: String,
    pub state: String,
    pub reason: String,
    pub cooldown_until: Option<i64>,
    pub updated_at: i64,
}

pub async fn load_all(pool: &SqlitePool) -> Result<Vec<ProviderHealthRow>> {
    let rows = sqlx::query_as::<_, ProviderHealthRow>("SELECT * FROM provider_health")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn upsert(
    pool: &SqlitePool,
    provider_id: &str,
    state: &str,
    reason: &str,
    cooldown_until: Option<i64>,
    updated_at: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO provider_health (provider_id, state, reason, cooldown_until, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(provider_id) DO UPDATE SET
            state = excluded.state,
            reason = excluded.reason,
            cooldown_until = excluded.cooldown_until,
            updated_at = excluded.updated_at",
    )
    .bind(provider_id)
    .bind(state)
    .bind(reason)
    .bind(cooldown_until)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, provider_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM provider_health WHERE provider_id = ?1")
        .bind(provider_id)
        .execute(pool)
        .await?;
    Ok(())
}
