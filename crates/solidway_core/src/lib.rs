//! SolidWay 数据面引擎（纯内存）。
//!
//! **硬边界**（见 `openspec/changes/add-gateway-core/design.md` D1）：
//!
//! - 不做任何数据库操作（依赖不含 `sqlx`）；
//! - 不读取配置文件 / 启动配置环境变量；
//! - 不提供管理端点，只提供转发端点与 `/health`。
//!
//! 输入 = 由控制面（`solidway-api`）构造并**全量下发**的 [`CoreConfig`]；
//! 输出 = [`CoreEvent`] 事件流，由控制面消费并持久化。

pub mod affinity;
pub mod config;
pub mod decode;
pub mod engine;
pub mod event;
pub mod health;
pub mod inbound;
pub mod metrics;
pub mod registry;
pub mod sigv4;
pub mod usage;

pub use affinity::Affinity;
pub use config::{
    AffinitySeed, ApiFormat, ApiKeyEntry, CoreConfig, CoreOptions, ModelMapping, ProviderEntry,
    ProviderHealthSeed, ProviderMetricSeed, RouteEntry, RouteSelection, SelectionStrategy,
    SelectionThresholds, SelectionWeights,
};
pub use decode::{BodyDecoder, ContentEncoding};
pub use engine::{Core, CoreError, GatewayErrorCode, hash_api_key};
pub use event::{
    AccessEvent, AttemptEvent, CompletionEvent, CompletionOutcome, CoreEvent, EventBus,
    HealthChangedEvent,
};
pub use health::{
    Classification, Cooldowns, Failure, HealthManager, HealthState, Mark, MarkReason, Probing,
    TransportError, classify, epoch_secs, parse_retry_after,
};
pub use inbound::{
    Endpoint, Family, Inbound, clean_query, normalize_base_url, upstream_url,
};
pub use metrics::{MetricValue, Metrics, StatsView};
pub use registry::{
    KeyBinding, Pick, ProviderRuntime, ProviderSnapshot, Registry, RouteRuntime, Selector,
};
pub use usage::TokenUsage;
