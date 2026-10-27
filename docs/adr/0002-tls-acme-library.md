# 内置 ACME 客户端选型：`instant-acme` + `tokio-rustls`

状态：proposed（2026-10-19，用户裁决选用该组合）

## 背景

`2026-10-18-builtin-tls-acme` 规定：无反向代理时，服务按 `access.public_origins` 的域名自动申请、续期并热加载证书（Let's Encrypt / 任意 ACME），桌面模式可用，不依赖外部工具。该 spec 只约束了行为——challenge 类型、30 天续期窗口、证书缓存位置与权限、热替换、状态可观测——没有指定 ACME 客户端库。

需要补一个库选型决策，因为：

- 候选库在**谁持有状态**上根本不同：一个把调度与状态封装进库，一个只提供 ACME 协议原语；
- 本 change 的 spec 明确要求可观测状态（`cert.state` = `ok` / `waiting` / `paused`、`last_error`、`failed_attempts_24h`）与精确的退避语义（5 分钟起、1 小时上限、24 小时 5 次后暂停）；
- 引入 TLS 栈是新依赖（当前 `rustls` 只是 `reqwest` 的间接依赖），按项目规范需要显式决策。

## 决策

1. TLS 终止：`rustls`（显式直接依赖）+ `tokio-rustls`（`TlsAcceptor`），ALPN `h2` / `http/1.1`。
2. ACME 客户端：`instant-acme`，只负责账户、订单、挑战、finalize、证书链下载等协议原语；**续期调度、退避、暂停、缓存命名、热替换全部由本仓库实现**（`tls/scheduler.rs` / `tls/store.rs` / `tls/resolver.rs`）。
3. `tls-alpn-01` 的临时挑战证书用 `rcgen` 生成（含 `id-pe-acmeIdentifier` 关键扩展），仅为该 challenge 引入。
4. HTTP-01 的 token 存进程内共享表，由 `http_port` 的 axum router 在最高优先级路径应答，不额外起监听。

## 被否方案

- **`rustls-acme`**：与 hyper / axum 集成最省事，自带续期调度与 `ResolvesServerCert`。否决理由是状态归库所有——`last_error` / 退避曲线 / `paused` 只能从库内私有状态反推或重做，且它强绑定 hyper 与 `tokio-rustls` 版本，升级 axum 时被连带拖住。我们的 spec 已经把证书生命周期当成可观测对象，把控制权交出去等于用映射层换短期代码量。
- **`acme-lib`**：阻塞式（`ureq`），与 tokio 服务模型冲突。
- **外部 `certbot` / `lego`**：与「内置、桌面模式可用、不依赖外部工具」的规格直接冲突；只能作为 standalone 反代部署的文档建议。
- **完全自研 ACME 客户端**：ACME（RFC 8555）的 JWS 签名、账户复用、订单状态机、重试语义不值得自研，`instant-acme` 已把这些原语做完整。
- **不启用内置 TLS，仅支持反代**：否决于 spec 阶段（桌面部署没有反代）。

## 后果

- 需要自写约 200–300 行：续期调度器、`ResolvesServerCert` 实现（`ArcSwap` 热替换）、缓存摘要命名与回退清理、HTTP-01 token 表。
- 状态语义由本仓库定义，`GET /api/tls/status` 的 `state` / `last_error` / `failed_attempts_24h` 直接来自调度器，无映射损耗。
- 依赖面增加 `rustls` / `tokio-rustls` / `instant-acme` / `rcgen` 四个直接依赖；`rcgen` 仅在启用 `tls-alpn-01` 的代码路径使用。
- `paused` 的解除只有两条路：人工 `POST /api/tls/retry` 或进程重启；不提供「无限自动重试」开关，避免静默撞 ACME 的 failed validation 速率限制。
- 若将来出现需求要「多实例共享证书」，本决策不覆盖（spec 明确单实例约束）；届时改由外部证书分发承担。
