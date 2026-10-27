//! `data_root/tls/` 下的账户与证书缓存：摘要命名、0600、原子写、回退与清理。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ApiError, Result};
use crate::tls::domain::domains_digest;

const ACCOUNT_FILE: &str = "account.json";

/// 一份缓存证书（含元数据）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCert {
    pub domains: Vec<String>,
    /// 证书链 PEM。
    pub chain_pem: String,
    /// 私钥 PEM。
    pub key_pem: String,
    pub not_after: i64,
    pub last_renewed_at: i64,
}

impl StoredCert {
    fn is_expired(&self, now: i64) -> bool {
        self.not_after <= now
    }
}

/// 证书缓存目录（`<data_root>/tls`）。
#[derive(Debug, Clone)]
pub struct CertStore {
    root: PathBuf,
}

impl CertStore {
    pub fn new(data_root: &Path) -> Self {
        Self {
            root: data_root.join("tls"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn ensure_root(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    fn cert_path(&self, digest: &str) -> PathBuf {
        self.root.join(format!("{digest}.cert.pem"))
    }

    fn meta_path(&self, digest: &str) -> PathBuf {
        self.root.join(format!("{digest}.meta.json"))
    }

    /// 与给定域名集合完全匹配的未过期证书。
    pub fn load_matching(&self, domains: &[String], now: i64) -> Option<StoredCert> {
        let cert = self.load_digest(&domains_digest(domains))?;
        (!cert.is_expired(now)).then_some(cert)
    }

    /// 异步版本：把磁盘 I/O 放到 blocking pool，避免占住 Tokio worker。
    pub async fn load_matching_async(&self, domains: &[String], now: i64) -> Option<StoredCert> {
        let store = self.clone();
        let domains = domains.to_vec();
        tokio::task::spawn_blocking(move || store.load_matching(&domains, now))
            .await
            .ok()
            .flatten()
    }

    /// 最近一份未过期证书（域名集合变化后的回退）。
    pub fn load_latest_valid(&self, now: i64) -> Option<StoredCert> {
        self.entries()
            .into_iter()
            .filter(|cert| !cert.is_expired(now))
            .max_by_key(|cert| cert.not_after)
    }

    /// 异步版本：把磁盘 I/O 放到 blocking pool。
    pub async fn load_latest_valid_async(&self, now: i64) -> Option<StoredCert> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.load_latest_valid(now))
            .await
            .ok()
            .flatten()
    }

    fn entries(&self) -> Vec<StoredCert> {
        let Ok(dir) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        dir.filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            let digest = name.strip_suffix(".meta.json")?;
            self.load_digest(digest)
        })
        .collect()
    }

    fn load_digest(&self, digest: &str) -> Option<StoredCert> {
        let meta = fs::read_to_string(self.meta_path(digest)).ok()?;
        serde_json::from_str(&meta).ok()
    }

    /// 写一份证书：`<digest>.cert.pem` + `<digest>.meta.json`，0600 且原子落盘。
    pub fn save(&self, cert: &StoredCert) -> Result<()> {
        self.ensure_root()?;
        let digest = domains_digest(&cert.domains);
        // 私钥与证书链同文件，权限 0600。
        let pem = format!("{}\n{}", cert.chain_pem.trim(), cert.key_pem.trim());
        write_private(&self.cert_path(&digest), pem.as_bytes())?;
        let meta = serde_json::to_vec_pretty(cert)
            .map_err(|error| ApiError::internal(format!("证书元数据序列化失败: {error}")))?;
        write_atomic(&self.meta_path(&digest), &meta)?;
        self.prune(&digest, cert.not_after);
        Ok(())
    }

    /// 异步版本：证书落盘包含 fsync，必须放到 blocking pool。
    pub async fn save_async(&self, cert: StoredCert) -> Result<()> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.save(&cert))
            .await
            .map_err(|error| ApiError::internal(format!("证书落盘任务失败: {error}")))?
    }

    /// 读取一份证书的 PEM（证书链 + 私钥）。
    pub fn read_pem(&self, domains: &[String]) -> Option<String> {
        fs::read_to_string(self.cert_path(&domains_digest(domains))).ok()
    }

    /// 保留当前摘要与最近一份未过期证书，其余清理。
    fn prune(&self, keep_digest: &str, keep_not_after: i64) {
        let Ok(dir) = fs::read_dir(&self.root) else {
            return;
        };
        // 找一份「非当前摘要」的最远到期证书。
        let mut best: Option<(String, i64)> = None;
        for entry in dir.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(digest) = name.strip_suffix(".meta.json") else {
                continue;
            };
            if digest == keep_digest {
                continue;
            }
            let Some(cert) = self.load_digest(digest) else {
                continue;
            };
            if best.as_ref().map(|(_, na)| cert.not_after > *na).unwrap_or(true) {
                best = Some((digest.to_string(), cert.not_after));
            }
        }
        let keep_other = best.map(|(digest, _)| digest);
        let Ok(dir) = fs::read_dir(&self.root) else {
            return;
        };
        for entry in dir.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name == ACCOUNT_FILE {
                continue;
            }
            let digest = name
                .strip_suffix(".meta.json")
                .or_else(|| name.strip_suffix(".cert.pem"));
            let Some(digest) = digest else { continue };
            if digest == keep_digest {
                continue;
            }
            if keep_other.as_deref() == Some(digest) && keep_not_after > 0 {
                continue;
            }
            let _ = fs::remove_file(&path);
        }
    }

    /// 异步版本：账户凭据读取放到 blocking pool。
    pub async fn load_account_async(&self) -> Option<String> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.load_account())
            .await
            .ok()
            .flatten()
    }

    /// ACME 账户凭据（JSON 原文）。
    pub fn load_account(&self) -> Option<String> {
        fs::read_to_string(self.root.join(ACCOUNT_FILE)).ok()
    }

    pub fn save_account(&self, credentials: &impl Serialize) -> Result<()> {
        self.ensure_root()?;
        let raw = serde_json::to_vec(credentials)
            .map_err(|error| ApiError::internal(format!("账户凭据序列化失败: {error}")))?;
        write_private(&self.root.join(ACCOUNT_FILE), &raw)
    }

    /// 异步版本：账户凭据落盘放到 blocking pool。
    pub async fn save_account_async(&self, credentials: &impl Serialize) -> Result<()> {
        let raw = serde_json::to_vec(credentials)
            .map_err(|error| ApiError::internal(format!("账户凭据序列化失败: {error}")))?;
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            store.ensure_root()?;
            write_private(&store.root.join(ACCOUNT_FILE), &raw)
        })
        .await
        .map_err(|error| ApiError::internal(format!("账户凭据落盘任务失败: {error}")))?
    }
}

/// 0600 + 原子写（tmp + rename）。
fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cert(domains: &[&str], not_after: i64) -> StoredCert {
        StoredCert {
            domains: domains.iter().map(|d| d.to_string()).collect(),
            chain_pem: "-----BEGIN CERTIFICATE-----\nchain\n-----END CERTIFICATE-----".to_string(),
            key_pem: "-----BEGIN PRIVATE KEY-----\nkey\n-----END PRIVATE KEY-----".to_string(),
            not_after,
            last_renewed_at: 1,
        }
    }

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidway-tls-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn saves_and_loads_by_digest() {
        let root = temp_root();
        let store = CertStore::new(&root);
        let domains = vec!["gw.example.com".to_string()];
        store.save(&cert(&["gw.example.com"], 2000)).unwrap();

        let loaded = store.load_matching(&domains, 1000).expect("matching cert");
        assert_eq!(loaded.not_after, 2000);
        let pem = store.read_pem(&domains).expect("pem");
        assert!(pem.contains("PRIVATE KEY"));
        assert!(pem.contains("CERTIFICATE"));

        // 过期证书不作为命中结果。
        assert!(store.load_matching(&domains, 2500).is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root();
        let store = CertStore::new(&root);
        store.save(&cert(&["gw.example.com"], 2000)).unwrap();
        store.save_account(&serde_json::json!({"k": 1})).unwrap();

        for name in [
            format!("{}.cert.pem", domains_digest(&["gw.example.com".to_string()])),
            "account.json".to_string(),
        ] {
            let mode = fs::metadata(root.join("tls").join(&name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{name} 权限应为 0600");
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn keeps_one_previous_cert_and_drops_the_rest() {
        let root = temp_root();
        let store = CertStore::new(&root);
        store.save(&cert(&["a.example.com"], 1000)).unwrap();
        store.save(&cert(&["b.example.com"], 3000)).unwrap();
        store.save(&cert(&["c.example.com"], 2000)).unwrap();

        // 当前 c（2000），另一份保留到期最远的 b（3000），a 被清理。
        assert!(store.save(&cert(&["c.example.com"], 2000)).is_ok());
        assert!(store.load_matching(&["b.example.com".to_string()], 500).is_some());
        assert!(store.load_matching(&["a.example.com".to_string()], 500).is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn falls_back_to_latest_valid() {
        let root = temp_root();
        let store = CertStore::new(&root);
        store.save(&cert(&["old.example.com"], 1200)).unwrap();
        store.save(&cert(&["new.example.com"], 4000)).unwrap();

        // 域名集合不匹配 → 回退到最近一份未过期证书。
        assert!(store.load_matching(&["other.example.com".to_string()], 1000).is_none());
        let latest = store.load_latest_valid(1000).expect("fallback cert");
        assert_eq!(latest.not_after, 4000);
        fs::remove_dir_all(&root).ok();
    }
}
