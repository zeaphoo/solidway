//! core → 控制面的事件流。core 只发事件，不做任何持久化。

use tokio::sync::broadcast;

use crate::health::HealthState;
use crate::usage::TokenUsage;

const DEFAULT_CAPACITY: usize = 4096;

/// 数据面事件。控制面订阅后负责持久化 / 计量。
#[derive(Debug, Clone)]
pub enum CoreEvent {
    /// 一次逻辑请求（L1 元数据）。
    Access(AccessEvent),
    /// 一次逻辑请求的响应体结束 / 中断（L2 元数据，含 token）。
    Completion(CompletionEvent),
    /// 对某个 provider 凭据的一次尝试（换号诊断）。
    Attempt(AttemptEvent),
    /// provider 健康状态变更（控制面持久化）。
    HealthChanged(HealthChangedEvent),
}

/// 逻辑请求级别的访问记录。
#[derive(Debug, Clone)]
pub struct AccessEvent {
    pub request_id: String,
    /// 请求开始时间（epoch 秒）。
    pub at: i64,
    pub route_id: String,
    pub api_key_id: String,
    /// 客户端模型名（别名映射前）。
    pub model: String,
    /// 最终承接的 provider；无承接时为空。
    pub provider_id: String,
    /// 入站 API 格式。
    pub api_format: String,
    /// 是否为 SSE 流式响应。
    pub is_stream: bool,
    pub status: u16,
    pub attempts: u32,
    /// 首字节时间（毫秒）；失败路径为总耗时（失败请求不参与 TTFB 语义）。R1-F-12③
    pub ttfb_ms: u64,
    pub bytes_in: u64,
}

/// 响应体结束 / 中断事件。
#[derive(Debug, Clone)]
pub struct CompletionEvent {
    pub request_id: String,
    /// 与对应 `AccessEvent` 相同的请求开始时间，保证归入同一时间桶。
    pub at: i64,
    pub route_id: String,
    pub api_key_id: String,
    pub model: String,
    pub provider_id: String,
    pub outcome: CompletionOutcome,
    /// 从转发开始到响应体结束的总耗时（毫秒）。
    pub duration_ms: u64,
    /// 写出字节数，以**改写后**的响应流为准（别名回写不改变长度语义）。R1-F-12④
    pub bytes_out: u64,
    pub usage: Option<TokenUsage>,
    /// 是否生成端点。`false`（`count_tokens` / `countTokens`）表示请求计入请求数，
    /// 但不计 token、也不计入「用量缺失」。
    pub generation: bool,
}

/// 请求完成状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionOutcome {
    /// 响应体正常结束。
    Completed,
    /// 客户端断开或网关侧流被丢弃。
    Aborted,
    /// 响应体传输过程中上游报错。
    UpstreamError,
}

impl CompletionOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            CompletionOutcome::Completed => "completed",
            CompletionOutcome::Aborted => "aborted",
            CompletionOutcome::UpstreamError => "upstream_error",
        }
    }
}

/// 单次 provider 尝试。
#[derive(Debug, Clone)]
pub struct AttemptEvent {
    pub request_id: String,
    pub at: i64,
    pub route_id: String,
    /// 客户端模型名。
    pub model: String,
    pub provider_id: String,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub latency_ms: u64,
}

/// provider 健康状态迁移。
#[derive(Debug, Clone)]
pub struct HealthChangedEvent {
    pub provider_id: String,
    pub state: HealthState,
    /// epoch 秒。
    pub at: i64,
}

/// 事件总线：`broadcast` 通道，无订阅者时事件被丢弃（core 不阻塞、不缓存）。
#[derive(Debug, Clone)]
pub struct EventBus {
    tx: broadcast::Sender<CoreEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.tx.subscribe()
    }

    /// 发布事件；无订阅者时静默丢弃。
    pub fn emit(&self, event: CoreEvent) {
        let _ = self.tx.send(event);
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
