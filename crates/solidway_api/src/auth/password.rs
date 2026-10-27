use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use tokio::sync::Semaphore;

use crate::error::{ApiError, Result};

/// 同时执行的口令运算上限。Argon2 会占用 CPU 与约 19MB 内存，
/// 不能让公开登录接口无限并发。
const PASSWORD_CONCURRENCY: usize = 2;
static PASSWORD_OPS: Semaphore = Semaphore::const_new(PASSWORD_CONCURRENCY);

/// 使用 Argon2id 生成口令哈希（PHC 字符串）。
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| ApiError::internal(format!("口令哈希失败: {e}")))?
        .to_string();
    Ok(hash)
}

/// CPU 密集的口令哈希放到 blocking pool，避免占住 Tokio worker。
pub async fn hash_password_async(password: String) -> Result<String> {
    let _permit = PASSWORD_OPS
        .acquire()
        .await
        .map_err(|_| ApiError::internal("口令哈希队列已关闭"))?;
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|error| ApiError::internal(format!("口令哈希任务失败: {error}")))?
}

/// 校验口令；CPU 密集运算同样放到 blocking pool。
pub async fn verify_password_async(password: String, hash: String) -> bool {
    let Ok(_permit) = PASSWORD_OPS.acquire().await else {
        return false;
    };
    match tokio::task::spawn_blocking(move || verify_password(&password, &hash)).await {
        Ok(matches) => matches,
        Err(error) => {
            tracing::error!(%error, "password verify task failed");
            false
        }
    }
}

/// 校验口令；任何解析 / 校验失败都视为不匹配。
pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// 锁定态口令哈希：非法 PHC 字符串，`verify_password` 恒为 `false`。
/// 用于「尚未设置口令」的 admin（引导中）：任何口令都无法登录，解锁即写入真实哈希。
pub const LOCKED_HASH: &str = "!";

/// 该哈希是否处于锁定态（无口令可用）。
pub fn is_locked(hash: &str) -> bool {
    PasswordHash::new(hash).is_err()
}

/// 用户名不存在 / 已禁用时用于抹平登录时序的哑 PHC 哈希（R3-F-6）。
///
/// 固定常量（Argon2id，默认参数）避免每次未知用户名登录都重新生成哈希；
/// 校验结果永远被丢弃，绝不用于放行。
pub const DUMMY_TIMING_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$zY7XV6sEzhYdc5dOlLlGbQ$Ffo596Z2z187ZosOqOQ8jxM3Zr37M6KpUVCZtOXvfww";

/// 生成随机初始口令（字母 + 数字）。
pub fn generate_password(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";
    (0..len)
        .map(|_| {
            let idx = rand::random::<u32>() as usize % ALPHABET.len();
            ALPHABET[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dummy_timing_hash_is_valid_phc_and_always_rejects() {
        // 必须可解析，否则 verify_password 会提前返回，时序抹平失效（R3-F-6）。
        assert!(!is_locked(DUMMY_TIMING_HASH));
        assert!(!verify_password("guess-1", DUMMY_TIMING_HASH));
        assert!(!verify_password("guess-2", DUMMY_TIMING_HASH));
    }
}
