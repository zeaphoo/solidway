# Design

## 1. 为什么把「粘性」降级成策略 `last_used`

粘性原本是「任何策略下都生效的隐藏修饰」，导致三个问题：用户选了轮询却得到「粘性优先的轮询」；粘性跨档位锁死高档位（本 change 的触发缺陷）；粘性不可见。

重设计后：**策略是唯一变量**。会话记录（粘性）从修饰词变成 `last_used` 一个策略的内部实现，`round_robin` / `latency` / `throughput` / `auto` 明确不读它。想关闭粘性 = 选别的策略，不再需要 `selection_config.affinity` 开关（上一版设计中的字段，未发布，直接取消）。

**读 / 写分离**：所有策略都**写入**会话记录（成功承载即记录），只有 `last_used` **读取**。这样：
- 切到 `last_used` 立刻有历史可用，不需要预热；
- 切走再切回不丢失；
- 写入成本是一张内存 map 的 upsert，可忽略。

## 2. `last_used` 的作用域：会话级，不是路由级

两种可能语义：

| 语义 | 行为 | 采用 |
|---|---|---|
| 会话级 | (路由, 模型, 会话标识) 维度记录，无会话标识的请求回退轮询 | ✅ |
| 路由级 | 整个 (路由, 模型) 一直用上次成功的 Provider | ❌ |

路由级等于「一主到底」，会让任何一次抖动都换主，且与「轮询」形成无意义的重叠。会话级保留粘性的真实收益（前缀缓存的 provider 亲和），并与轮询形成清晰对照。

会话标识仍按家族提取（`previous_response_id` / `conversation` / `prompt_cache_key` / `user` / `metadata.user_id`），规则不变。

## 3. 档位是硬约束（根因修复）

`Selector::pick` 中接受会话记录的条件追加档位判定：记录指向的凭据 `priority` 必须等于当前候选集合（健康 + 服务该模型与格式 + 启用 + `priority > 0`）的最高档位。低于该档位时视为未命中（**条目保留、不删除**）：

- 高档位冷却结束后自然回到高档位（成功承载时记录被刷新为该 Provider），无需迁移或清理任务；
- 高档位冷却期间，低档位的记录恰好就是最高可用档位，命中恢复生效，前缀缓存收益在降档期保留。

## 4. 删除 `uptime` 策略

「可用性优先」保留为 `auto` 的一个默认权重（`w_uptime = 0.5`），而不是独立策略：

- `uptime` 策略本质是「w_uptime 主导的打分」，与 `latency` / `throughput` 是同构的，值本身没有独特实现；
- 五个值里已经有两个打分策略，第三个会让「策略」列表变成权重预设清单；
- 指数 uptime 惩罚对**所有**打分策略都生效，所以可用性永远不会被忽略。

迁移：`0002_selection_strategy_redesign.sql` 把 `routes.selection_strategy = 'uptime'` 改为 `'auto'`；数据面 `SelectionStrategy::parse("uptime")` 映射为 `Auto`（防回滚与旧前端写回）；写入接口对 `uptime` MUST 返回 422。

## 5. 吞吐指标口径

```
tokens_per_sec = output_tokens / max(duration_ms − ttft_ms, ε)
```

- **分母**用「首字节之后」的时间，即纯生成速度；首字节延迟已经由 `latency` 指标单独衡量，避免两个策略衡量同一件事。
- **采样条件**：生成端点（`generation = true`）、`usage.output_tokens > 0`、`duration_ms > ttft_ms`。任一不满足即跳过该样本（不写 EWMA、不计样本数）。
  - 非流式请求常常 `duration_ms ≈ ttft_ms`，会被大量跳过 —— 这是有意的：非流式下这个商数的物理意义不稳定，宁可没有样本（回落到 `defaultTokensPerSec`）也不要噪声。
- **usage 缺失**：流式请求未带 `include_usage` 时没有 `output_tokens`，不计样本（`fix-usage-token-capture` 已把 OpenAI 家族流式的 `include_usage` 注入，覆盖面已提升）。
- **reasoning token**：`output_tokens` 含思维链 token，跨厂商口径不可比。界面文案写「输出 token 速度」，作为同厂商内的相对参考，不承诺跨厂商可比。
- **不进启动种子**：`usage_attempt_bucket` 只有 per-provider 的请求数 / 成功数 / 时延和，没有 per-provider token 与生成时长，无法聚合出吞吐种子。重启后吞吐指标清零，期间用 `defaultTokensPerSec`（默认 30）兜底，样本攒够后接管。
- EWMA 系数与既有指标一致（`ALPHA = 0.2`），陈旧判定同样 30 分钟。

## 6. 打分公式与默认权重

```
score = (w_uptime/Σw)·uptimeRatio + (w_latency/Σw)·latencyRatio
      + (w_throughput/Σw)·throughputRatio + uptimePenalty

throughputRatio = minThroughput / throughput − 1   （吞吐越高，比值越小，越好）
```

| 策略 | w_uptime | w_latency | w_throughput | 探索率 |
|---|---|---|---|---|
| `latency` | 0.1 | 0.9 | 0 | 0 |
| `throughput` | 0.1 | 0 | 0.9 | 0 |
| `auto` | 0.5 | 0.25 | 0.25 | 0.01 |

`w_price` 仍恒为 0（无价格指标）。`round_robin` / `last_used` 不打分，权重不参与；`falls_back()`（权重和 ≤ 0 回退轮询）只对三个打分策略成立。

阈值默认值：`uptimePenaltyThreshold=95`、`defaultUptime=100`、`defaultLatencyMs=1000`、`defaultTokensPerSec=30`。

## 7. 同档多个 Provider 的顺序仍由档位 / 策略决定

不再引入「档内固定顺序」字段（ADR 0004 已禁止 `sort_order`）。要严格主备就把 `priority` 填成不同值；同档即「等价可替换」，档内按策略打散。

## 8. 状态列拆分

`RouteSelectionSection.jsx:323` 现在是三目 `samples === 0 ? 无样本 : stale ? 陈旧 : HEALTH_LABEL[health]`。「指标新鲜度」（`metrics.rs` 的 30 分钟陈旧判定）与「健康状态」（`health.rs` 的 `Healthy` / `Quarantined` / `Retired`）是正交维度，合并后「陈旧」会盖住健康态。拆成两列，并新增「token/s」列。
