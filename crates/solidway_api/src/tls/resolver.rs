//! rustls 证书解析器：常规域名用当前证书，`acme-tls/1` ALPN 用临时挑战证书。
//!
//! 续期成功后仅替换 `ArcSwap` 中的证书，不重建监听器、不重启进程。

use std::sync::Arc;

use arc_swap::ArcSwap;
use rustls::ServerConfig;
use rustls::crypto::ring as provider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;

use crate::error::{ApiError, Result};

/// ACME TLS-ALPN-01 的 ALPN 标识。
pub const ACME_TLS_ALPN: &[u8] = b"acme-tls/1";

/// 证书解析器。
#[derive(Debug, Default)]
pub struct CertResolver {
    current: ArcSwap<Option<Arc<CertifiedKey>>>,
    challenge: ArcSwap<Option<Arc<CertifiedKey>>>,
}

impl CertResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// 安装（或热替换）当前证书。
    pub fn set_cert(&self, chain_pem: &str, key_pem: &str) -> Result<()> {
        let certified = parse_certified_key(chain_pem, key_pem)?;
        self.current.store(Arc::new(Some(Arc::new(certified))));
        Ok(())
    }

    /// 安装 TLS-ALPN-01 临时挑战证书（仅对 `acme-tls/1` 生效）。
    pub fn set_challenge_cert(&self, certified: CertifiedKey) {
        self.challenge.store(Arc::new(Some(Arc::new(certified))));
    }

    pub fn clear_challenge_cert(&self) {
        self.challenge.store(Arc::new(None));
    }

    pub fn has_cert(&self) -> bool {
        self.current.load().is_some()
    }

    /// 供 `https_port` 使用的 rustls 配置（h2 / http/1.1）。
    pub fn server_config(self: &Arc<Self>) -> Arc<ServerConfig> {
        let mut config = ServerConfig::builder_with_provider(Arc::new(provider::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("rustls 默认协议版本")
            .with_no_client_auth()
            .with_cert_resolver(Arc::clone(self) as Arc<dyn ResolvesServerCert>);
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Arc::new(config)
    }
}

impl ResolvesServerCert for CertResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let wants_acme = client_hello
            .alpn()
            .map(|mut protocols| protocols.any(|p| p == ACME_TLS_ALPN))
            .unwrap_or(false);
        if wants_acme {
            return self.challenge.load().as_ref().clone();
        }
        self.current.load().as_ref().clone()
    }
}

/// PEM（可能含多段证书链）→ `CertifiedKey`。
pub fn parse_certified_key(chain_pem: &str, key_pem: &str) -> Result<CertifiedKey> {
    let mut chain_reader = std::io::BufReader::new(chain_pem.as_bytes());
    let mut chain: Vec<CertificateDer<'static>> = Vec::new();
    for item in rustls_pemfile::certs(&mut chain_reader) {
        let der = item.map_err(|error| ApiError::internal(format!("证书链解析失败: {error}")))?;
        chain.push(der);
    }
    if chain.is_empty() {
        return Err(ApiError::internal("证书链为空".to_string()));
    }
    let mut key_reader = std::io::BufReader::new(key_pem.as_bytes());
    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|error| ApiError::internal(format!("私钥解析失败: {error}")))?
        .ok_or_else(|| ApiError::internal("私钥为空".to_string()))?;
    let signing_key = provider::default_provider()
        .key_provider
        .load_private_key(key)
        .map_err(|error| ApiError::internal(format!("私钥类型不支持: {error}")))?;
    Ok(CertifiedKey::new(chain, signing_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn self_signed() -> (String, String) {
        let key = rcgen::KeyPair::generate().unwrap();
        let params = rcgen::CertificateParams::new(vec!["gw.example.com".to_string()]).unwrap();
        let cert = params.self_signed(&key).unwrap();
        (cert.pem(), key.serialize_pem())
    }

    #[test]
    fn parses_self_signed_cert_and_key() {
        let (chain, key) = self_signed();
        let certified = parse_certified_key(&chain, &key).expect("parse");
        assert_eq!(certified.cert.len(), 1);
    }

    #[test]
    fn resolver_reports_loaded_cert() {
        let resolver = CertResolver::new();
        assert!(!resolver.has_cert());
        let (chain, key) = self_signed();
        resolver.set_cert(&chain, &key).unwrap();
        assert!(resolver.has_cert());
    }

    #[test]
    fn server_config_uses_h2_and_http11() {
        let resolver = Arc::new(CertResolver::new());
        let config = resolver.server_config();
        assert_eq!(
            config.alpn_protocols,
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        );
    }
}
