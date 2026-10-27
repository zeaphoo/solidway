use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

pub const COOKIE_NAME: &str = "solidway_session";
const TOKEN_BYTES: usize = 32;

/// 生成随机会话令牌（下发给 Cookie 的明文）。
pub fn generate_token() -> String {
    let bytes: [u8; TOKEN_BYTES] = rand::random();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// 会话令牌的服务端存储形式（SHA-256 hex）。
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex::encode(digest)
}
