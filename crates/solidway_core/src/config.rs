//! core 的内存配置模型：由控制面构造并全量下发（原子替换）。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::health::{Cooldowns, HealthState};

/// 请求体整读上限（转发端点仅为路由与 model 校验需要）。
pub const DEFAULT_REQUEST_BODY_LIMIT: usize = 32 * 1024 * 1024;

fn default_auth_scheme() -> String {
    "bearer".to_string()
}

fn default_credential_kind() -> String {
    "api_key".to_string()
}

fn default_priority() -> i64 {
    1
}

/// 入站 API 格式。网关不做格式转换：请求格式必须与 Provider 的声明精确匹配。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    /// `POST /v1/chat/completions`。
    ChatCompletions,
    /// `POST /v1/responses`。
    Responses,
    /// Anthropic Messages：`POST /v1/messages` 与 `/v1/messages/count_tokens`。
    AnthropicMessages,
}

impl ApiFormat {
    pub const ALL: [ApiFormat; 3] = [
        ApiFormat::ChatCompletions,
        ApiFormat::Responses,
        ApiFormat::AnthropicMessages,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ApiFormat::ChatCompletions => "chat_completions",
            ApiFormat::Responses => "responses",
            ApiFormat::AnthropicMessages => "anthropic_messages",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.as_str() == raw)
    }
}

/// 客户端模型名 → 上游模型 id 的映射。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMapping {
    /// 客户端可用模型名（路由、`/v1/models`、粘性键都用它）。
    pub name: String,
    /// 上游模型 id；为空时等同于 `name`（不改写请求体）。
    #[serde(default)]
    pub upstream: String,
}

impl ModelMapping {
    pub fn upstream_or_name(&self) -> &str {
        if self.upstream.is_empty() {
            &self.name
        } else {
            &self.upstream
        }
    }

    /// 是否需要改写请求 / 响应中的 `model` 字段。
    pub fn rewrites(&self) -> bool {
        self.upstream_or_name() != self.name
    }
}

/// 一份完整配置：控制面每次写操作后重新构造并下发。
#[derive(Debug, Clone, Default)]
pub struct CoreConfig {
    pub api_keys: Vec<ApiKeyEntry>,
    pub routes: Vec<RouteEntry>,
    pub providers: Vec<ProviderEntry>,
    pub health: Vec<ProviderHealthSeed>,
    /// 选号指标种子：**只在 `Core::new` 时采用**（运行期 `apply_config` 忽略）。
    pub metrics: Vec<ProviderMetricSeed>,
    /// 会话粘性种子：**只在 `Core::new` 时采用**（运行期 `apply_config` 忽略）。
    pub affinity: Vec<AffinitySeed>,
    pub options: CoreOptions,
}

/// 启动时恢复的选号指标（由控制面从历史用量聚合得出）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderMetricSeed {
    pub provider_id: String,
    pub ttfb_ms: f64,
    /// `0..=1`。
    pub failure_rate: f64,
    pub samples: u64,
}

/// 启动时恢复的会话粘性条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AffinitySeed {
    pub route_id: String,
    pub model: String,
    pub session: String,
    pub provider_id: String,
    /// Unix 秒；早于当前时刻的条目会被忽略。
    pub expires_at: i64,
}

/// API Key（接入凭据）：仅存哈希；`key_hash` 由控制面计算后下发。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    pub id: String,
    pub route_id: String,
    pub key_hash: String,
    pub enabled: bool,
    /// Unix 秒；`None` = 永不过期。
    #[serde(default)]
    pub expires_at: Option<i64>,
}

/// 路由选号策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrategy {
    /// 轮询（默认）：行为与引入策略之前一致。
    #[default]
    RoundRobin,
    /// 可用性优先。
    Uptime,
    /// 首字节优先。
    Latency,
    /// 综合加权（含按比例随机探索）。
    Auto,
}

impl SelectionStrategy {
    pub const ALL: [SelectionStrategy; 4] = [
        SelectionStrategy::RoundRobin,
        SelectionStrategy::Uptime,
        SelectionStrategy::Latency,
        SelectionStrategy::Auto,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SelectionStrategy::RoundRobin => "round_robin",
            SelectionStrategy::Uptime => "uptime",
            SelectionStrategy::Latency => "latency",
            SelectionStrategy::Auto => "auto",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == raw)
    }

    /// 该策略的默认权重（`price` / `throughput` 本轮恒为 0）。
    pub fn default_weights(self) -> SelectionWeights {
        match self {
            SelectionStrategy::RoundRobin | SelectionStrategy::Uptime => SelectionWeights {
                uptime: 0.9,
                latency: 0.1,
                price: 0.0,
                throughput: 0.0,
            },
            SelectionStrategy::Latency => SelectionWeights {
                uptime: 0.1,
                latency: 0.9,
                price: 0.0,
                throughput: 0.0,
            },
            SelectionStrategy::Auto => SelectionWeights {
                uptime: 0.75,
                latency: 0.25,
                price: 0.0,
                throughput: 0.0,
            },
        }
    }

    /// 该策略的默认探索率：只有 `auto` 默认开启。
    pub fn default_exploration_rate(self) -> f64 {
        match self {
            SelectionStrategy::Auto => 0.01,
            _ => 0.0,
        }
    }
}

/// 打分权重（归一化后参与计算）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectionWeights {
    pub uptime: f64,
    pub latency: f64,
    /// 本轮保留字段，MUST 为 0（无价格指标）。
    pub price: f64,
    /// 本轮保留字段，MUST 为 0（无吞吐指标）。
    pub throughput: f64,
}

impl Default for SelectionWeights {
    fn default() -> Self {
        SelectionStrategy::Auto.default_weights()
    }
}

impl SelectionWeights {
    pub fn sum(&self) -> f64 {
        self.uptime + self.latency + self.price + self.throughput
    }
}

/// 打分阈值与兜底值。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectionThresholds {
    /// 低于该 uptime（%）开始指数惩罚。
    pub uptime_penalty: f64,
    /// 无指标时的兜底 uptime（%）。
    pub default_uptime: f64,
    /// 无指标时的兜底首字节时延（毫秒）。
    pub default_latency_ms: f64,
    /// `auto` 策略的随机探索比例（0..=1）。
    pub exploration_rate: f64,
}

impl Default for SelectionThresholds {
    fn default() -> Self {
        Self {
            uptime_penalty: 95.0,
            default_uptime: 100.0,
            default_latency_ms: 1000.0,
            exploration_rate: 0.0,
        }
    }
}

/// 一条路由的选号策略（控制面解析后的完整配置）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RouteSelection {
    pub strategy: SelectionStrategy,
    pub weights: SelectionWeights,
    pub thresholds: SelectionThresholds,
}

impl Default for RouteSelection {
    fn default() -> Self {
        RouteSelection::for_strategy(SelectionStrategy::RoundRobin)
    }
}

impl RouteSelection {
    /// 某策略的完整默认配置。
    pub fn for_strategy(strategy: SelectionStrategy) -> Self {
        Self {
            strategy,
            weights: strategy.default_weights(),
            thresholds: SelectionThresholds {
                exploration_rate: strategy.default_exploration_rate(),
                ..SelectionThresholds::default()
            },
        }
    }

    /// 实际生效的策略：权重和 ≤ 0 时回退轮询。
    pub fn effective_strategy(&self) -> SelectionStrategy {
        if self.strategy != SelectionStrategy::RoundRobin && self.weights.sum() <= 0.0 {
            SelectionStrategy::RoundRobin
        } else {
            self.strategy
        }
    }

    /// 是否因权重全为 0 而回退。
    pub fn falls_back(&self) -> bool {
        self.strategy != SelectionStrategy::RoundRobin && self.weights.sum() <= 0.0
    }
}

/// 路由：对消费方暴露、按名寻址的 LLM 服务单位。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEntry {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// 选号策略（缺省轮询）。
    #[serde(default)]
    pub selection: RouteSelection,
}

/// provider 凭据：一条可转发的上游凭据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub id: String,
    pub route_id: String,
    /// 备注名（展示用）。
    pub name: Option<String>,
    /// provider 预设 key（如 `openai`）。
    pub provider: String,
    /// 上游 base URL（控制面已按预设模板与变量展开，数据面不再做模板替换）。
    pub base_url: String,
    /// 上游凭据明文（API Key 或 Service Account JSON；core 不落盘、不记日志）。
    pub api_key: String,
    /// 凭据类型：api_key / google_service_account。
    #[serde(default = "default_credential_kind")]
    pub credential_kind: String,
    /// 鉴权方式（来自 provider 预设）：bearer / x-api-key / api-key /
    /// google_service_account / sigv4 / none。
    #[serde(default = "default_auth_scheme")]
    pub auth_scheme: String,
    /// Provider 级非敏感选项（`path_template` / `path_params` / `base_url_vars` 等）。
    #[serde(default)]
    pub options: Option<serde_json::Value>,
    /// 可无转换服务的入站 API 格式。**空集合 = 当前没有可服务的入站格式**。
    #[serde(default)]
    pub api_formats: Vec<ApiFormat>,
    /// 该凭据可服务的模型清单（客户端名 → 上游 id）。**必填、可为空**；空 = 不服务任何 model。
    pub models: Vec<ModelMapping>,
    pub enabled: bool,
    /// 选号档位：`0` = 不参与选号；其余按降序分档，档内再按策略排序。
    #[serde(default = "default_priority")]
    pub priority: i64,
}

/// 启动回灌的健康种子（控制面从 `provider_health` 读出）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealthSeed {
    pub provider_id: String,
    pub state: HealthState,
}

/// 运行期参数（网络/超时由控制面随配置下发，冷却与粘性来自 DB settings）。R1-F-7
#[derive(Debug, Clone)]
pub struct CoreOptions {
    pub connect_timeout: Duration,
    pub first_byte_timeout: Duration,
    /// 响应体单次读取（chunk）的超时；防故障上游挂住转发或换号循环。
    pub response_body_timeout: Duration,
    pub cooldowns: Cooldowns,
    pub affinity_ttl: Duration,
    pub request_body_limit: usize,
    /// 非流式响应体上限；与请求侧 32MB 口径保持一致。
    pub response_body_limit: usize,
}

impl Default for CoreOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            first_byte_timeout: Duration::from_secs(60),
            response_body_timeout: Duration::from_secs(10),
            cooldowns: Cooldowns {
                auth: Duration::from_secs(30 * 60),
                rate_limit: Duration::from_secs(5 * 60),
                unavailable: Duration::from_secs(60),
                quota: Duration::from_secs(6 * 3600),
            },
            affinity_ttl: Duration::from_secs(24 * 3600),
            request_body_limit: DEFAULT_REQUEST_BODY_LIMIT,
            response_body_limit: DEFAULT_REQUEST_BODY_LIMIT,
        }
    }
}
