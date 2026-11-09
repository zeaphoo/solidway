# gateway-core Specification (delta: selection-strategy-redesign)

## MODIFIED Requirements

### Requirement: 路由选号策略

每条路由 MUST 携带一个选号策略，取值为 `round_robin` / `last_used` / `latency` / `throughput` / `auto`；缺省 MUST 为 `round_robin`。策略由控制面随配置下发，变更 MUST 原子生效，MUST NOT 要求重启。

| 策略 | 档内排序依据 | 是否打分 | 是否读会话记录 |
|---|---|---|---|
| `round_robin` | 该 (路由, 模型) 的轮询计数器顺序 | 否 | 否 |
| `last_used` | 该 (路由, 模型, 会话标识) 上次成功承载的 Provider 置顶，其余按轮询计数器顺序 | 否 | 是 |
| `latency` | 首字节时延低者优先（`w_latency` 主导） | 是 | 否 |
| `throughput` | 输出 token 速度高者优先（`w_throughput` 主导） | 是 | 否 |
| `auto` | `uptime` / `latency` / `throughput` 三项加权 + 指数 uptime 惩罚，并按 `explorationRate` 随机探索 | 是 | 否 |

**档位是硬约束**：候选 MUST 先按 `priority` 降序分档，`priority` MUST NOT 进入打分（不作惩罚项、不作权重）；任何策略 MUST NOT 把低档位候选排在高档位候选之前。会话记录同样 MUST 服从档位（见「会话粘性」）。

`last_used` 在无会话标识或无记录时 MUST 等同于 `round_robin`；`round_robin` MUST NOT 读取会话记录。`latency` / `throughput` / `auto` MUST NOT 读取会话记录。

`round_robin` / `last_used` 下 `explorationRate`、`weights`、`thresholds` MUST NOT 生效（含权重和为 0 的回退判定），快照中的 `score` MUST 为 `null`。

#### Scenario: 轮询保持默认行为
- **WHEN** 路由未显式配置策略
- **THEN** 按轮询计数器顺序选号，不使用会话记录，与「粘性置顶」修复前的差异仅在于不再受会话记录影响

#### Scenario: 轮询不使用会话记录
- **WHEN** 策略为 `round_robin`，某会话上次由凭据 B 承载，且同档位存在凭据 A（轮询顺序在前）
- **THEN** 先尝试 A，MUST NOT 因会话记录把 B 置顶

#### Scenario: 最后使用优先
- **WHEN** 策略为 `last_used`，某会话上次由同档位的凭据 B 承载
- **THEN** 该会话的后续请求先尝试 B，其余候选按轮询计数器顺序

#### Scenario: 无会话标识回退轮询
- **WHEN** 策略为 `last_used`，请求体不含任何会话标识字段
- **THEN** 按轮询计数器顺序选号

#### Scenario: 最后使用不越过档位
- **WHEN** 策略为 `last_used`，会话记录指向 `priority = 1` 的凭据，而同模型存在健康的 `priority = 9` 凭据
- **THEN** 先尝试 `priority = 9` 的凭据，MUST NOT 因会话记录先试 `priority = 1` 的凭据

#### Scenario: 时延优先
- **WHEN** 策略为 `latency`，两个健康凭据的 EWMA 首字节时延分别为 200ms 与 900ms
- **THEN** 先尝试 200ms 的凭据

#### Scenario: 吞吐优先
- **WHEN** 策略为 `throughput`，两个健康凭据的 EWMA 输出 token 速度分别为 80/s 与 20/s
- **THEN** 先尝试 80/s 的凭据

#### Scenario: 可用性优先
- **WHEN** 策略为 `auto`，两个健康凭据的 EWMA 失败率分别为 0 与 0.3，其余指标相同
- **THEN** 先尝试失败率为 0 的凭据（`w_uptime` 主导）

#### Scenario: 粘性优先于策略
- **WHEN** 策略为 `last_used`，同一档位内两个健康凭据，会话记录命中其中时延不是最低的那个
- **THEN** 先尝试该记录的凭据

#### Scenario: 粘性不可用时跳过
- **WHEN** 会话记录指向的凭据 `priority = 0`、不健康或不支持当前入站格式
- **THEN** 跳过该凭据，按策略顺序继续

### Requirement: 会话粘性

网关 MUST 按家族从请求中提取会话标识，并按 (路由, 模型, 会话标识) 维护**内存**「最后使用记录」（滑动 TTL）：

- `/v1/responses`：`previous_response_id` → `conversation` → `prompt_cache_key` → `user`；
- `/v1/chat/completions`：`prompt_cache_key` → `user`；
- `/v1/messages`：请求体 `metadata.user_id`，缺失时 MUST NOT 记录；
- `/v1/messages/count_tokens`：MUST NOT 记录（非生成端点，无会话语义）。

**写**：任何策略下，成功承载（收到上游响应头）MUST 记录或更新为该 Provider；换号未成功 MUST NOT 更新。
**读**：只有 `last_used` 策略 MUST 读取；其余策略 MUST NOT 读取，也 MUST NOT 因记录改变顺序。
**服从档位**：仅当记录指向的凭据处于当前候选集合的最高档位时才生效；低于该档位时 MUST 视为未命中，且条目 MUST NOT 被删除。记录指向的凭据不健康、`priority = 0` 或不支持当前格式时 MUST 跳过并按策略顺序继续，记录 MUST NOT 被删除。

记录 MAY 由控制面以种子形式注入（启动时恢复）：种子条目 MUST 带过期时刻，已过期的条目 MUST 被忽略；MUST 只在本地无该 (路由, 模型, 会话标识) 记录时采用，MUST NOT 覆盖运行期已更新的记录。

#### Scenario: 同会话复用
- **WHEN** 某会话标识的请求由凭据 A 承载且 A 健康
- **THEN** 同标识的后续请求在 `last_used` 策略下仍优先由 A 承载

#### Scenario: 换号跟随
- **WHEN** 记录指向的凭据失效并换由 B 成功承载
- **THEN** 该会话后续在 `last_used` 策略下优先由 B 承载

#### Scenario: 重启后恢复
- **WHEN** core 以包含该会话条目的种子启动，且条目未过期
- **THEN** 该会话的首个请求在 `last_used` 策略下优先由种子里的凭据承载

#### Scenario: 过期种子被忽略
- **WHEN** 种子里的条目 `expires_at` 早于当前时刻
- **THEN** 该条目被丢弃，按无记录处理

#### Scenario: 不覆盖运行期数据
- **WHEN** 运行期已把某会话更新为凭据 B，随后配置再次下发带有该会话的旧种子（凭据 A）
- **THEN** 仍由 B 承载

#### Scenario: Anthropic 会话键
- **WHEN** 两个 `POST /v1/messages` 请求的 `metadata.user_id` 相同，策略为 `last_used`
- **THEN** 后一个请求优先复用前一个的承接 Provider

#### Scenario: 非 last_used 策略仍写入记录
- **WHEN** 策略为 `round_robin`，某会话由凭据 B 成功承载
- **THEN** 记录被写入或更新为 B，但本次及后续选号 MUST NOT 读取它

#### Scenario: 记录不越过档位
- **WHEN** 记录指向 `priority = 1` 的凭据，同模型存在健康的 `priority = 9` 凭据
- **THEN** 先尝试 `priority = 9` 的凭据，记录条目 MUST NOT 被删除

### Requirement: 选号指标

数据面 MUST 为每个 Provider 凭据在**内存**中维护选号指标：首字节时延 EWMA、失败率 EWMA 与输出 token 速度 EWMA。指标 MUST NOT 持久化、MUST NOT 读取数据库或配置文件。

- 成功拿到上游响应头时 MUST 更新首字节时延与失败率（失败率向 0 收敛）；
- 标记类失败（凭据级 401/403/429/5xx/传输失败）MUST 更新失败率（向 1 收敛），MUST NOT 更新首字节时延与 token 速度；
- 请求级错误（不标记凭据的 4xx）MUST NOT 计入任何指标；
- 输出 token 速度 `tokens_per_sec = output_tokens / (duration_ms − ttft_ms)`，MUST 只在「生成端点、`output_tokens > 0`、`duration_ms > ttft_ms`」时采样；不满足时 MUST NOT 写入该指标，也 MUST NOT 增加该指标的样本数；
- 缺失指标（无样本，或距最后一次样本超过 30 分钟）MUST 使用默认值：`defaultUptime`、`defaultLatencyMs` 与 `defaultTokensPerSec`；
- 同一 Provider 的指标 MUST 在路由之间共享，进程重启后清零；token 速度 MUST NOT 由启动种子恢复（历史用量按 Provider 聚合不含 token 与生成时长）。

#### Scenario: 成功样本更新时延
- **WHEN** 某凭据成功返回首字节
- **THEN** 该凭据的时延 EWMA 与失败率被更新，样本数 +1

#### Scenario: 失败样本不影响时延
- **WHEN** 某凭据返回 503
- **THEN** 失败率上升，时延 EWMA 与 token 速度 EWMA 保持不变

#### Scenario: 请求级错误不计入
- **WHEN** 某凭据返回不标记的 400
- **THEN** 该凭据的指标不发生任何变化

#### Scenario: 流式样本更新 token 速度
- **WHEN** 某流式请求首字节耗时 300ms、总耗时 2300ms、上游 usage 报 `output_tokens = 200`
- **THEN** 该凭据的 token 速度 EWMA 按 200 ÷ 2s = 100/s 更新

#### Scenario: 无 usage 不采样
- **WHEN** 某请求成功但响应不含 usage（`output_tokens` 缺失）
- **THEN** token 速度 EWMA 与样本数均不变化

#### Scenario: 非流式短尾不采样
- **WHEN** 某请求 `duration_ms` 与 `ttft_ms` 之差 ≤ 0
- **THEN** token 速度 MUST NOT 被采样

#### Scenario: 陈旧指标
- **WHEN** 某凭据最后一次样本在 31 分钟前
- **THEN** 打分使用默认值（含 `defaultTokensPerSec`），并把它标记为陈旧

### Requirement: 打分公式

打分策略（`latency` / `throughput` / `auto`）MUST 使用同一打分公式（越小越好）：

~~~txt
score = (w_uptime/Σw)·uptimeRatio + (w_latency/Σw)·latencyRatio
      + (w_throughput/Σw)·throughputRatio + uptimePenalty

uptimeRatio     = maxUptime / uptime − 1
latencyRatio    = ttfb / minTtfb − 1
throughputRatio = minThroughput / throughput − 1
uptimePenalty   = uptime < uptimePenaltyThreshold
                ? ((uptimePenaltyThreshold − uptime)/uptimePenaltyThreshold × 5)²
                : 0
~~~

各项含义：`uptime = (1 − failureRate) × 100`；`throughput` 为输出 token 速度（tokens/s）；`minTtfb` / `maxUptime` / `minThroughput` 取当前候选集合内的极值；缺失指标用默认值代入。

默认权重：`latency` 策略 `w_uptime=0.1 / w_latency=0.9 / w_throughput=0`；`throughput` 策略 `w_uptime=0.1 / w_latency=0 / w_throughput=0.9`；`auto` 策略 `w_uptime=0.5 / w_latency=0.25 / w_throughput=0.25`。`w_price` MUST 恒为 0（本版本无价格指标）。

#### Scenario: 指数惩罚生效
- **WHEN** 某候选 uptime = 80%，另一候选 99%
- **THEN** 前者额外承担约 0.62 的惩罚项，可被后者反超

#### Scenario: 吞吐主导
- **WHEN** 策略为 `throughput`，两个候选 uptime 相同，token 速度分别为 80/s 与 20/s
- **THEN** 前者分数更低（更优）

#### Scenario: 分数不含档位
- **WHEN** 两个候选指标相同但 `priority` 不同
- **THEN** 二者分数相同，顺序由档位决定（见「Provider 优先级（档位）」）

### Requirement: 打分回退与平局打散

打分策略权重和 `Σw ≤ 0` 时 MUST 回退 `round_robin`，MUST NOT 报错。分数相同（或候选全部无样本）时 MUST 按该 (路由, 模型) 的轮询计数器打散，保证公平与探索。

`auto` 策略的 `explorationRate`（缺省 `0.01`）MUST 按比例随机跳过打分、改用轮询顺序，为冷启动提供样本；`round_robin` / `last_used` / `latency` / `throughput` 策略下该值 MUST NOT 生效。

阈值缺省：`uptimePenaltyThreshold=95`、`defaultUptime=100`、`defaultLatencyMs=1000`、`defaultTokensPerSec=30`。

#### Scenario: 权重和为零回退轮询
- **WHEN** `auto` 策略的三项权重都被配置为 0
- **THEN** 按 `round_robin` 选号，不返回错误

#### Scenario: 非打分策略不回退
- **WHEN** 策略为 `last_used`，其权重字段被配置为 0
- **THEN** 该配置 MUST NOT 触发回退，仍按 `last_used` 语义选号

#### Scenario: 同分打散
- **WHEN** 两个候选分数相同（或都无样本）
- **THEN** 按轮询计数器顺序决定先试谁

#### Scenario: 探索取样
- **WHEN** `auto` 策略的 `explorationRate = 1`
- **THEN** 每次请求都按轮询顺序选号（不按打分）

#### Scenario: 缺少 token 速度样本
- **WHEN** 某一候选无 token 速度样本，另一候选有
- **THEN** 缺失方按 `defaultTokensPerSec` 代入参与打分
