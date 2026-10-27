# 数据面引擎独立为纯内存 crate（`solidway_core`）

状态：accepted（2026-10-15，用户裁决）

## 背景

`develop` 分支已实现网关转发能力，但形态是「单 crate + TOML 配置 + 健康直接写 SQLite」：配置来自文件（改动需重启）、健康状态在转发路径内直接落库。这与 `main` 的「DB + 管理 API + Web 控制台」事实源冲突，无法直接合并。

## 决策

1. 新增 crate **`solidway_core`** 作为**纯内存数据面**：不做任何数据库操作、不读取配置文件、不提供管理端点，只提供转发端点与 `/health`。
2. 控制面 **`solidway_api`** 持有 DB，负责**下发配置**（`CoreConfig`）与**上收并持久化事件**（`CoreEvent`）。
3. 寻址改为 **key → route**：`Authorization: Bearer <API Key>` 直接定位路由；转发端点无池段。
4. 通用语言采用 `main` 一套：**路由（route）/ provider 凭据（provider key）/ API Key**；弃用 `develop` 的租户 / 池 / provider / 轮换组。
5. 运行期参数归属：网络与超时留 `config.toml`（部署级，重启生效）；冷却与 `affinity_ttl` 入 DB settings（仅 admin，可即时下发）。

## 被否方案

- **直接合并 develop 的单 crate**：配置来自 TOML、健康直接写库，与管理面 DB 事实源冲突，且把持久化耦合进转发路径。
- **core 自己读 DB**：破坏「纯内存数据面」边界，转发正确性依赖 DB 可用性。
- **core 读配置文件**：同上，且与「即时下发」冲突。
- **保留 develop 的路径池寻址**：与 `main` 的 API Key 绑定路由模型冲突。

## 后果

- core 可独立单测、可替换、无持久化副作用；`cargo tree` 可验证其依赖不含 `sqlx`。
- 配置变更**即时生效**，不存在「热加载」概念；重启时控制面把持久化的健康状态（含冷却截止时刻）**全量回灌** core。
- 代价：`CoreConfig` / `CoreEvent` 成为需要维护的接口契约；`develop` 的 4 份 spec 需按新术语与寻址重写。
- `develop` 的 ADR（0001–0006）记录其自身演进，本仓库不并入；其中「保真白名单」与「持久化 + 探针」的口径由本仓库规格重新表述。
