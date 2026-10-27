//! 证书续期调度：30 天窗口、指数退避（5 分钟起 / 1 小时上限 / 抖动）、
//! 24 小时内连续失败 5 次后暂停自动重试（人工重试或重启解除）。

use std::sync::Arc;
use std::time::Duration;

use crate::settings::TlsSettings;
use crate::tls::TlsRuntime;
use crate::tls::acme;
use crate::tls::store::StoredCert;

/// 续期窗口：到期前 30 天开始。
pub const RENEW_WINDOW_SECS: i64 = 30 * 24 * 3600;
/// 退避起点。
pub const BACKOFF_START: Duration = Duration::from_secs(5 * 60);
/// 退避上限。
pub const BACKOFF_CAP: Duration = Duration::from_secs(60 * 60);
/// 24 小时内累计失败次数上限，达到后暂停自动重试。
pub const MAX_FAILURES_PER_WINDOW: u32 = 5;
/// 失败统计窗口。
pub const FAILURE_WINDOW_SECS: i64 = 24 * 3600;

/// 第 `failures` 次失败后的等待时长（含 ±20% 抖动，上限 1 小时）。
pub fn backoff_delay(failures: u32, jitter: f64) -> Duration {
    let factor = 1u32 << failures.saturating_sub(1).min(8);
    let base = BACKOFF_START.saturating_mul(factor).min(BACKOFF_CAP);
    let scaled = base.as_secs_f64() * (1.0 + jitter.clamp(-0.2, 0.2));
    Duration::from_secs_f64(scaled.clamp(1.0, BACKOFF_CAP.as_secs_f64()))
}

/// 距离下次尝试的等待时间。
pub fn next_delay(now: i64, cert_not_after: Option<i64>, failures: u32) -> Duration {
    if failures > 0 {
        // 抖动用时间派生的伪随机，避免额外依赖随机源。
        let jitter = ((now.rem_euclid(1000) as f64 / 1000.0) - 0.5) * 0.4;
        return backoff_delay(failures, jitter);
    }
    match cert_not_after {
        // 无证书：立即签发。
        None => Duration::ZERO,
        Some(not_after) => {
            let renew_at = not_after - RENEW_WINDOW_SECS;
            if renew_at <= now {
                Duration::ZERO
            } else {
                Duration::from_secs((renew_at - now) as u64)
            }
        }
    }
}

/// 调度循环。暂停时只等人工重试信号。
pub async fn run(runtime: Arc<TlsRuntime>) {
    loop {
        if runtime.is_paused() {
            runtime.wait_for_retry().await;
            runtime.reset_failures();
            continue;
        }

        let delay = runtime.next_attempt_delay();
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = runtime.wait_for_retry() => {
                runtime.reset_failures();
                continue;
            }
        }

        match acme::issue(
            &runtime.settings,
            runtime.domains(),
            runtime.store(),
            runtime.challenges(),
            runtime.resolver(),
        )
        .await
        {
            Ok(issued) => {
                let stored = StoredCert {
                    domains: runtime.domains().to_vec(),
                    chain_pem: issued.chain_pem.clone(),
                    key_pem: issued.key_pem.clone(),
                    not_after: issued.not_after,
                    last_renewed_at: crate::state::now_unix(),
                };
                if let Err(error) = runtime.store().save_async(stored).await {
                    tracing::error!(%error, "failed to write certificate cache");
                    runtime.record_failure(format!("证书缓存写入失败: {error}"));
                    continue;
                }
                if let Err(error) = runtime
                    .resolver()
                    .set_cert(&issued.chain_pem, &issued.key_pem)
                {
                    tracing::error!(%error, "failed to load certificate into rustls");
                    runtime.record_failure(format!("证书装载失败: {error}"));
                    continue;
                }
                tracing::info!(not_after = issued.not_after, "certificate issued or renewed and hot-swapped");
                runtime.record_success(issued.not_after);
            }
            Err(error) => {
                tracing::warn!(%error, "certificate issuance failed");
                runtime.record_failure(error);
            }
        }
    }
}

/// 组装运行期设置（供测试复用）。
pub fn settings_snapshot(settings: &TlsSettings) -> TlsSettings {
    settings.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        let d1 = backoff_delay(1, 0.0);
        let d2 = backoff_delay(2, 0.0);
        let d3 = backoff_delay(3, 0.0);
        assert_eq!(d1, Duration::from_secs(5 * 60));
        assert_eq!(d2, Duration::from_secs(10 * 60));
        assert_eq!(d3, Duration::from_secs(20 * 60));

        // 上限 1 小时（含抖动也不越界）。
        for failures in 5..40 {
            assert!(backoff_delay(failures, 0.2) <= BACKOFF_CAP);
            assert!(backoff_delay(failures, -0.2) <= BACKOFF_CAP);
        }
        assert_eq!(backoff_delay(10, 0.0), BACKOFF_CAP);
    }

    #[test]
    fn next_delay_follows_30_day_window() {
        // 无证书：立即签发。
        assert_eq!(next_delay(1000, None, 0), Duration::ZERO);
        // 距到期超过 30 天：等到窗口开启。
        let not_after = 1000 + RENEW_WINDOW_SECS + 60;
        assert_eq!(next_delay(1000, Some(not_after), 0), Duration::from_secs(60));
        // 已进入窗口：立即续期。
        assert_eq!(next_delay(1000, Some(1000 + 10), 0), Duration::ZERO);
        // 有失败时优先退避（带抖动，落在 ±20% 区间内）。
        let backoff = next_delay(1000, Some(1000 + 10), 1);
        assert!(backoff >= Duration::from_secs(4 * 60), "{backoff:?}");
        assert!(backoff <= Duration::from_secs(6 * 60), "{backoff:?}");
    }
}
