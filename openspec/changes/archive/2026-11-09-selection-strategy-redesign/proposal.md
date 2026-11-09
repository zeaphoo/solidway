# Proposal: selection-strategy-redesign

## Why

现状的三个缺陷在同一个生产会话里暴露出来：

1. **粘性是隐藏的选号修饰**。`engine.rs:572` 起把会话记录命中的凭据无条件放在 `Pick::order()` 首位，`priority` 分档让位；TTL 默认 24 小时（`config.rs:367`）且跨重启恢复。实测后果：`priority = 9` 的 Provider 只剩 1 个样本（且是失败样本：`failure_rate = ALPHA = 0.2` → 可用性 80.0、ttfb 从未写入故为 0），`priority = 1` 的 Provider 41+ 样本持续增长 —— 高档位被一次失败永久晾着，而用户既看不见也停不下来。
2. **策略与粘性是正交维度，语义不可预期**。用户选了「轮询」，得到的却不是轮询，而是「粘性优先的轮询」；想「按优先级压满」只能靠试探。
3. **没有吞吐指标**。spec 明确「`w_price` 与 `w_throughput` 本轮 MUST 恒为 0（本版本无价格与吞吐指标）」，但「token 速度优先」是用户真实需求，也确实是 LLM 路由最有区分度的信号之一。

## What Changes

**策略集重设计（5 个值，档位始终是硬约束）**：

| 值 | 界面名 | 档内排序依据 | 会话记录 |
|---|---|---|---|
| `round_robin` | 轮询 | 该 (路由, 模型) 轮询计数器 | **不读** |
| `last_used` | 最后使用优先 | 该 (路由, 模型, 会话标识) 上次成功的 Provider 置顶，其余按轮询计数器 | **读** |
| `latency` | 响应快优先 | 打分：首字节时延低者优先 | 不读 |
| `throughput` | 吞吐优先 | 打分：输出 token 速度高者优先 | 不读 |
| `auto` | 综合加权 | 打分：uptime / latency / throughput 加权 + 指数 uptime 惩罚 + 探索率 | 不读 |

- **删除 `uptime` 策略**：「可用性优先」的表达由 `auto` 承担（`w_uptime` 默认 0.5，仍可编辑）。存量路由由迁移脚本改写为 `auto`；数据面解析到旧值 `uptime` 时 MUST 映射为 `auto`（防止回滚与旧前端写回）。
- **粘性降级为 `last_used` 策略的一部分**：取消「任何策略下粘性置顶」的全局规则，取消 `selection_config.affinity` 开关（上一版设计中的字段，未发布）。会话记录仍**在所有策略下写入**（成功承载即记录），但**只有 `last_used` 读取** —— 读与写分离，切策略即时生效且不丢历史。
- **会话记录 MUST 服从档位**（跨档位不生效）：这是根因修复。`last_used` 命中的凭据只有在其 `priority` 等于当前候选集合最高档位时才置顶。
- **新增吞吐指标**：`Metrics` 增加「输出 token 速度」EWMA，样本口径 `output_tokens / (duration_ms − ttft_ms)`，仅生成端点且 `output_tokens > 0` 时采样。同时新增阈值 `defaultTokensPerSec`（无样本 / 陈旧时的兜底值），启用 `w_throughput` 并给打分公式加 `throughputRatio`。
- **`round_robin` 去掉粘性视为缺陷修复**：默认策略行为从「粘性优先的轮询」改为「纯轮询」，CHANGELOG 记 Fixed。
- **控制台**：五个策略按钮（每项一句说明）、指标表新增「token/s」列、状态列拆成「指标状态 / 健康」、「最后使用记录」区块（诊断用途，仅在 `last_used` 下影响选号）。

## Capabilities

### New Capabilities

（无）

### Modified Capabilities

- `gateway-core`：
  - 「路由选号策略」：策略枚举重写为 5 值；档位硬约束；`last_used` 语义；各策略「是否读会话记录 / 是否打分」写死。
  - 「会话粘性」：改为「最后使用记录」——所有策略写入、仅 `last_used` 读取、服从档位。
  - 「选号指标」：新增输出 token 速度 EWMA 与采样口径。
  - 「打分公式」：新增 `throughputRatio`，启用 `w_throughput`，更新默认权重表与阈值。
  - 「打分回退与平局打散」：策略清单更新（去掉 `uptime`、加入 `throughput`），补充 `defaultTokensPerSec`。
- `management-api`：
  - 「路由选号策略接口」：`strategy` 枚举 5 值；`providers[]` 增加 `tokens_per_sec`；`resolved.weights.throughput` 与 `thresholds.defaultTokensPerSec` 可配置。
  - 「选号配置校验」：`weights.throughput` 由「必须为 0」改为 `[0, 1]`；新增 `defaultTokensPerSec` 校验；`strategy = "uptime"` MUST 返回 422。
  - 新增「会话记录查看与清除」要求（沿用 `affinity` 资源名）。
- `admin-console`：
  - 「路由选号策略界面」：策略按钮改为五项；指标表新增 token/s 列与两列状态；「最后使用记录」区块。

## Impact

- 数据面 core：
  - `config.rs`：`SelectionStrategy` 变体与默认权重 / 探索率；`SelectionThresholds` 增加 `default_tokens_per_sec`；`falls_back()` 只对打分策略成立。
  - `metrics.rs`：`Stats` 增加 `tokens_per_sec` EWMA；`MetricValue` / `StatsView` 增加该字段；`record_throughput()`。
  - `engine.rs`：响应体结束时按 `usage.output_tokens` 与 `duration_ms − ttft_ms` 采样；会话记录「写始终、读仅 `last_used`」；`pick` 的档位判定。
  - `registry.rs`：`order_candidates` 增加 `LastUsed` 与 `Throughput` 分支；`score_map` 增加吞吐项。
- 控制面 api：`selection.rs`（枚举 / 权重 / 阈值 / 校验）、`handlers.rs`（快照加 `tokens_per_sec`、`affinity` 端点）、`core_bridge.rs`、新迁移 `0002_selection_strategy_redesign.sql`（`uptime` → `auto`）。
- Web：`RouteSelectionSection.jsx`、`useSelectionStrategy.js`、新增 `useRouteAffinity.js`、`locales/{zh-CN,en-US}.json`。
- 文档：`docs/domain-context.md`（新术语「最后使用优先」「吞吐（输出 token 速度）」）、`docs/adr/0004-route-selection-scoring.md`（追加修订记录）、`CHANGELOG.md`。
- **行为变更（记 Fixed）**：`round_robin` 不再使用会话记录；被会话记录锁在低档位的路由在下个请求即回到高档位。
- **不在本 change 内**：吞吐指标不进启动种子（`usage_attempt_bucket` 无 per-provider token 数据），重启后靠新样本累积，期间用 `defaultTokensPerSec` 兜底；价格为 0 权重不变。
