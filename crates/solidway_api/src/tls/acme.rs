//! ACME 签发：账户复用、下单、挑战应答、finalize 与证书下载。
//!
//! 只做协议原语；续期调度、退避、暂停与状态全在 [`crate::tls::scheduler`]，
//! 这样 `GET /api/tls/status` 的语义由本仓库定义（见 `docs/adr/0002-tls-acme-library.md`）。

use std::sync::Arc;

use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, NewAccount,
    NewOrder, OrderStatus, RetryPolicy,
};
use rustls::sign::CertifiedKey;
use x509_parser::prelude::{FromDer, X509Certificate};

use crate::settings::{TlsChallenge, TlsSettings};
use crate::tls::challenge::ChallengeStore;
use crate::tls::resolver::{CertResolver, parse_certified_key};
use crate::tls::store::CertStore;

/// 一次成功签发的产物。
#[derive(Debug, Clone)]
pub struct IssuedCert {
    pub key_pem: String,
    pub chain_pem: String,
    pub not_after: i64,
}

/// 申请 / 续期一张覆盖 `domains` 的 SAN 证书。
pub async fn issue(
    settings: &TlsSettings,
    domains: &[String],
    store: &CertStore,
    challenges: &ChallengeStore,
    resolver: &Arc<CertResolver>,
) -> Result<IssuedCert, String> {
    let account = load_or_create_account(settings, store).await?;

    let identifiers: Vec<Identifier> = domains.iter().cloned().map(Identifier::Dns).collect();
    let mut order = account
        .new_order(&NewOrder::new(&identifiers))
        .await
        .map_err(|error| format!("创建 ACME 订单失败: {error}"))?;

    let pending = match order.state().status {
        OrderStatus::Pending => true,
        OrderStatus::Ready | OrderStatus::Processing | OrderStatus::Valid => false,
        other => return Err(format!("ACME 订单状态异常: {other:?}")),
    };

    if pending {
        answer_challenges(settings, &mut order, challenges, resolver).await?;
    }

    let status = order
        .poll_ready(&RetryPolicy::default())
        .await
        .map_err(|error| format!("等待 ACME 校验完成失败: {error}"))?;
    if status != OrderStatus::Ready {
        return Err(format!("ACME 订单未就绪: {status:?}"));
    }

    let key_pem = order
        .finalize()
        .await
        .map_err(|error| format!("ACME finalize 失败: {error}"))?;
    let chain_pem = order
        .poll_certificate(&RetryPolicy::default())
        .await
        .map_err(|error| format!("下载 ACME 证书失败: {error}"))?;

    challenges.clear();
    resolver.clear_challenge_cert();

    let not_after = not_after(&chain_pem)
        .ok_or_else(|| "无法解析证书到期时间".to_string())?;

    // 先验证能装进 rustls，再落盘，避免缓存出无法加载的证书。
    parse_certified_key(&chain_pem, &key_pem).map_err(|error| error.to_string())?;

    Ok(IssuedCert {
        key_pem,
        chain_pem,
        not_after,
    })
}

async fn load_or_create_account(
    settings: &TlsSettings,
    store: &CertStore,
) -> Result<Account, String> {
    let directory = settings.directory.url().to_string();
    let builder = || {
        Account::builder().map_err(|error| format!("ACME 客户端初始化失败: {error}"))
    };

    if let Some(raw) = store.load_account_async().await {
        match serde_json::from_str::<AccountCredentials>(&raw) {
            Ok(credentials) => {
                return builder()?
                    .from_credentials(credentials)
                    .await
                    .map_err(|error| format!("恢复 ACME 账户失败: {error}"));
            }
            Err(error) => {
                tracing::warn!(%error, "ACME account credentials corrupted; re-registering");
            }
        }
    }

    let contact = format!("mailto:{}", settings.email.trim());
    let contacts: Vec<&str> = vec![contact.as_str()];
    let (account, credentials) = builder()?
        .create(
            &NewAccount {
                contact: &contacts,
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            directory,
            None,
        )
        .await
        .map_err(|error| format!("注册 ACME 账户失败: {error}"))?;
    store
        .save_account_async(&credentials)
        .await
        .map_err(|error| format!("保存 ACME 账户失败: {error}"))?;
    Ok(account)
}

async fn answer_challenges(
    settings: &TlsSettings,
    order: &mut instant_acme::Order,
    challenges: &ChallengeStore,
    resolver: &Arc<CertResolver>,
) -> Result<(), String> {
    let challenge_type = || match settings.challenge {
        TlsChallenge::Http01 => ChallengeType::Http01,
        TlsChallenge::TlsAlpn01 => ChallengeType::TlsAlpn01,
    };

    let mut authorizations = order.authorizations();
    while let Some(result) = authorizations.next().await {
        let mut authz = result.map_err(|error| format!("读取授权失败: {error}"))?;
        match authz.status {
            AuthorizationStatus::Pending => {}
            AuthorizationStatus::Valid => continue,
            other => return Err(format!("ACME 授权状态异常: {other:?}")),
        }

        let Some(mut challenge) = authz.challenge(challenge_type()) else {
            return Err(format!("授权缺少 {} 挑战", settings.challenge.as_str()));
        };
        let key_authorization = challenge.key_authorization();

        match settings.challenge {
            TlsChallenge::Http01 => {
                let token = challenge.token.clone();
                challenges.insert(&token, key_authorization.as_str());
            }
            TlsChallenge::TlsAlpn01 => {
                let domain = challenge.identifier().to_string();
                let digest = key_authorization.digest();
                let certified = challenge_cert(&domain, digest.as_ref())
                    .map_err(|error| format!("生成 ALPN 挑战证书失败: {error}"))?;
                resolver.set_challenge_cert(certified);
            }
        }

        challenge
            .set_ready()
            .await
            .map_err(|error| format!("提交挑战失败: {error}"))?;
    }
    Ok(())
}

/// TLS-ALPN-01 的临时自签名挑战证书（含 `id-pe-acmeIdentifier` 关键扩展）。
fn challenge_cert(domain: &str, digest: &[u8]) -> Result<CertifiedKey, String> {
    let key = rcgen::KeyPair::generate().map_err(|error| error.to_string())?;
    let mut params = rcgen::CertificateParams::new(vec![domain.to_string()])
        .map_err(|error| error.to_string())?;
    params
        .custom_extensions
        .push(rcgen::CustomExtension::new_acme_identifier(digest));
    let cert = params
        .self_signed(&key)
        .map_err(|error| format!("自签名失败: {error}"))?;
    parse_certified_key(&cert.pem(), &key.serialize_pem()).map_err(|error| error.to_string())
}

/// 证书链第一张的有效期终点（Unix 秒）。
fn not_after(chain_pem: &str) -> Option<i64> {
    let mut reader = std::io::BufReader::new(chain_pem.as_bytes());
    let der = rustls_pemfile::certs(&mut reader).next()?.ok()?;
    let (_, cert) = X509Certificate::from_der(der.as_ref()).ok()?;
    Some(cert.validity().not_after.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_cert_carries_acme_identifier() {
        let digest = [7u8; 32];
        let certified = challenge_cert("gw.example.com", &digest).expect("challenge cert");
        assert_eq!(certified.cert.len(), 1);
        // 扩展存在（id-pe-acmeIdentifier = 1.3.6.1.5.5.7.1.31）。
        let (_, cert) = X509Certificate::from_der(certified.cert[0].as_ref()).expect("parse");
        let found = cert
            .extensions()
            .iter()
            .any(|ext| ext.oid.to_id_string() == "1.3.6.1.5.5.7.1.31");
        assert!(found, "缺少 acmeIdentifier 扩展");
    }

    #[test]
    fn not_after_reads_validity() {
        let key = rcgen::KeyPair::generate().unwrap();
        let params = rcgen::CertificateParams::new(vec!["gw.example.com".to_string()]).unwrap();
        let cert = params.self_signed(&key).unwrap();
        let ts = not_after(&cert.pem()).expect("not_after");
        assert!(ts > 0);
    }
}
