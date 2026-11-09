# admin-console Specification (delta: selection-strategy-redesign)

## MODIFIED Requirements

### Requirement: 路由选号策略界面

控制台 SHALL 在路由详情页提供「选号策略」区块（位于接入信息之后、Provider 列表之前），包含：

- **策略选择**：`轮询` / `最后使用优先` / `响应快优先` / `吞吐优先` / `综合加权` 五项，每项 MUST 附带一句说明（含"只影响选号，不影响请求与响应字节"）。至少包含以下语义：`轮询` = 档内轮流、不使用会话记录；`最后使用优先` = 同一会话复用上次成功的 Provider（同档位内）；`响应快优先` = 按首字节时延；`吞吐优先` = 按输出 token 速度；`综合加权` = 可用性 / 时延 / 吞吐加权；所有策略 MUST 说明「高档位永远先于低档位尝试」；
- **权重与阈值**：仅「综合加权」可编辑（`uptime` / `latency` / `throughput` 三个权重、`explorationRate`；惩罚阈值与兜底值可折叠在「高级」内，兜底值含 `defaultTokensPerSec`）；其它策略 MUST 展示"该策略使用固定权重"并只读显示生效值；「轮询」与「最后使用优先」MUST NOT 展示权重；
- **实时指标表**：Provider / 优先级 / 首字节时延 / uptime / **token/s** / 样本数 / **指标状态**（新鲜 / 陈旧 / 无样本）/ **健康**（正常 / 冷却中 / 已退役）/ 当前分数，SHOULD 每 10 秒刷新；Provider 列 MUST 显示与 Provider 列表一致的显示名（`name`，空则 Provider 标识；复用同一 id→label 映射），MUST NOT 直接渲染 `provider_id`；指标行无法映射到已知 Provider 时 MUST 回退为 `provider_id` 的掩码前缀（如前 8 位）；
- **会话记录区块**：列在指标表下方，展示当前未过期记录条数与明细（模型 / 会话标识 / Provider 显示名 / 过期时刻），支持单条与全部清除；非「最后使用优先」策略下 MUST 标注"当前策略不读取会话记录，仅作诊断"；
- **保存**：保存成功后 MUST 提示"已生效，无需重启"；MUST NOT 出现重启提示。

「指标状态」与「健康」MUST 为两列，MUST NOT 合并成一列：指标状态只表达样本新鲜度（无样本 / 陈旧 / 新鲜），健康只表达数据面健康态。指标陈旧时 MUST NOT 隐藏健康状态。

`token/s` 列 MUST NOT 承诺跨厂商可比，列表后缀或提示 MUST 说明其为「输出 token 速度」。

策略导致数据面回退轮询时（`resolved.fallback = true`），界面 MUST 显示可读提示。

#### Scenario: 默认策略
- **WHEN** 路由从未配置策略
- **THEN** 展示「轮询」为选中项，且不展示可编辑权重

#### Scenario: 策略列表不含可用性优先
- **WHEN** 用户打开策略选择
- **THEN** 只展示五项（不含「可用性优先」），且「综合加权」说明中标注可用性权重默认为 0.5

#### Scenario: 切换到综合加权
- **WHEN** 用户选择「综合加权」
- **THEN** 展示三个权重与探索率输入，默认值来自服务端 `resolved`

#### Scenario: 切换到最后使用优先
- **WHEN** 用户选择「最后使用优先」
- **THEN** 不展示权重，指标表分数列显示「—」，会话记录区块标注其生效

#### Scenario: 保存即时生效
- **WHEN** 用户保存策略
- **THEN** 提示"已生效，无需重启"，指标表按新策略显示分数

#### Scenario: 全零权重提示
- **WHEN** 用户把三个权重都设为 0 并保存
- **THEN** 保存成功，并显示"权重全为 0，当前按轮询选号"的提示

#### Scenario: 指标缺失
- **WHEN** 某 Provider 还没有样本
- **THEN** 该行时延 / token/s / 分数显示为「—」，指标状态为「无样本」，健康列仍展示其真实健康态

#### Scenario: 指标陈旧但健康
- **WHEN** 某 Provider 距最后一次样本超过 30 分钟且当前健康态为正常
- **THEN** 指标状态显示「陈旧」，健康列显示「正常」

#### Scenario: 无吞吐样本
- **WHEN** 某 Provider 只有非流式样本（未满足吞吐采样条件）
- **THEN** token/s 显示为「—」，其余指标正常展示

#### Scenario: 指标表 Provider 显示名
- **WHEN** 路由下存在名为 `alibaba-cn` 的 Provider 且指标表出现其指标行
- **THEN** 该行 Provider 列显示 `alibaba-cn`（与 Provider 列表一致），不显示 UUID

#### Scenario: 指标行无法映射
- **WHEN** 指标行的 `provider_id` 无法映射到当前 Provider 列表
- **THEN** Provider 列回退显示 `provider_id` 的掩码前缀（如前 8 位）

#### Scenario: 查看并清除会话记录
- **WHEN** 用户展开会话记录区块并点击某条记录的清除
- **THEN** 该条目被删除、条目数减一，并提示"已清除，下次请求按档位重新选号"

#### Scenario: 会话记录为空
- **WHEN** 当前路由没有未过期记录
- **THEN** 会话记录区块显示「当前无会话记录」空态
