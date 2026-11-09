# Tasks

## 1. 数据面 core：策略枚举与排序

- [x] 1.1 `crates/solidway_core/src/config.rs`：`SelectionStrategy` 改为 `RoundRobin` / `LastUsed` / `Latency` / `Throughput` / `Auto`（`as_str`/`parse` 同步）；`parse("uptime")` 映射为 `Auto`；`ALL` 长度 5
- [x] 1.2 默认权重：`Latency` = 0.1/0.9/0，`Throughput` = 0.1/0/0.9，`Auto` = 0.5/0.25/0.25；`price` 恒 0
- [x] 1.3 `SelectionThresholds` 增加 `default_tokens_per_sec`（缺省 `30.0`）
- [x] 1.4 `falls_back()` / `effective_strategy()` 只对 `Latency` / `Throughput` / `Auto` 生效（`LastUsed` 权重为 0 不回退）
- [x] 1.5 `registry.rs`：`order_candidates` 增加 `LastUsed`（档内先放会话记录命中的凭据，其余按轮询计数器）与 `Throughput`（打分）分支；`LastUsed` 不打分
- [x] 1.6 `registry.rs`：`Selector::pick` 传入会话记录时追加档位判定（记录的凭据 `priority` == 候选最高档位），仅在 `LastUsed` 策略下查表
- [x] 1.7 `score_map`：增加 `throughputRatio = minThroughput / throughput − 1` 与 `w_throughput` 项
- [x] 1.8 单测：`config.rs`（默认权重 / 解析 / 回退判定）、`registry.rs`（5 策略的排序、档位硬约束、`LastUsed` 档内生效）

## 2. 数据面 core：吞吐指标

- [x] 2.1 `metrics.rs`：`Stats` 增加 `tokens_per_sec` EWMA；`MetricValue` / `StatsView` 增加 `tokens_per_sec`；新增 `record_throughput(provider_id, tps, now)`
- [x] 2.2 `metrics.rs`：`STALE_SECS` 判定覆盖新指标；`missing()` 与 `view()` 同步
- [x] 2.3 `engine.rs`：响应体结束时按「生成端点 + `usage.output_tokens > 0` + `duration_ms > ttft_ms`」采样 `output_tokens / (duration_ms − ttft_ms)`
- [x] 2.4 单测：`metrics.rs`（EWMA 收敛、陈旧、缺失）+ `engine.rs`（采样条件、非流式不采样）
- [x] 2.5 `registry.rs` `snapshot()` 与 `ProviderSnapshot` 增加 `tokens_per_sec`

## 3. 数据面 core：会话记录

- [x] 3.1 `engine.rs`：成功承载后 **无论策略** 都 `remember`；只有 `LastUsed` 策略 `lookup`
- [x] 3.2 `affinity.rs`：新增 `list(route_id, limit)` 与 `remove(route_id, Option<(&model, &session)>) -> usize`
- [x] 3.3 `Core` 门面：`list_affinity`、`clear_affinity`
- [x] 3.4 集成测试：`crates/solidway_core/tests/selection.rs`——`round_robin` 不读记录、`last_used` 读记录、记录不越过档位、高档位回池后回到高档位

## 4. 控制面 api

- [x] 4.1 `crates/solidway_api/src/selection.rs`：`WeightsOverride.throughput` 改回可写 `[0,1]`；新增 `thresholds.default_tokens_per_sec`；`resolve` 合并；`from_row` 对 `uptime` 走 `parse` 映射
- [x] 4.2 `handlers.rs`：`put_selection_strategy` 校验表按新枚举与 `default_tokens_per_sec`；`strategy = "uptime"` 返回 422（提示改用 `auto`）
- [x] 4.3 `handlers.rs`：`ProviderSelectionView` 增加 `tokens_per_sec`
- [x] 4.4 新增 `GET /api/routes/{id}/affinity`、`DELETE /api/routes/{id}/affinity`（owner 隔离、`model`+`session` 可选、返回 `{ removed }`）
- [x] 4.5 迁移 `crates/solidway_api/migrations/0002_selection_strategy_redesign.sql`：`UPDATE routes SET selection_strategy='auto' WHERE selection_strategy='uptime'`
- [x] 4.6 接口测试：`tests/selection.rs`——5 策略存取、`uptime` 422、吞吐权重与兜底值校验、`tokens_per_sec` 出现在快照；`tests/affinity.rs`——列举 / 清除 / 404

## 5. 控制台

- [x] 5.1 `RouteSelectionSection.jsx`：`STRATEGIES` 改为五项并补说明文案；`EDITABLE = auto` 保持；`LastUsed`/`RoundRobin` 不展示权重
- [x] 5.2 指标表：新增 `token/s` 列；拆出「指标状态」与「健康」两列（删掉现有三目合并）
- [x] 5.3 会话记录区块（`useRouteAffinity.js`）：条数 + 明细 + 单条 / 全部清除；非 `last_used` 策略标注「仅诊断」
- [x] 5.4 `locales/zh-CN.json`、`en-US.json`：五项策略名与说明、「可用性优先已并入综合加权」的迁移提示、token/s 列、两列状态、会话记录区块

## 6. 文档与规格

- [x] 6.1 `docs/domain-context.md`：新增「最后使用优先（last-used）」「输出 token 速度（throughput）」术语；「会话粘性」改为「仅在 `last_used` 策略下生效且服从档位」
- [x] 6.2 `docs/adr/0004-route-selection-scoring.md`：追加修订记录（5 策略集；删除 `uptime`；粘性降级为策略；吞吐指标口径与不设种子的理由）
- [x] 6.3 `CHANGELOG.md`：`[Unreleased]` 记 Added（`last_used` / `throughput` 策略、吞吐指标、token/s 与记录区块）、Changed（`auto` 默认权重含吞吐）、Fixed（`round_robin` 不再受会话记录影响、记录不越过档位）、Removed（`uptime` 策略 → `auto`）
- [x] 6.4 实现完成后归档本 change，把 delta 合并进 `openspec/specs/{gateway-core,management-api,admin-console}/spec.md`

## 7. 验证

- [x] 7.1 `cargo test -p solidway_core -p solidway_api` 全绿
- [x] 7.2 手工验证：`round_robin` 下高档位 Provider 每请求都被先尝试（指标样本数增长）；`last_used` 下同会话固定；切到 `throughput` 后分数按 token/s 变化
- [x] 7.3 存量数据检查：迁移后不存在 `selection_strategy = 'uptime'`，且原 `uptime` 路由的选号顺序符合 `auto` 默认权重
