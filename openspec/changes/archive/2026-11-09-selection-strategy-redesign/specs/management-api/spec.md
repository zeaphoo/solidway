# management-api Specification (delta: selection-strategy-redesign)

## MODIFIED Requirements

### Requirement: 路由选号策略接口

每条路由记录 MUST 额外包含选号策略字段 `selection_strategy`（缺省 `round_robin`）与 `selection_config`（JSON，缺省 `{}`）；`selection_config` MUST 只保存用户显式覆盖的字段，缺省值由系统补全；既有路由 CRUD 的字段与语义不变。存量路由取缺省值即与引入本能力之前行为一致。

系统 SHALL 提供（资源名 `selection-strategy` 与列名 `selection_strategy`、领域术语「选号策略」一致）：

- `GET /api/routes/{id}/selection-strategy`：返回 `strategy`、`config`（存储的覆盖项）、`resolved`（补全默认值后的完整配置，含 `weights`（`uptime` / `latency` / `throughput`）/ `thresholds`（含 `defaultTokensPerSec`）/ `fallback`）与 `providers[]`（每个 Provider 的 `priority`、健康状态、实时指标——`ttfb_ms` / `uptime_pct` / `tokens_per_sec` / `samples` / `stale`——与当前分数）；
- `PUT /api/routes/{id}/selection-strategy`：校验 → 落库 → **立即下发**数据面 → 返回与 GET 相同结构。

GET 除配置外 MUST 附带该策略的**生效快照**（`providers[]` 的指标与分数）：策略与其生效结果是同一资源的两面，拆成两个端点会出现「刚保存策略、分数还是旧的」的时序错觉。

owner 隔离与既有路由接口一致：他人路由 MUST 返回 404。

`providers[]` 的指标 MUST 取自数据面内存快照，MUST NOT 由控制面重新计算分数；无样本时 `ttfb_ms`、`tokens_per_sec` 与 `score` 为 `null`、`stale = true`。策略为 `round_robin` 或 `last_used` 时 `score` MUST 恒为 `null`。数据面未装配时 `providers` MUST 为空数组而非报错。

存量 `selection_strategy = 'uptime'` MUST 由迁移改写为 `'auto'`；`resolved` 中 MUST NOT 再出现 `uptime` 策略。

#### Scenario: 读取默认策略
- **WHEN** 路由从未配置过策略
- **THEN** 返回 `strategy = "round_robin"`、`config = {}`、`resolved` 为默认权重

#### Scenario: 保存并即时生效
- **WHEN** PUT `{ "strategy": "latency" }`
- **THEN** 落库成功、数据面按新策略选号，MUST NOT 返回 `restart_required`

#### Scenario: 保存吞吐策略
- **WHEN** PUT `{ "strategy": "throughput" }`
- **THEN** 返回 `resolved.weights = { uptime: 0.1, latency: 0, throughput: 0.9 }`

#### Scenario: 指标快照
- **WHEN** 某 Provider 已有样本
- **THEN** 返回其 `ttfb_ms`、`uptime_pct`、`samples`、`stale=false` 与 `score`

#### Scenario: 指标快照含 token 速度
- **WHEN** 某 Provider 已有吞吐样本
- **THEN** 返回其 `tokens_per_sec`（`null` 表示无样本或采样条件不满足）

#### Scenario: 存量 uptime 已迁移
- **WHEN** 数据库中该路由曾为 `uptime`
- **THEN** GET 返回 `strategy = "auto"`，权重为 `auto` 的缺省值

#### Scenario: 非 owner
- **WHEN** 非 owner 请求他人路由的选号配置
- **THEN** 返回 404

### Requirement: 选号配置校验

`PUT /api/routes/{id}/selection-strategy` MUST 校验：

| 字段 | 规则 |
|---|---|
| `strategy` | `round_robin` / `last_used` / `latency` / `throughput` / `auto` |
| `weights.uptime`、`weights.latency`、`weights.throughput` | `[0, 1]` |
| `weights.price` | 必须为 `0`（本版本无价格指标） |
| `thresholds.uptimePenalty` | `[1, 100]` |
| `thresholds.defaultUptime` | `(0, 100]` |
| `thresholds.defaultLatencyMs` | `[1, 600000]` |
| `thresholds.defaultTokensPerSec` | `(0, 100000]` |
| `thresholds.explorationRate` | `[0, 1]` |

违反 MUST 返回 `422`、`code = invalid_request` 与可读说明。`strategy = "uptime"` MUST 被拒绝，说明文案 MUST 指引改用 `auto`。打分策略权重和 `== 0` MUST NOT 被拒绝（数据面回退 `round_robin`，响应 `resolved.fallback = true`）。

#### Scenario: 非法策略
- **WHEN** PUT `{ "strategy": "price" }`
- **THEN** 返回 422 并说明支持的取值

#### Scenario: 已删除的 uptime 策略
- **WHEN** PUT `{ "strategy": "uptime" }`
- **THEN** 返回 422，说明该策略已并入 `auto`

#### Scenario: 非零价格权重
- **WHEN** PUT `weights.price = 0.3`
- **THEN** 返回 422，说明本版本不支持价格权重

#### Scenario: 吞吐权重可配置
- **WHEN** PUT `{ "strategy": "auto", "config": { "weights": { "throughput": 0.4 } } }`
- **THEN** 保存成功，`resolved.weights.throughput = 0.4`

#### Scenario: 非法兜底吞吐
- **WHEN** PUT `thresholds.defaultTokensPerSec = 0`
- **THEN** 返回 422，说明必须大于 0

#### Scenario: 全零权重允许
- **WHEN** PUT `weights.uptime = 0` 且 `weights.latency = 0` 且 `weights.throughput = 0`
- **THEN** 保存成功，`resolved.fallback = true`

## ADDED Requirements

### Requirement: 会话记录查看与清除

系统 SHALL 提供会话最后使用记录的只读查看与清除接口（资源名 `affinity`）：

- `GET /api/routes/{id}/affinity`：返回该路由**未过期**的记录数组，每项含 `model`、`session`、`provider_id`、`expires_at`（Unix 秒），按 `expires_at` 倒序，最多 200 条；
- `DELETE /api/routes/{id}/affinity`：清空该路由的记录；`model` 与 `session` 查询参数同时出现时 MUST 只清除该组合；返回 `{ "removed": <条数> }`。

记录在所有策略下都会被写入，但只有 `last_used` 策略读取（见 gateway-core「会话粘性」）；界面 MAY 用它做诊断。owner 隔离与既有路由接口一致：他人路由 MUST 返回 404。清除 MUST 立即作用于数据面内存，MUST NOT 要求重启，MUST NOT 删除 `runtime_snapshot` 行。

#### Scenario: 查看记录
- **WHEN** 某路由有 3 条未过期记录
- **THEN** GET 返回 3 项，含 `model` / `session` / `provider_id` / `expires_at`

#### Scenario: 会话标识原样返回
- **WHEN** 某记录由请求体 `prompt_cache_key = s-1` 建立
- **THEN** GET 返回的 `session` 为 `s-1`（不掩码，供与客户端请求比对）

#### Scenario: 数据面未装配
- **WHEN** 数据面尚未装配
- **THEN** GET 返回空数组 `[]`，MUST NOT 报错

#### Scenario: 清除单条
- **WHEN** DELETE 带 `model=deepseek-flash&session=s-1`
- **THEN** 仅该 (路由, 模型, 会话) 记录被清除，返回 `{ "removed": 1 }`

#### Scenario: 清除后立即按策略选号
- **WHEN** 清除某会话的记录后该会话再次发请求
- **THEN** 按档位与策略重新选号，MUST NOT 复用被清除的 Provider

#### Scenario: 非 owner
- **WHEN** 非 owner 访问他人路由的记录资源
- **THEN** 返回 404
