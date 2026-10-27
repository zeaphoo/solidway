# ADR 0011 — Provider 端点模板化与预设零自由度

## 状态

已接受（2026-10-22）；Gemini 原生预设与 `provider_native` 相关部分已被 `docs/adr/0013-drop-gemini-inbound.md` 取代（2026-10-23）

## 背景

预设只提供"默认值"，端点仍可被自由编辑；控制台靠"`base_url` 是否与预设默认值**字面相等**"反推是否偏离，偏离就把 `provider` 自动改成 `<key>-custom`。该机制产生四类故障（详见 `openspec/changes/preset-endpoint-template-rework/proposal.md`）：

1. 占位符型默认值（`azure` 的 `{resource}`、`google-vertex` 的 `{location}` 等 9 条）必须替换真实值才可用，替换后字符串永不相等 → **必然误判**，编辑态只改模型清单也会静默改名。
2. 改名后 `presets::find` 落空：`protocol` 被写成 `openai-chat-completions`、预设兜底失效、Vertex 参数区块消失。
3. `credential_kind` 与 `auth_scheme` 两个可选字段讲同一件事，后端静默覆盖一半、报错一半。
4. Service Account 凭据的发现/探针静默失效，界面把"未探测"显示成"健康"。

## 决策

1. **端点由模板唯一决定**：目录字段 `base_url_template` + `base_url_variables`；写入时展开并落库绝对地址。全部 233→235 条预设统一为模板形态，原先端点依部署而异的 10 条改为单变量 `${BASE_URL}`。否决"运行期展开"：forward / probe / discover 三处都要各自实现替换，漏一处即把占位符发到网络。
2. **预设条目零自由度**：`provider`（= 预设 key）、`base_url`、`api_formats`、`auth_scheme` 锁定，只剩变量可填；差异用显式「复制为自定义 / 转为自定义」（`<key>-custom`）表达。否决"保留偏离自动改名"：它把"网关端点变了"误读为"身份变了"。
3. **`provider` 即 presets key**，不新增外键列；副本以 `<key>-custom` 命名约定反查（剥离一次、精确匹配、不递归）。**改名 = 解绑**：转发行为不变（见第 4 条），仅失去模板变量表单。
4. **预设供给的值全部冻结落库**：`base_url`（展开）、`api_formats`、`auth_scheme`、`options`（含 `path_template` / `path_params` / `base_url_vars`）。运行期不查目录，目录改版不影响存量。
5. **地址构造显式**：数据面按冻结的 `path_template` 渲染上游路径（`{param}` / `{model}` / `{action}` / `{endpoint}` + 模板 query），未声明模板时按 `{base_url}{家族后缀}`；删除 5 处 `provider` 前缀判断与 `{location}` 运行期替换。`protocol` 字段整体删除——它把"线格式族"与"端点形态"混在一个值域里，而端点形态已由模板表达；正文规则（`rawPredict` 形状）也改由渲染出的 action 决定，不依赖厂商字段。
6. **一个模板匹配原语**：`expand` / `match` 互逆，覆盖"写入一致性校验 / 变量反推 / Google 端点判定（`{prefix}.googleapis.com`）/ 是否仍符合预设"四类判断。否决硬编码域名后缀。
7. **端点形态用多预设表达**：Vertex 拆为 regional/global × Gemini/Anthropic 四条，global 把 `locations/global` 写成模板字面量；否决"一条预设多套模板 / 可选路径段"。
8. **凭据类型由内容判定**：SA JSON → `google_service_account`，其余 `api_key`；请求提示冲突返回 400 而非静默覆盖。SA 仅允许 Google 端点（预设声明或 `{prefix}.googleapis.com` 命中）。
9. **Service Account 的能力边界显式**：不做探针与列模型，`discover` 返回 `unsupported_credential_kind`，健康呈现新增"不可探测"（覆盖 SA / 空模型清单 / 仅 `provider_native`），未探测不得显示为健康。

## 后果

- 目录契约变更（`default_base_url` → `base_url_template`，新增变量/分组字段），需重新生成 `providers.json`。
- 需要同时改控制面写入校验、数据面适配器、控制台表单与列表；本次一次性完成，无迁移与兼容负担（项目未发布）。
- 顺带修复：`azure` / `cloudflare-workers-ai` / `databricks` / `infomaniak` / `neon` / `snowflake-cortex` 六个预设此前"开箱不可用"（`{resource}`、`${...}` 从未被替换）。
- 遗留（见 `docs/future-roadmap.md`）：SA 探针与 Vertex `publisherModels` 列模型未实现。
