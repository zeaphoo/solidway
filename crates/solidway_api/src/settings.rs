use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db::settings as store;
use crate::error::{ApiError, Result};

const USAGE_KEY: &str = "usage";
const GATEWAY_KEY: &str = "gateway";
const NETWORK_KEY: &str = "network";
const AUTH_KEY: &str = "auth";
const ACCESS_KEY: &str = "access";
const TLS_KEY: &str = "tls";
const ALERT_KEY: &str = "alert";

/// 逐条明细的保留范围。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetailMode {
    Off,
    Errors,
    All,
}

impl Default for DetailMode {
    fn default() -> Self {
        Self::Errors
    }
}

/// 用量统计设置（采集契约见 openspec 的用量统计设计）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageSettings {
    pub enabled: bool,
    pub detail_mode: DetailMode,
    pub detail_retention_hours: u32,
    pub detail_sample_rate: f64,
    /// 本地时区相对 UTC 的分钟偏移；用于 hour / day 桶对齐。
    /// **只读**：由服务端决定，用于对齐 hour / day 桶。见 `default_timezone_offset`。
    pub timezone_offset_minutes: i64,
}

/// 告警设置（Webhook）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertSettings {
    /// 为空表示不告警；仅接受 http/https 绝对 URL。
    pub webhook_url: Option<String>,
    /// 故障恢复时是否也推送。
    pub notify_recovery: bool,
    /// 同一 provider + 原因的最小告警间隔（分钟）。
    pub min_interval_minutes: u64,
}

impl Default for AlertSettings {
    fn default() -> Self {
        Self {
            webhook_url: None,
            notify_recovery: true,
            min_interval_minutes: 30,
        }
    }
}

impl AlertSettings {
    fn validate(&self) -> Result<()> {
        if let Some(url) = self
            .webhook_url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
        {
            let ok = (url.starts_with("http://") || url.starts_with("https://"))
                && !url.contains([' ', '\n'])
                && url
                    .split_once("://")
                    .map(|(_, rest)| !rest.is_empty())
                    .unwrap_or(false);
            if !ok {
                return Err(ApiError::InvalidRequest(
                    "webhook_url 必须是 http / https 绝对 URL".to_string(),
                ));
            }
        }
        if self.min_interval_minutes > 1440 {
            return Err(ApiError::InvalidRequest(
                "min_interval_minutes 超出范围 (0-1440)".to_string(),
            ));
        }
        Ok(())
    }
}

pub async fn load_alert(pool: &SqlitePool) -> Result<AlertSettings> {
    load(pool, ALERT_KEY).await
}

pub async fn save_alert(
    pool: &SqlitePool,
    settings: &AlertSettings,
    updated_by: &str,
    now: i64,
) -> Result<()> {
    settings.validate()?;
    save(pool, ALERT_KEY, settings, updated_by, now).await
}

/// 统计时区的初始值：取操作系统当前本地 UTC 偏移，失败时回退 UTC。
///
/// 时区只读（见 `openspec/changes/2026-10-20-usage-detail-and-timezone`）：
/// 桶在采集时按该偏移对齐，改动会让历史桶与新桶边界错位。
/// 不读取应用专属环境变量；系统时区由操作系统配置提供。
pub fn default_timezone_offset() -> i64 {
    time::UtcOffset::current_local_offset()
        .ok()
        .map(|offset| i64::from(offset.whole_minutes()))
        .filter(|value| (-720..=840).contains(value))
        .unwrap_or(0)
}

impl Default for UsageSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            detail_mode: DetailMode::Errors,
            detail_retention_hours: 24,
            detail_sample_rate: 1.0,
            timezone_offset_minutes: default_timezone_offset(),
        }
    }
}

impl UsageSettings {
    fn validate(&self) -> Result<()> {
        if self.detail_retention_hours > 8760 {
            return Err(ApiError::InvalidRequest(
                "detail_retention_hours 超出范围 (0-8760)".to_string(),
            ));
        }
        if self.detail_sample_rate.is_nan() || !(0.0..=1.0).contains(&self.detail_sample_rate) {
            return Err(ApiError::InvalidRequest(
                "detail_sample_rate 必须在 0-1".to_string(),
            ));
        }
        if !(-720..=840).contains(&self.timezone_offset_minutes) {
            return Err(ApiError::InvalidRequest(
                "timezone_offset_minutes 必须在 -720 到 840 之间".to_string(),
            ));
        }
        Ok(())
    }
}

/// 转发链路的运行期参数（仅 admin 可改；即时下发给 core）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewaySettings {
    /// 401/403 后的冷却秒数。
    pub cooldown_auth_secs: u64,
    /// 429（无 Retry-After）后的冷却秒数。
    pub cooldown_rate_limit_secs: u64,
    /// 5xx / 网络 / 超时后的冷却秒数。
    pub cooldown_unavailable_secs: u64,
    /// 配额耗尽类失败（`insufficient_quota`）的长冷却秒数（默认 6 小时）。
    pub quota_cooldown_secs: u64,
    /// 会话粘性滑动 TTL 秒数。
    pub affinity_ttl_secs: u64,
}

impl Default for GatewaySettings {
    fn default() -> Self {
        Self {
            cooldown_auth_secs: 30 * 60,
            cooldown_rate_limit_secs: 5 * 60,
            cooldown_unavailable_secs: 60,
            quota_cooldown_secs: 6 * 3600,
            affinity_ttl_secs: 24 * 3600,
        }
    }
}

impl GatewaySettings {
    fn validate(&self) -> Result<()> {
        const MAX: u64 = 30 * 24 * 3600;
        for (name, value) in [
            ("cooldown_auth_secs", self.cooldown_auth_secs),
            ("cooldown_rate_limit_secs", self.cooldown_rate_limit_secs),
            ("cooldown_unavailable_secs", self.cooldown_unavailable_secs),
            ("quota_cooldown_secs", self.quota_cooldown_secs),
            ("affinity_ttl_secs", self.affinity_ttl_secs),
        ] {
            if value > MAX {
                return Err(ApiError::InvalidRequest(format!(
                    "{name} 超出范围 (0-{MAX})"
                )));
            }
        }
        Ok(())
    }
}

/// 监听地址上限：防止把整张网卡表塞进来。
pub const MAX_BIND_ADDRS: usize = 16;

/// 网络与上游超时。`bind_addrs` / `port` 需重启生效；两个超时即时生效。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkSettings {
    /// 监听地址列表（同一端口）。
    pub bind_addrs: Vec<String>,
    /// 只读兼容字段：等于 `bind_addrs` 的首个元素（旧配置读取来源）。
    pub bind_addr: String,
    pub port: u16,
    pub connect_timeout_secs: u64,
    pub first_byte_timeout_secs: u64,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            bind_addrs: vec!["127.0.0.1".to_string()],
            bind_addr: "127.0.0.1".to_string(),
            port: 6565,
            connect_timeout_secs: 10,
            first_byte_timeout_secs: 60,
        }
    }
}

impl NetworkSettings {
    /// 归一化：去空白、去重、保序；列表为空时回退到旧字段 / 默认值；
    /// 最后把只读字段同步为首个地址。
    pub fn normalize(&mut self) {
        let mut addrs: Vec<String> = Vec::new();
        for raw in self.bind_addrs.drain(..) {
            let trimmed = raw.trim().to_string();
            if trimmed.is_empty() || addrs.contains(&trimmed) {
                continue;
            }
            addrs.push(trimmed);
        }
        if addrs.is_empty() {
            let legacy = self.bind_addr.trim().to_string();
            addrs.push(if legacy.is_empty() {
                "127.0.0.1".to_string()
            } else {
                legacy
            });
        }
        self.bind_addrs = addrs;
        self.bind_addr = self.bind_addrs.first().cloned().unwrap_or_default();
    }

    /// 解析成 IP 列表（已归一化后调用）。
    pub fn bind_ips(&self) -> Vec<std::net::IpAddr> {
        self.bind_addrs
            .iter()
            .filter_map(|raw| raw.parse().ok())
            .collect()
    }

    fn validate(&self) -> Result<()> {
        let mut normalized = self.clone();
        normalized.normalize();
        if normalized.bind_addrs.is_empty() {
            return Err(ApiError::InvalidRequest("至少需要一个绑定地址".to_string()));
        }
        if normalized.bind_addrs.len() > MAX_BIND_ADDRS {
            return Err(ApiError::InvalidRequest(format!(
                "绑定地址最多 {MAX_BIND_ADDRS} 个"
            )));
        }
        let mut ips: Vec<std::net::IpAddr> = Vec::new();
        for raw in &normalized.bind_addrs {
            let ip = raw
                .parse::<std::net::IpAddr>()
                .map_err(|_| ApiError::InvalidRequest(format!("绑定地址必须是 IP 地址: {raw}")))?;
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        }
        // 通配符已覆盖同族全部地址：与任何其它地址混用都会 EADDRINUSE。
        if ips.len() > 1 && ips.iter().any(|ip| ip.is_unspecified()) {
            return Err(ApiError::InvalidRequest(
                "通配地址（0.0.0.0 / ::）不能与其他地址同时使用".to_string(),
            ));
        }
        if self.port == 0 {
            return Err(ApiError::InvalidRequest(
                "port 必须在 1-65535（不支持 0）".to_string(),
            ));
        }
        for (name, value) in [
            ("connect_timeout_secs", self.connect_timeout_secs),
            ("first_byte_timeout_secs", self.first_byte_timeout_secs),
        ] {
            if !(1..=3600).contains(&value) {
                return Err(ApiError::InvalidRequest(format!("{name} 必须在 1-3600")));
            }
        }
        Ok(())
    }
}

/// 会话与 Cookie 设置。
///
/// `cookie_secure` **刻意不暴露在管理界面**：一旦设成 `true` 而部署是纯 HTTP，
/// 浏览器不再回传会话 cookie，表现为"登录成功却仍未登录"，而修复入口正是登录后的设置页。
/// 若要把它放进界面，必须先设计不依赖登录的救援通道。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthSettings {
    pub session_ttl_hours: u64,
    pub cookie_secure: bool,
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            session_ttl_hours: 24,
            cookie_secure: false,
        }
    }
}

impl AuthSettings {
    fn validate(&self) -> Result<()> {
        if !(1..=720).contains(&self.session_ttl_hours) {
            return Err(ApiError::InvalidRequest(
                "session_ttl_hours 必须在 1-720".to_string(),
            ));
        }
        Ok(())
    }
}

/// 对外访问地址：客户端访问网关使用的 origin / 路径前缀列表。
///
/// 只填域名或前缀即可（如 `https://gw.example.com`、`https://host/solidway`）；
/// 控制台展示时会自动拼接 `/v1`。第一项为默认，列表为空时回退到当前 origin。
/// HTTPS 由 URL 的 scheme 表示。TLS 可由内置监听（`tls.enabled`）或前置反向代理终止，
/// 二者择一；`tls` 节存在时其证书域名直接取自这里的 host。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessSettings {
    pub public_origins: Vec<String>,
}

impl Default for AccessSettings {
    fn default() -> Self {
        Self {
            public_origins: Vec::new(),
        }
    }
}

impl AccessSettings {
    fn validate(&self) -> Result<()> {
        if self.public_origins.len() > 10 {
            return Err(ApiError::InvalidRequest(
                "对外访问地址最多 10 个".to_string(),
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for raw in &self.public_origins {
            let url = raw.trim().trim_end_matches('/');
            if url.is_empty() {
                return Err(ApiError::InvalidRequest("对外访问地址不能为空".to_string()));
            }
            let Some((scheme, rest)) = url.split_once("://") else {
                return Err(ApiError::InvalidRequest(format!(
                    "对外访问地址必须是绝对 URL: {url}"
                )));
            };
            if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
                return Err(ApiError::InvalidRequest(format!(
                    "对外访问地址仅支持 http / https: {url}"
                )));
            }
            if url.contains(['?', '#']) {
                return Err(ApiError::InvalidRequest(format!(
                    "对外访问地址不能包含 query 或 fragment: {url}"
                )));
            }
            let host = rest.split('/').next().unwrap_or_default().trim();
            if host.is_empty() {
                return Err(ApiError::InvalidRequest(format!(
                    "对外访问地址缺少 host: {url}"
                )));
            }
            if !seen.insert(url.to_string()) {
                return Err(ApiError::InvalidRequest(format!(
                    "对外访问地址不能重复: {url}"
                )));
            }
        }
        Ok(())
    }
}

/// ACME 挑战类型。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TlsChallenge {
    #[serde(rename = "http-01")]
    Http01,
    #[serde(rename = "tls-alpn-01")]
    TlsAlpn01,
}

impl TlsChallenge {
    pub fn as_str(self) -> &'static str {
        match self {
            TlsChallenge::Http01 => "http-01",
            TlsChallenge::TlsAlpn01 => "tls-alpn-01",
        }
    }

    /// ACME 校验使用的公网端口（固定，与本地监听端口无关）。
    pub fn public_port(self) -> u16 {
        match self {
            TlsChallenge::Http01 => 80,
            TlsChallenge::TlsAlpn01 => 443,
        }
    }
}

impl Default for TlsChallenge {
    fn default() -> Self {
        TlsChallenge::Http01
    }
}

/// ACME directory。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcmeDirectory {
    Production,
    Staging,
}

impl Default for AcmeDirectory {
    fn default() -> Self {
        AcmeDirectory::Production
    }
}

impl AcmeDirectory {
    pub fn url(self) -> &'static str {
        match self {
            AcmeDirectory::Production => "https://acme-v02.api.letsencrypt.org/directory",
            AcmeDirectory::Staging => "https://acme-staging-v02.api.letsencrypt.org/directory",
        }
    }
}

/// 内置 TLS / ACME 设置。
///
/// 端口是**本地监听端口**；ACME 校验固定访问公网 80 / 443，由部署侧端口映射决定。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TlsSettings {
    pub enabled: bool,
    pub email: String,
    pub challenge: TlsChallenge,
    pub directory: AcmeDirectory,
    pub http_port: u16,
    pub https_port: u16,
    pub hsts: bool,
}

impl Default for TlsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            email: String::new(),
            challenge: TlsChallenge::Http01,
            directory: AcmeDirectory::Production,
            http_port: 80,
            https_port: 443,
            hsts: false,
        }
    }
}

impl TlsSettings {
    /// 启用时的校验；`domains` 为从 `access.public_origins` 推导出的合法 DNS 域名。
    pub fn validate(&self, domains: &[String]) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if domains.is_empty() {
            return Err(ApiError::InvalidRequest(
                "启用 HTTPS 需要至少一个合法 DNS 域名（来自对外访问地址）".to_string(),
            ));
        }
        if self.email.trim().is_empty() {
            return Err(ApiError::InvalidRequest(
                "启用 HTTPS 需要 ACME 邮箱".to_string(),
            ));
        }
        if !self.email.contains('@') {
            return Err(ApiError::InvalidRequest(format!(
                "ACME 邮箱格式非法: {}",
                self.email
            )));
        }
        if self.http_port == 0 || self.https_port == 0 {
            return Err(ApiError::InvalidRequest(
                "http_port / https_port 必须在 1-65535".to_string(),
            ));
        }
        if self.http_port == self.https_port {
            return Err(ApiError::InvalidRequest(
                "http_port 与 https_port 不能相同".to_string(),
            ));
        }
        Ok(())
    }

    /// 需要端口映射时的提示（challenge 所需公网端口与本机端口不一致）。
    pub fn port_hint(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let local = match self.challenge {
            TlsChallenge::Http01 => self.http_port,
            TlsChallenge::TlsAlpn01 => self.https_port,
        };
        let public = self.challenge.public_port();
        (local != public).then(|| {
            format!(
                "{} 校验固定访问公网 {public}，本机监听 {local}：需要在前面做 {public}→{local} 的端口映射",
                self.challenge.as_str()
            )
        })
    }
}

/// 全部设置（数据库 `settings` 表的六个键）。
#[derive(Debug, Clone, Serialize)]
pub struct AllSettings {
    pub usage: UsageSettings,
    pub gateway: GatewaySettings,
    pub network: NetworkSettings,
    pub auth: AuthSettings,
    pub access: AccessSettings,
    pub tls: TlsSettings,
    pub alert: AlertSettings,
}

async fn load<T>(pool: &SqlitePool, key: &str) -> Result<T>
where
    T: Default + for<'de> Deserialize<'de>,
{
    match store::get(pool, key).await? {
        Some(raw) => match serde_json::from_str(&raw) {
            Ok(value) => Ok(value),
            Err(error) => {
                // R2-A-6③：坏 JSON 回退该 key 的默认值，不再 500；按 key 记日志便于定位。
                tracing::warn!(key = %key, %error, "invalid settings JSON; falling back to default");
                Ok(T::default())
            }
        },
        None => Ok(T::default()),
    }
}

async fn save<T>(pool: &SqlitePool, key: &str, value: &T, updated_by: &str, now: i64) -> Result<()>
where
    T: Serialize,
{
    let raw = serde_json::to_string(value).map_err(|_| ApiError::internal("设置序列化失败"))?;
    store::upsert(pool, key, &raw, updated_by, now).await
}

pub async fn load_gateway(pool: &SqlitePool) -> Result<GatewaySettings> {
    load(pool, GATEWAY_KEY).await
}

pub async fn save_gateway(
    pool: &SqlitePool,
    settings: &GatewaySettings,
    updated_by: &str,
    now: i64,
) -> Result<()> {
    settings.validate()?;
    save(pool, GATEWAY_KEY, settings, updated_by, now).await
}

pub async fn load_usage(pool: &SqlitePool) -> Result<UsageSettings> {
    load(pool, USAGE_KEY).await
}

pub async fn save_usage(
    pool: &SqlitePool,
    settings: &UsageSettings,
    updated_by: &str,
    now: i64,
) -> Result<()> {
    settings.validate()?;
    save(pool, USAGE_KEY, settings, updated_by, now).await
}

/// 首次启动时把系统推导出的统计时区固化到 DB；之后 DB 是唯一事实源。
pub async fn ensure_usage(pool: &SqlitePool, updated_by: &str, now: i64) -> Result<()> {
    if store::get(pool, USAGE_KEY).await?.is_none() {
        save_usage(pool, &UsageSettings::default(), updated_by, now).await?;
    }
    Ok(())
}

pub async fn load_network(pool: &SqlitePool) -> Result<NetworkSettings> {
    let mut network: NetworkSettings = load(pool, NETWORK_KEY).await?;
    // 旧配置只有 `bind_addr`：折成单元素列表后再对外暴露。
    network.normalize();
    Ok(network)
}

pub async fn save_network(
    pool: &SqlitePool,
    settings: &NetworkSettings,
    updated_by: &str,
    now: i64,
) -> Result<()> {
    let mut normalized = settings.clone();
    normalized.normalize();
    normalized.validate()?;
    save(pool, NETWORK_KEY, &normalized, updated_by, now).await
}

pub async fn load_auth(pool: &SqlitePool) -> Result<AuthSettings> {
    load(pool, AUTH_KEY).await
}

pub async fn save_auth(
    pool: &SqlitePool,
    settings: &AuthSettings,
    updated_by: &str,
    now: i64,
) -> Result<()> {
    settings.validate()?;
    save(pool, AUTH_KEY, settings, updated_by, now).await
}

pub async fn load_access(pool: &SqlitePool) -> Result<AccessSettings> {
    load(pool, ACCESS_KEY).await
}

pub async fn save_access(
    pool: &SqlitePool,
    settings: &AccessSettings,
    updated_by: &str,
    now: i64,
) -> Result<()> {
    settings.validate()?;
    save(pool, ACCESS_KEY, settings, updated_by, now).await
}

pub async fn load_tls(pool: &SqlitePool) -> Result<TlsSettings> {
    load(pool, TLS_KEY).await
}

pub async fn save_tls(
    pool: &SqlitePool,
    settings: &TlsSettings,
    updated_by: &str,
    now: i64,
) -> Result<()> {
    let access = load_access(pool).await?;
    settings.validate(&crate::tls::domain::derive_domains(&access.public_origins).domains)?;
    save(pool, TLS_KEY, settings, updated_by, now).await
}

pub async fn load_all(pool: &SqlitePool) -> Result<AllSettings> {
    Ok(AllSettings {
        usage: load_usage(pool).await?,
        gateway: load_gateway(pool).await?,
        network: load_network(pool).await?,
        auth: load_auth(pool).await?,
        access: load_access(pool).await?,
        tls: load_tls(pool).await?,
        alert: load_alert(pool).await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled() -> TlsSettings {
        TlsSettings {
            enabled: true,
            email: "ops@example.com".to_string(),
            ..TlsSettings::default()
        }
    }

    #[test]
    fn tls_defaults() {
        let tls = TlsSettings::default();
        assert!(!tls.enabled);
        assert_eq!(tls.challenge, TlsChallenge::Http01);
        assert_eq!(tls.directory, AcmeDirectory::Production);
        assert_eq!((tls.http_port, tls.https_port), (80, 443));
        assert!(!tls.hsts);
    }

    #[test]
    fn rejects_enable_without_dns_domain() {
        let err = enabled().validate(&[]).unwrap_err();
        assert!(matches!(err, ApiError::InvalidRequest(_)));
    }

    #[test]
    fn rejects_port_conflict_and_zero() {
        let domains = vec!["gw.example.com".to_string()];
        let mut tls = enabled();
        tls.http_port = 8443;
        tls.https_port = 8443;
        assert!(tls.validate(&domains).is_err());

        let mut tls = enabled();
        tls.http_port = 0;
        assert!(tls.validate(&domains).is_err());
    }

    #[test]
    fn rejects_missing_or_invalid_email() {
        let domains = vec!["gw.example.com".to_string()];
        let mut tls = enabled();
        tls.email = String::new();
        assert!(tls.validate(&domains).is_err());

        tls.email = "not-an-email".to_string();
        assert!(tls.validate(&domains).is_err());
    }

    #[test]
    fn disabled_settings_need_no_domains() {
        assert!(TlsSettings::default().validate(&[]).is_ok());
    }

    #[test]
    fn port_hint_only_when_ports_differ() {
        let domains = vec!["gw.example.com".to_string()];
        let tls = enabled();
        assert!(tls.port_hint().is_none());

        let mut custom = enabled();
        custom.http_port = 8080;
        assert!(custom.validate(&domains).is_ok());
        assert!(custom.port_hint().unwrap().contains("80→8080"));

        let mut alpn = enabled();
        alpn.challenge = TlsChallenge::TlsAlpn01;
        alpn.https_port = 8443;
        assert!(alpn.port_hint().unwrap().contains("443→8443"));
    }
}
