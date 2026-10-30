use std::path::Path;

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::error::{ApiError, Result};

type HmacSha256 = Hmac<Sha256>;

const MASTER_KEY_FILE: &str = "master.key";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
/// HKDF 域分离 info，与 llmgateway 同思路。
const HKDF_INFO: &[u8] = b"solidway-provider-key-token:v1";
const FORMAT_PREFIX: &str = "sw:v1";

struct KeyEntry {
    kid: String,
    master: [u8; KEY_BYTES],
}

/// provider 凭据的对称加密（AES-256-GCM + HKDF-SHA256 + kid + AAD）。
pub struct Cipher {
    keyring: Vec<KeyEntry>,
    current: usize,
}

impl Cipher {
    pub fn new(masters: Vec<[u8; KEY_BYTES]>) -> Result<Self> {
        if masters.is_empty() {
            return Err(ApiError::internal("主密钥为空"));
        }
        let keyring = masters
            .into_iter()
            .map(|master| KeyEntry {
                kid: kid_of(&master),
                master,
            })
            .collect();
        Ok(Self { keyring, current: 0 })
    }

    fn derive_data_key(master: &[u8; KEY_BYTES]) -> Result<[u8; KEY_BYTES]> {
        let hk = Hkdf::<Sha256>::new(None, master);
        let mut okm = [0u8; KEY_BYTES];
        hk.expand(HKDF_INFO, &mut okm)
            .map_err(|_| ApiError::internal("HKDF 派生失败"))?;
        Ok(okm)
    }

    /// 加密：返回 `sw:v1:<kid>:<iv>:<ct>:<tag>`。
    pub fn encrypt(&self, plaintext: &str, aad: &str) -> Result<String> {
        let entry = &self.keyring[self.current];
        let data_key = Self::derive_data_key(&entry.master)?;
        let cipher = Aes256Gcm::new_from_slice(&data_key)
            .map_err(|_| ApiError::internal("初始化 cipher 失败"))?;
        let nonce: [u8; NONCE_BYTES] = rand::random();
        let mut ciphertext = cipher
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| ApiError::internal("加密失败"))?;
        let tag = ciphertext.split_off(ciphertext.len() - TAG_BYTES);
        Ok(format!(
            "{FORMAT_PREFIX}:{}:{}:{}:{}",
            entry.kid,
            URL_SAFE_NO_PAD.encode(nonce),
            URL_SAFE_NO_PAD.encode(&ciphertext),
            URL_SAFE_NO_PAD.encode(&tag),
        ))
    }

    /// 解密；`kid` 未知或 AAD 不匹配均报错。
    pub fn decrypt(&self, encoded: &str, aad: &str) -> Result<String> {
        let parts: Vec<&str> = encoded.split(':').collect();
        if parts.len() != 6 || parts[0] != "sw" || parts[1] != "v1" {
            return Err(ApiError::internal("密文格式非法"));
        }
        let kid = parts[2];
        let entry = self
            .keyring
            .iter()
            .find(|e| e.kid == kid)
            .ok_or_else(|| ApiError::internal(format!("未知密钥 id: {kid}")))?;
        let nonce = URL_SAFE_NO_PAD
            .decode(parts[3])
            .map_err(|_| ApiError::internal("密文 nonce 非法"))?;
        let mut ct = URL_SAFE_NO_PAD
            .decode(parts[4])
            .map_err(|_| ApiError::internal("密文内容非法"))?;
        let tag = URL_SAFE_NO_PAD
            .decode(parts[5])
            .map_err(|_| ApiError::internal("密文 tag 非法"))?;
        if nonce.len() != NONCE_BYTES || tag.len() != TAG_BYTES {
            return Err(ApiError::internal("密文段长度非法"));
        }
        let nonce_bytes: [u8; NONCE_BYTES] = nonce
            .as_slice()
            .try_into()
            .map_err(|_| ApiError::internal("密文 nonce 长度非法"))?;
        ct.extend_from_slice(&tag);
        let data_key = Self::derive_data_key(&entry.master)?;
        let cipher = Aes256Gcm::new_from_slice(&data_key)
            .map_err(|_| ApiError::internal("初始化 cipher 失败"))?;
        let plaintext = cipher
            .decrypt(
                &Nonce::from(nonce_bytes),
                Payload {
                    msg: &ct,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| ApiError::internal("解密失败（密钥或 AAD 不匹配）"))?;
        String::from_utf8(plaintext).map_err(|_| ApiError::internal("明文非 UTF-8"))
    }

    /// 用于展示的掩码：保留首尾各 4 个字符，便于人工核对是哪一把凭据。
    /// 总长（含分隔）不足时整体脱敏，不泄露任何字符。
    pub fn mask(token: &str) -> String {
        const EDGE: usize = 4;
        let chars: Vec<char> = token.chars().collect();
        // 首尾各 4 且中间至少留 8 个被遮蔽字符，否则整体脱敏。
        if chars.len() < EDGE * 2 + 8 {
            return "****".to_string();
        }
        let head: String = chars[..EDGE].iter().collect();
        let tail: String = chars[chars.len() - EDGE..].iter().collect();
        format!("{head}****{tail}")
    }

    /// HMAC-SHA256 指纹，用于归因 / 去重，不泄露明文。
    pub fn fingerprint(&self, token: &str) -> Result<String> {
        let entry = &self.keyring[self.current];
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&entry.master)
            .map_err(|_| ApiError::internal("初始化 HMAC 失败"))?;
        mac.update(token.as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }
}

fn kid_of(master: &[u8; KEY_BYTES]) -> String {
    let digest = Sha256::digest(master);
    hex::encode(&digest[..4])
}

/// 从 `--master-key` 或 `<data_root>/master.key` 加载主密钥。
///
/// 优先级：命令行内联密钥 → 文件 → 首次自动生成（仅文件模式）。
pub fn load_master_keys(data_root: &Path, override_key: Option<&str>) -> Result<Vec<[u8; KEY_BYTES]>> {
    if let Some(raw) = override_key {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(ApiError::internal("--master-key 不能为空"));
        }
        return parse_keyring(raw);
    }

    let path = data_root.join(MASTER_KEY_FILE);
    if !path.exists() {
        let key: [u8; KEY_BYTES] = rand::random();
        write_master_key(&path, STANDARD.encode(key).as_bytes())?;
        tracing::warn!(
            path = %path.display(),
            "master key file created; back it up separately, provider credentials cannot be decrypted without it"
        );
    }
    let text = std::fs::read_to_string(&path).map_err(ApiError::from)?;
    parse_keyring(text.trim())
}

fn parse_keyring(raw: &str) -> Result<Vec<[u8; KEY_BYTES]>> {
    let mut keys = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let bytes = decode_key(part)
            .ok_or_else(|| ApiError::internal("主密钥格式非法（应为 base64 或 hex）"))?;
        if bytes.len() != KEY_BYTES {
            return Err(ApiError::internal(format!(
                "主密钥长度非法：期望 {KEY_BYTES} 字节，实际 {}",
                bytes.len()
            )));
        }
        let mut key = [0u8; KEY_BYTES];
        key.copy_from_slice(&bytes);
        keys.push(key);
    }
    if keys.is_empty() {
        return Err(ApiError::internal("未提供有效主密钥"));
    }
    Ok(keys)
}

fn decode_key(value: &str) -> Option<Vec<u8>> {
    // 32 字节密钥的 hex 编码恰好是 64 个字符，也全部落在 base64 字母表内，
    // 若按「第一个解码成功者」返回会被 base64 抢先解成 48 字节。这里收集三种
    // 编码的结果，优先取长度正确的那个；都不匹配时回退首个成功结果，供上层报长度错。
    let mut fallback = None;
    for bytes in [
        hex::decode(value).ok(),
        STANDARD.decode(value).ok(),
        URL_SAFE_NO_PAD.decode(value).ok(),
    ]
    .into_iter()
    .flatten()
    {
        if bytes.len() == KEY_BYTES {
            return Some(bytes);
        }
        fallback.get_or_insert(bytes);
    }
    fallback
}

/// 原子创建主密钥文件（R3-F9）。
///
/// Unix 下用 `create_new + mode(0o600)` 一步创建，权限从文件诞生即生效，
/// 不存在「先按 umask 写入、再 chmod」的窗口。
#[cfg(unix)]
fn write_master_key(path: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(ApiError::from)?;
    file.write_all(contents).map_err(ApiError::from)
}

#[cfg(not(unix))]
fn write_master_key(path: &Path, contents: &[u8]) -> Result<()> {
    std::fs::write(path, contents).map_err(ApiError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> [u8; KEY_BYTES] {
        [byte; KEY_BYTES]
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let cipher = Cipher::new(vec![key(1)]).unwrap();
        let ct = cipher.encrypt("sk-secret-value", "provider_key|abc").unwrap();
        assert!(ct.starts_with("sw:v1:"));
        assert!(!ct.contains("sk-secret-value"));
        assert_eq!(cipher.decrypt(&ct, "provider_key|abc").unwrap(), "sk-secret-value");
    }

    #[test]
    fn aad_mismatch_fails() {
        let cipher = Cipher::new(vec![key(1)]).unwrap();
        let ct = cipher.encrypt("sk-secret-value", "provider_key|abc").unwrap();
        assert!(cipher.decrypt(&ct, "provider_key|other").is_err());
    }

    #[test]
    fn key_rotation_reads_old_ciphertext() {
        let old = Cipher::new(vec![key(1)]).unwrap();
        let old_ct = old.encrypt("tok", "aad").unwrap();
        assert!(old_ct.contains(&kid_of(&key(1))));

        let rotated = Cipher::new(vec![key(2), key(1)]).unwrap();
        assert_eq!(rotated.decrypt(&old_ct, "aad").unwrap(), "tok");

        let new_ct = rotated.encrypt("tok2", "aad").unwrap();
        assert!(new_ct.contains(&kid_of(&key(2))));
        assert_ne!(kid_of(&key(1)), kid_of(&key(2)));
    }

    #[test]
    fn unknown_kid_fails() {
        let cipher = Cipher::new(vec![key(1)]).unwrap();
        let ct = cipher.encrypt("tok", "aad").unwrap();
        let other = Cipher::new(vec![key(9)]).unwrap();
        assert!(other.decrypt(&ct, "aad").is_err());
    }

    #[test]
    fn mask_keeps_head_and_tail() {
        assert_eq!(Cipher::mask("sk-1234567890abcd"), "sk-1****abcd");
        // 太短：整体脱敏，避免首尾直接拼出原值。
        assert_eq!(Cipher::mask("sk-short"), "****");
        assert_eq!(Cipher::mask("abc"), "****");
    }

    #[test]
    fn master_key_accepts_hex_and_base64() {
        let raw = [0x5au8; KEY_BYTES];
        let hex = hex::encode(raw);
        assert_eq!(hex.len(), 64, "32 字节的 hex 应为 64 字符");

        let from_hex = parse_keyring(&hex).expect("hex 形式应可解析");
        let from_base64 = parse_keyring(&STANDARD.encode(raw)).expect("base64 形式应可解析");
        assert_eq!(from_hex, vec![raw]);
        assert_eq!(from_base64, vec![raw]);
    }
}
