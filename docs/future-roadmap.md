# Future Roadmap — 远期与待评估

> 只放「未来可能做什么」，未排期、未承诺。

- 协议通路：对外只保留 OpenAI（chat completions / responses）与 Anthropic Messages 三条入站格式；Gemini 入站与 `provider_native` 已移除（ADR 0013）。未来不符合这三种格式的上游需要通过 **egress 适配器** 做协议转换，而不是新增入站格式。
- 内置 HTTPS：`http-01` / `tls-alpn-01` 已实现（2026-10-19）；通配符 / DNS-01、多实例共享证书、OCSP stapling 未纳入。
- 云托管上游（Bedrock / Vertex / Azure / Foundry）：建议 v2 再纳入。
- 开源许可与商业版分界：待定。
- L2 用量提取（tokens/cost，tee 副本不改线上字节）与 model 维度分析：数据面与模式 B 用量统计已落地（2026-10-15），L2 待定。
- 模型清单瘦身：discover 一次拉入大量模型（如 dashscope 192 个）导致路由模型并集膨胀（`/v1/models` 输出变大、选号候选变宽）——可评估默认只勾热门 / featured 或分页选择（观察项 3，挂账）。
- Anthropic 兼容端点的列模型通路：兼容实现普遍未提供列模型端点（`/v1/models`），discover 族内可用性有限；若将来出现「从模型别名 / 文档导入」等替代通路可再评估（ADR 0010 已否决失败后多路径试探）。
- Bedrock Converse 与非 Claude 模型：`/model/{id}/converse` 是独立内容格式 + 新入站形状，本轮只做 Anthropic `invoke`；需要 Llama / Nova / Mistral 等模型时另立 change（新增 `api_format` 与 Converse 形状入站）。
- Vertex 原生 Gemini 入站路径：**已随 Gemini 入站移除**（ADR 0013）。入站路径必须固定且不含凭据级参数；Vertex Anthropic 仍通过 `/v1/messages` + 路径模板接入。
- Service Account 的健康探针：`gcp_accounts` 已能换取 OAuth2 token，但探针路径仍走静态 header，SA 凭据被整条跳过（`engine.rs` tick 循环）；要做需把 token 取用接到探针并处理过期与失败分类（ADR 0011 决定本轮不做）。
- Vertex 的列模型通路：`google-vertex-anthropic` 的模型清单端点与 `{base}/models` 不同族，且 Service Account 不支持 discover；要做需独立 change（ADR 0011 决定本轮不做，界面按「该凭据类型不支持」显式禁用）。
- 测试墙钟竞态清理：`crates/solidway_core/tests/probes.rs:167`（300ms 固定等待）与 `solidway_api` 测试（affinity / heavy_lifecycle / incidents / persistence / probe / restart / usage）仍有固定 sleep 等待，按 `docs/system-patterns.md`「测试等待模式」约定逐步改造为条件等待（probe-body-timeout-deflake 只处理 `tests/health.rs`，范围外挂账）。
- desktop-app spec 过时表述清理：「健康监控与状态」requirement 残留「探测端口 MUST 从 `<data_root>/config.toml` 的 `[server] port` 读取」，与「桌面壳发现端口」（MUST NOT 读取 config.toml）及代码现状矛盾（mac-desktop-packaging 评审 r1 A1 发现，系既有腐烂）——另立小型 spec 修订 change 清理，可与「交付面统一入 spec」议题（mac-desktop-packaging design D7 备选方案）合并考虑。
- mac 装机冒烟验收：`scripts/package-mac.sh` 产出的 `.app` 经 ditto 装入 `/Applications` 后的手工冒烟（启动 → 「运行中」→ 控制台可达，端口以 `solidway: listening addr=` 事件为准 → 退出后 `solidway` 子进程停止）待验收环境执行（mac-desktop-packaging 任务 5.2，方案 B 后唯一外部验收项）。
- 延迟直方图口径：失败请求的 `ttfb_ms` 记总耗时（字段注释已固定口径为「失败路径为总耗时」），管理面延迟直方图把失败请求的总耗时当 TTFB 计入——统计侧过滤 / 拆字段待评估（review-lowpriority-fixbatch 按裁决只固定注释，R1-F-12③）。
- 「换号全败归因最后尝试者」：现按 spec「无承接时为空」传空 `provider_id`（R1-F-2）；若需要把全败请求归因到最后尝试的 Provider 以利诊断，需另立 change 修订 management-api 归因口径并连带聚合桶语义。 另：请求级错误（非 marked，如上游 400）的 `Access.provider_id` 仍传该 Provider（既有行为，本批 delta 未覆盖）；若统一「无承接时为空」口径需一并评估（review-lowpriority-fixbatch r1 观察项）。
- 帮助内容改 Markdown 承载：当前帮助正文在 `web/src/locales/help/<lang>.json`（随构建打包，严格走 i18n）。若出现「运维自行改文档、不发版」的需求，可评估 Markdown 文件 + 前端渲染（需引入 `react-markdown` 与 dist 打包策略、以及锚点/检索的等价实现）。见 `openspec/changes/2026-10-26-console-onboarding-help/design.md`。
- 引导进度跨设备：当前「已跳过 / 已完成」只存本机 `localStorage`（按用户隔离）。若多设备用户反馈反复触发，再评估服务端 `users.onboarded_at`（需迁移 + 管理 API 契约 + spec 变更）。
- 桌面壳打开体验两处范围外观察（tray-menu-dock-reopen）：服务未就绪（未收到 `listening`）时点 Dock / 托盘沿用现状提示（「尚未获知监听地址」，随就绪自愈），是否改为「启动中」占位待观察；非 macOS 无 Dock 等价重开入口，隐藏启动后只能从托盘「以管理员自动登录」进入是否够用待反馈。
