# ADR 0010 — discover 的家族感知地址构造与列模型形状解析

## 状态

已接受（2026-10-21）；Gemini 分支已被 `docs/adr/0013-drop-gemini-inbound.md` 取代（2026-10-23），OpenAI / Anthropic 部分仍有效

> 本 ADR 把 ADR 0008 的版本段归一化**扩展应用**到模型发现（discover）的请求地址构造；ADR 0008 关于「归一化只作用于转发目标、不回写 base_url」的裁决不变，本 ADR 只新增一处使用场景（管理面 discover），并补充列模型形状与失败语义的裁决。

## 背景

discover（`POST /api/models/discover` 及创建时自动拉取共用的 `discover_models_with`）无条件 `GET {base_url}/models` 并只按 OpenAI list 形状（`data[].id`）解析，而列模型端点与响应形状按协议族分化（H-6 实测，`WarRoom/SolidWay/实测发现-2026-10-20.md`）：

| 族 | 列模型事实 | 现状结果 |
|---|---|---|
| anthropic | 兼容实现普遍无列模型端点：阿里 `/apps/anthropic/v1/models` 与 `/apps/anthropic/models` 均 404；官方是 `GET /v1/models` | 必失败（toast 失败，无明确原因） |
| gemini | 真实响应是 Gemini 形状 `{"models":[{"name":"models/<id>",…}]}` | 请求通、解析空（报「未能获取模型」） |
| openai | `GET {base}/models` + `data[].id` | 正常 |

另有 ADR 0008 引入的不一致：转发已对缺版本段的 anthropic / gemini base 自动补 `/v1` / `/v1beta`，discover 仍裸拼 `{base}/models`——用户按 ADR 0008 省略版本段后，discover 反而 404。

## 决策

1. **地址构造家族感知**：目标 = `{normalize(base_url)}/models`，`normalize` 复用 ADR 0008 判定（path 含 `v1` / `v1beta` / `v1alpha` 段则不补，段名精确比对），按家族补 `/v1`（anthropic）/ `/v1beta`（gemini）/ 不补（OpenAI）；只作用于请求构造，不回写存储值。
2. **家族按优先级取一**：`chat_completions` / `responses` > `gemini_generate_content` > `anthropic_messages`；一条凭据只发一次请求。
3. **形状解析双形状宽容**：接受 OpenAI / Anthropic list（`data[].id`）与 Gemini list（`models[].name` 剥 `models/` 前缀），任一命中即成功；均不命中 = 形状不符。`display_name` / `displayName` / `baseModelId` 不参与映射（保持 `{name: id, upstream: id}` 同名语义）。
4. **失败二分**：`404` / `405` / `501` 或形状不符 → `error_code = unsupported_model_list` +「上游不支持模型清单拉取，请手动添加模型」；其余失败（不可达、超时、`401` / `403`、其他非 2xx）给可读 `error`、不标「不支持」。
5. **i18n 走稳定错误码**：`DiscoverModelsResult` 与 `autodiscover` 结果携带 `error_code`，界面按 `errors.<code>` 翻译，回退服务端文案。

## 被否方案

- **严格按分支限定形状**（anthropic 分支只认 Anthropic 形状）：兼容实现返回形状不一，严格拒绝会制造新的「必失败」；「能否取出模型 id」是唯一判据。
- **anthropic 失败后回退试探 `{base}/models` 等路径**：隐藏多路试探放大故障流量、拉长失败反馈；失败即明确提示更可排查（H-5 立场）。
- **所有非 2xx 一律标「不支持」**：`401` / `403` 是凭据问题，标「不支持」会误导用户放弃凭据排查；「不支持」只给证据确凿的失败（404 / 405 / 501 / 形状不符）。
- **界面按表单格式硬编码提示（不加 `error_code`）**：服务端失败原因（404 vs 401 vs 超时）界面不可知，硬编码会误报；稳定错误码是仓库既有 i18n 机制（`errors.<code>`）。
- **列表返回 `display_name` 作为客户端名**：与「获取模型」两列同名语义冲突，且别名应由用户决定。

## 后果

### 正面

- anthropic 族失败从「莫名失败」变为明确、可行动的提示（「请手动添加模型」），创建自动拉取的 toast 同步修正（不再误导「可稍后手动获取」）；
- gemini 族解析与地址构造修复（真实 Gemini 形状可解析、缺版本段 base 可用），与转发口径一致；
- OpenAI 族与所有「base 已含版本段」的既有形态**零行为变化**，回归测试保持绿。

### 负面 / 风险

- 多族混声明只试一个分支（优先 OpenAI）：若某代理只在 anthropic 路径提供列模型，混声明凭据会走 OpenAI 分支并失败——属罕见形态，用户可拆分凭据；
- Anthropic 兼容端点普遍无列模型端点的事实不变：多数 anthropic provider 仍需手动添加模型（本 change 只把失败变得可理解）；
- 分页不支持（`nextPageToken` / `has_more`）：清单截断在第一页，与现状一致；
- 请求契约新增 `api_formats` 字段：旧调用方不传则按预设 / OpenAI 分支推断，向后兼容。
