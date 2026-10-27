//! 证书域名的推导：只从 `access.public_origins` 的 host 取，不额外配置域名。

use serde::Serialize;

/// 未参与证书的 origin 及原因（界面提示用）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkippedOrigin {
    pub origin: String,
    pub reason: &'static str,
}

/// 推导结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DerivedDomains {
    /// 参与证书的 DNS 域名（去重、排序）。
    pub domains: Vec<String>,
    pub skipped: Vec<SkippedOrigin>,
}

/// 从对外访问地址推导证书域名。
///
/// 只保留合法 DNS 名：至少含一个点、非 IP、非 `localhost`、无端口 / 路径 / 通配符。
pub fn derive_domains(origins: &[String]) -> DerivedDomains {
    let mut domains: Vec<String> = Vec::new();
    let mut skipped: Vec<SkippedOrigin> = Vec::new();

    for raw in origins {
        let origin = raw.trim();
        let Some((_scheme, rest)) = origin.split_once("://") else {
            skipped.push(SkippedOrigin {
                origin: origin.to_string(),
                reason: "invalid_url",
            });
            continue;
        };
        let authority = rest.split('/').next().unwrap_or_default().trim();

        // 先剥端口（`host:port`；IPv6 的 `[::1]:port` 保持整体判断）。
        let (host, has_port) = if authority.starts_with('[') {
            (authority, false)
        } else {
            match authority.split_once(':') {
                Some((host, _port)) => (host, true),
                None => (authority, false),
            }
        };

        let reason = if host.is_empty() {
            Some("missing_host")
        } else if host.contains('*') {
            Some("wildcard")
        } else if host.starts_with('[') {
            Some("ip")
        } else if host.parse::<std::net::IpAddr>().is_ok() {
            Some("ip")
        } else if host.eq_ignore_ascii_case("localhost") || !host.contains('.') {
            Some("not_dns")
        } else if !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            Some("invalid_host")
        } else if has_port {
            Some("port")
        } else {
            None
        };

        match reason {
            Some(reason) => skipped.push(SkippedOrigin {
                origin: origin.to_string(),
                reason,
            }),
            None => {
                let host = host.to_ascii_lowercase();
                if !domains.contains(&host) {
                    domains.push(host);
                }
            }
        }
    }

    domains.sort();
    DerivedDomains { domains, skipped }
}

/// 域名集合的稳定摘要（缓存文件命名用，不用域名直接拼文件名）。
pub fn domains_digest(domains: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut sorted = domains.to_vec();
    sorted.sort();
    let mut hasher = Sha256::new();
    for domain in &sorted {
        hasher.update(domain.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(&hasher.finalize()[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origins(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn keeps_only_dns_names() {
        let derived = derive_domains(&origins(&[
            "https://gw.example.com",
            "http://192.168.1.10:6565",
            "http://localhost:6565",
            "https://*.example.com",
            "https://gw.example.com/prefix",
            "https://another.example.com:8443",
            "https://noport",
        ]));
        assert_eq!(derived.domains, vec!["gw.example.com".to_string()]);

        let reasons: Vec<&str> = derived.skipped.iter().map(|s| s.reason).collect();
        assert_eq!(
            reasons,
            vec!["ip", "not_dns", "wildcard", "port", "not_dns"]
        );
    }

    #[test]
    fn deduplicates_case_insensitively_and_sorts() {
        let derived = derive_domains(&origins(&[
            "https://b.example.com",
            "https://A.example.com",
            "https://a.example.com",
            "https://b.example.com",
        ]));
        assert_eq!(
            derived.domains,
            vec!["a.example.com".to_string(), "b.example.com".to_string()]
        );
    }

    #[test]
    fn digest_is_order_independent() {
        let a = vec!["a.example.com".to_string(), "b.example.com".to_string()];
        let mut b = a.clone();
        b.reverse();
        assert_eq!(domains_digest(&a), domains_digest(&b));
        assert_ne!(
            domains_digest(&a),
            domains_digest(&["a.example.com".to_string()])
        );
    }
}
