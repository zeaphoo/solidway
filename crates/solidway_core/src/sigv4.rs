//! AWS Signature Version 4（header 形式）。
//!
//! 只覆盖"用长期/临时凭据给普通请求签名"这一种用法：不做 presigned URL、
//! 不做流式分块签名（`x-amz-content-sha256: STREAMING-*`）。

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// AWS 凭据（来自凭据 JSON）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

impl AwsCredentials {
    /// 从凭据 JSON 解析；缺少必需字段时报可读错误。
    pub fn parse(raw: &str) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|_| "AWS 凭据不是合法 JSON".to_string())?;
        let field = |name: &str| {
            value
                .get(name)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };
        Ok(Self {
            access_key_id: field("access_key_id").ok_or("AWS 凭据缺少 access_key_id")?,
            secret_access_key: field("secret_access_key").ok_or("AWS 凭据缺少 secret_access_key")?,
            session_token: field("session_token"),
        })
    }
}

/// 签名结果：需要写入请求的 header 值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigV4Headers {
    pub host: String,
    pub amz_date: String,
    pub authorization: String,
    pub session_token: Option<String>,
}

/// 对一次请求签名。
///
/// `content_type` 参与签名（存在时），因此调用方必须原样发送同一个值。
#[allow(clippy::too_many_arguments)]
pub fn sign(
    credentials: &AwsCredentials,
    region: &str,
    service: &str,
    method: &str,
    url: &str,
    content_type: Option<&str>,
    body: &[u8],
    now: SystemTime,
) -> Result<SigV4Headers, String> {
    if region.trim().is_empty() {
        return Err("缺少 region，无法签名（请在端点变量中填写 region）".to_string());
    }
    let (host, path, query) = split_url(url)?;
    let (amz_date, datestamp) = amz_timestamps(now)?;

    let mut headers: Vec<(String, String)> = Vec::new();
    if let Some(value) = content_type {
        headers.push(("content-type".to_string(), value.trim().to_string()));
    }
    headers.push(("host".to_string(), host.clone()));
    headers.push(("x-amz-date".to_string(), amz_date.clone()));
    if let Some(token) = &credentials.session_token {
        headers.push(("x-amz-security-token".to_string(), token.clone()));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let canonical_headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{}\n", value.trim()))
        .collect();
    let signed_headers = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let payload_hash = hex::encode(Sha256::digest(body));

    let canonical_request = format!(
        "{method}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        canonical_uri(&path),
        canonical_query(&query),
    );
    let scope = format!("{datestamp}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let mut key = hmac(
        format!("AWS4{}", credentials.secret_access_key).as_bytes(),
        datestamp.as_bytes(),
    )?;
    key = hmac(&key, region.as_bytes())?;
    key = hmac(&key, service.as_bytes())?;
    key = hmac(&key, b"aws4_request")?;
    let signature = hex::encode(hmac(&key, string_to_sign.as_bytes())?);

    Ok(SigV4Headers {
        host,
        amz_date,
        authorization: format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            credentials.access_key_id
        ),
        session_token: credentials.session_token.clone(),
    })
}

fn hmac(key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| "HMAC 初始化失败".to_string())?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

/// 拆出 `host`（含非默认端口）、path 与 query。
fn split_url(url: &str) -> Result<(String, String, String), String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| format!("目标地址缺少 scheme: {url}"))?;
    let (authority, tail) = match rest.find(['/', '?']) {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    let default_port = if scheme.eq_ignore_ascii_case("https") {
        "443"
    } else {
        "80"
    };
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|ch| ch.is_ascii_digit()) => {
            if port == default_port {
                host.to_string()
            } else {
                authority.to_string()
            }
        }
        _ => authority.to_string(),
    };
    let (path, query) = match tail.split_once('?') {
        Some((path, query)) => (path, query),
        None => (tail, ""),
    };
    Ok((
        host,
        if path.is_empty() { "/".to_string() } else { path.to_string() },
        query.to_string(),
    ))
}

/// `yyyymmddThhmmssZ` 与 `yyyymmdd`。
fn amz_timestamps(now: SystemTime) -> Result<(String, String), String> {
    let secs = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时间早于 Unix epoch，无法签名".to_string())?
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(secs);
    let datestamp = format!("{year:04}{month:02}{day:02}");
    Ok((
        format!("{datestamp}T{hour:02}{minute:02}{second:02}Z"),
        datestamp,
    ))
}

/// Unix 秒 → UTC 年月日时分秒（Howard Hinnant 的 civil_from_days）。
fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    if month <= 2 {
        year += 1;
    }
    (
        year,
        month,
        day,
        (rem / 3_600) as u32,
        ((rem % 3_600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// SigV4 的 URI 编码：保留路径允许字符，其余按字节百分号编码。
fn canonical_uri(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'/'
            | b':'
            | b'@'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'=' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// query 规范化：按参数名（同名再按值）排序，键值分别编码。
fn canonical_query(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }
    let mut pairs: Vec<(String, String)> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (encode_component(key), encode_component(value)),
            None => (encode_component(pair), String::new()),
        })
        .collect();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn example_credentials() -> AwsCredentials {
        AwsCredentials {
            access_key_id: "AKIDEXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
            session_token: None,
        }
    }

    /// AWS 官方文档的签名示例（GET + query + content-type）。
    #[test]
    fn matches_aws_published_vector() {
        let now = UNIX_EPOCH + Duration::from_secs(1_440_938_160); // 2015-08-30T12:36:00Z
        let signed = sign(
            &example_credentials(),
            "us-east-1",
            "iam",
            "GET",
            "https://iam.amazonaws.com/?Action=ListUsers&Version=2010-05-08",
            Some("application/x-www-form-urlencoded; charset=utf-8"),
            b"",
            now,
        )
        .expect("sign");
        assert_eq!(signed.amz_date, "20150830T123600Z");
        assert_eq!(signed.host, "iam.amazonaws.com");
        assert_eq!(
            signed.authorization,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
    }

    #[test]
    fn session_token_is_signed() {
        let mut credentials = example_credentials();
        credentials.session_token = Some("SESSION".to_string());
        let signed = sign(
            &credentials,
            "us-east-1",
            "bedrock",
            "POST",
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.claude-v2:1/invoke",
            Some("application/json"),
            br#"{"prompt":"hi"}"#,
            UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        )
        .expect("sign");
        assert!(signed.authorization.contains("x-amz-security-token"));
        assert_eq!(signed.session_token.as_deref(), Some("SESSION"));
    }

    #[test]
    fn parses_credentials_and_rejects_missing_fields() {
        let parsed = AwsCredentials::parse(
            r#"{"access_key_id":"AKIA","secret_access_key":"s","session_token":"t"}"#,
        )
        .expect("parse");
        assert_eq!(parsed.session_token.as_deref(), Some("t"));
        assert!(AwsCredentials::parse(r#"{"access_key_id":"AKIA"}"#).is_err());
        assert!(AwsCredentials::parse("not json").is_err());
    }

    #[test]
    fn splits_host_path_and_query() {
        assert_eq!(
            split_url("https://bedrock-runtime.us-east-1.amazonaws.com/model/x:invoke?b=2&a=1")
                .unwrap(),
            (
                "bedrock-runtime.us-east-1.amazonaws.com".to_string(),
                "/model/x:invoke".to_string(),
                "b=2&a=1".to_string()
            )
        );
        assert_eq!(canonical_query("b=2&a=1"), "a=1&b=2");
        assert_eq!(canonical_uri("/model/anthropic.claude-v2:1/invoke"), "/model/anthropic.claude-v2:1/invoke");
    }

    #[test]
    fn empty_region_is_rejected() {
        assert!(
            sign(
                &example_credentials(),
                "",
                "bedrock",
                "POST",
                "https://example.com/x",
                None,
                b"",
                UNIX_EPOCH
            )
            .is_err()
        );
    }
}
