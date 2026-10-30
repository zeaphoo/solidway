# Proposal: fix-usage-token-capture

## Why

用量统计里的 token（输入 / 输出 / 缓存）恒为 0，而请求数、延迟、字节数都正常。实测数据面采集链路本身没问题：`usage_bucket` 里 `completed = 16` 且 `usage_missing = 16`，说明 16 个正常完成的请求全部「上游响应里解析不到 usage」，于是 token 按 spec 写 0。

两个成因都在「提取」这一层：

1. **压缩响应未解码**：网关按 spec 把客户端请求头（含 `accept-encoding`）原样转发，上游据此返回 `content-encoding: gzip`（实测 `token-plan.cn-beijing.maas.aliyuncs.com` 连 401 JSON 都压）。但 reqwest 未启用解压 feature，网关把压缩字节直接当 JSON/SSE 解析，`serde_json` 与按行切分必然失败。响应照原样带着 `content-encoding` 透传，客户端自己解压，所以线上功能正常、只有统计坏掉；`bytes_out` 记的也是压缩后字节。
2. **OpenAI 家族流式默认不带 usage**：`stream: true` 时上游只在请求带 `stream_options.include_usage` 才返回 usage chunk。原 spec 明确禁止网关注入，而多数客户端不会自己带，于是这类请求永远没有 token。Anthropic 原生流与 Responses 自带 usage，不受影响。

且 `usage_missing` 只存在于 API 响应，Web 端没有任何呈现，用户只看到几列 0，无法定位。

## What Changes

- **旁路解码**：数据面按上游响应的 `content-encoding`（`gzip` / `deflate` / `br` / `zstd`）增量解码，解出的明文只喂给 usage 解析器。**转发字节完全不变**：压缩体照原样透传，`content-encoding` 照原回给客户端，`bytes_out` 仍计原始字节。解码失败或未知编码静默降级为「解析不到 usage」，不影响转发。
- **非流式**：整段解码后再解析 JSON usage；解压上限 32 MiB（防解压炸弹）。
- **流式**：`ObservedSseStream` 增加增量解码器，解码输出接原有的按行切分，跨 chunk 的压缩帧与 SSE 行都能正确处理；流结束时 flush 解码器残留。
- **流式用量注入**：OpenAI Chat Completions 入站、顶层 `stream = true`、且请求体未声明 `stream_options` 时，在请求体顶层末尾补 `"stream_options":{"include_usage":true}`。只做定点插入（不重排、不重序列化），已显式声明 `stream_options` 的请求原样不动；Responses / Anthropic 注入无意义，不做。
- **spec 同步**：`gateway-core` 的「保真转发与白名单改写」白名单由五项扩为六项；「用量元数据提取」增加「按 `content-encoding` 旁路解码」要求，并把「MUST NOT 注入 `stream_options`」改为「SHALL 在缺失时补 `include_usage`」。
- **可观测性**：`usage_missing` 已在 API 返回，Web 端后续单独排期呈现（本 change 不做 UI）。

## Capabilities

### New Capabilities

（无）

### Modified Capabilities

- `gateway-core`：
  - 「保真转发与白名单改写」：请求改写白名单新增第 6 项（流式用量参数），明确「其余字节 MUST NOT 改动」不覆盖该定点插入。
  - 「用量元数据提取」：新增压缩响应旁路解码要求；`stream_options.include_usage` 由「MUST NOT 注入」改为「缺失时 MUST 补 `true`」，并补对应 Scenario。

## Impact

- 代码：`crates/solidway_core/src/decode.rs`（新增）、`engine.rs`（旁路解码接线 + `ensure_stream_usage`）。
- 依赖（仅数据面，纯 Rust、无 C 依赖）：新增 `flate2`（rust_backend）、`brotli-decompressor`、`zstd`（`default-features = false`）。
- 测试：`decode` 单测（4 种编码 / 跨块 / 损坏输入 / 多 member）、`engine` 注入单测、集成测试 `tests/usage_compressed.rs`（gzip JSON / gzip SSE 取到 usage 且转发字节逐字节一致；注入只在流式且未声明时发生）。
- 行为面：上游会多收到 `stream_options.include_usage`（OpenAI Chat 流式），响应会多一个 `choices: []` 的 usage chunk —— 与官方 SDK 行为一致。
- 不变：`accept-encoding` 转发策略、响应字节、`content-encoding` 头、非流式请求体。
