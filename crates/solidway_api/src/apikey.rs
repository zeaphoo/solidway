use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

/// 明文前缀，便于识别 key 来源。
pub const KEY_PREFIX: &str = "sw-";
const BODY_BYTES: usize = 32;
const DISPLAY_PREFIX_LEN: usize = 9;

/// 生成的 API Key：明文只在创建时返回一次，其余用于落库与展示。
pub struct GeneratedKey {
    pub plaintext: String,
    pub prefix: String,
    pub masked: String,
    pub hash: String,
}

pub fn generate() -> GeneratedKey {
    let bytes: [u8; BODY_BYTES] = rand::random();
    let plaintext = format!("{KEY_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
    let prefix: String = plaintext.chars().take(DISPLAY_PREFIX_LEN).collect();
    let masked = mask(&plaintext);
    let hash = hash(&plaintext);
    GeneratedKey {
        plaintext,
        prefix,
        masked,
        hash,
    }
}

/// 校验用的哈希（明文不需要回放，故与口令同思路只存哈希）。
pub fn hash(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 4 {
        return "****".to_string();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("****{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_key_shape() {
        let key = generate();
        assert!(key.plaintext.starts_with("sw-"));
        assert_eq!(key.prefix.len(), DISPLAY_PREFIX_LEN);
        assert!(key.masked.starts_with("****"));
        assert_eq!(key.hash.len(), 64);
        assert_ne!(key.hash, key.plaintext);
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(hash("sw-abc"), hash("sw-abc"));
        assert_ne!(hash("sw-abc"), hash("sw-abd"));
    }
}
