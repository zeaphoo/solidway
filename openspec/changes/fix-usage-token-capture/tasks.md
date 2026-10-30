# Tasks

## 1. 旁路解码（design: proposal「What Changes / 旁路解码」）

- [x] 1.1 新增 `crates/solidway_core/src/decode.rs`：`ContentEncoding::parse`（多值取最外层、未知按 identity）+ `BodyDecoder` 增量解码（gzip / deflate / br / zstd）；gzip 头自行剥离后走裸 inflate（避免 flate2 C zlib 依赖）；deflate 首块失败回退裸 deflate；失败置 `failed` 后续片段丢弃。验证：`cargo test -p solidway-core --lib decode::` 8 passed（含 1 字节切片跨块、双 member、损坏输入）
- [x] 1.2 `success_response` 接线：按上游 `content-encoding` 生成解码器；非流式仅对解码副本解析 JSON（`decode_bytes`，明文上限 32 MiB），转发字节仍用原 `bytes`；流式 `ObservedSseStream` 增加 `decoder`，解码输出接入原按行切分，流结束 flush 残留。验证：`crates/solidway_core/tests/usage_compressed.rs` 两条用例（响应字节与 `gzip_bytes(...)` 逐字节相等 + usage 提取成功）

## 2. 流式用量注入（proposal「流式用量注入」）

- [x] 2.1 `forward` 增加 `want_stream_usage`（仅 `Inbound::ChatCompletions`），每次尝试在 `render_target` 之后调用 `ensure_stream_usage`：顶层 `stream = true` 且无 `stream_options` 时在收尾 `}` 前插入 `"stream_options":{"include_usage":true}`（空对象不加逗号），其余字节不动。验证：`engine::tests::stream_usage_is_injected_only_for_streaming_chat`；集成用例断言上游收到注入后的字节、非流式与显式 `stream_options` 原样

## 3. 依赖与构建

- [x] 3.1 工作区新增 `flate2`（`rust_backend`，纯 Rust）、`brotli-decompressor`、`zstd`（`default-features = false`）；`solidway-core` 依赖与 dev-dependencies（测试压缩用 `flate2`）。验证：`cargo check -p solidway-core` / `cargo test -p solidway-core` 全绿

## 4. 规格同步

- [x] 4.1 delta spec `openspec/changes/fix-usage-token-capture/specs/gateway-core/spec.md`：MODIFIED「保真转发与白名单改写」（白名单 5 → 6 项 + 3 个 Scenario）、MODIFIED「用量元数据提取」（旁路解码要求 + 注入口径 + 2 个 Scenario）。归档时合并进主 spec
- [ ] 4.2 归档本 change 并把 delta 合并进 `openspec/specs/gateway-core/spec.md`（待实机验证 token 非 0 后执行）

## 5. 遗留（不在本 change 范围）

- [ ] 5.1 Web 端呈现 `usage_missing`（表格列或摘要提示），让 token 为 0 时能看出「上游未回传 usage」
