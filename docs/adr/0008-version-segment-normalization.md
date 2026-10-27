# ADR 0008 — anthropic / gemini 上游地址的版本段归一化

## 状态

已接受（2026-10-20）；Gemini 部分已被 `docs/adr/0013-drop-gemini-inbound.md` 取代（2026-10-23），Anthropic 部分仍有效

> 本 ADR 部分取代 ADR 0003「目标地址 = `base_url` + 入站 path 去掉家族根」中的两条裁决：「网关 MUST NOT 再补版本段」与「缺版本段只告警不改写」。ADR 0003 其余内容（按家族枚举端点、家族根剥离一次、`model` 来源）继续有效。

## 背景

ADR 0003 把 `base_url` 定为「上游 API 根（含版本段）」，网关不补版本段，缺了只在保存时告警。手工实测（2026-10-20，`WarRoom/SolidWay/实测发现-2026-10-20.md` H-5）证明这个约定与厂商公布的 base_url 惯例**系统性不匹配**：

| 厂商 | 公布的 anthropic base_url | 真实路径 |
|---|---|---|
| 百炼 token plan | `…/apps/anthropic` | `…/apps/anthropic/v1/messages` |
| 智谱 coding plan | `https://open.bigmodel.cn/api/anthropic` | `…/api/anthropic/v1/messages` |
| DeepSeek | `https://api.deepseek.com/anthropic` | `…/anthropic/v1/messages` |
| Kimi | `https://api.kimi.com/coding/` | `…/coding/v1/messages` |

即 anthropic / gemini 两族的「公布值 ≠ 可拼接值」：客户端（Anthropic SDK、Claude Code、google-genai SDK）都自行拼版本段，网关却要求用户手动补 `/v1`，照抄厂商文档配置必然上游 404，且错误原样透传、极难排查（实测全程踩过）。对照 OpenAI 兼容族公布的 base 都含 `/v1`，与 `{base}/chat/completions` 构造恰好吻合，不需要动。

保存时告警（ADR 0003 的兜底）救不了：它只提示「可能缺版本段」，用户仍不知道该补什么、补在哪，而且对所有 coding-plan 配置都误报。

## 决策

1. **转发目标构造按家族补版本段**：`anthropic_messages` 缺则补 `/v1`，`gemini_generate_content` 缺则补 `/v1beta`；
2. **已含版本段则不补**：`base_url` path 中存在名为 `v1` / `v1beta` / `v1alpha` 的 path 段（**段名精确比对**，含路径尾部；`v1beta1` / `v1alpha2` 等相邻形态不算命中）即视为已含，原样使用——宁可不补，避免 `/v1/v1`；
3. **OpenAI 两族不补**：`chat_completions` / `responses` 的 `base_url` 仍是完整 API 根（任意兼容前缀合法，如 `/api/paas/v4`、`/v1/openai`）；
4. **归一化只作用于转发目标**：不回写存储的 `base_url`，用户输入原样落库；
5. **移除「缺版本段」保存告警**：其前提（缺了就 404）已不成立，保留只会对每条 coding-plan 配置误报；「不自动改写 base_url」语义保留。

## 被否方案

- **维持现状 + 告警**（ADR 0003 原案）：实测证明用户照抄厂商文档仍会 404，告警文案解决不了「补什么、补在哪」，且误报面大。
- **保存时改写 `base_url`（补上版本段再落库）**：与既有约束「MUST NOT 自动改写用户输入」冲突，且用户再次编辑时看到的是自己没填过的值——正是 H-3 同类的「静默改数据」问题。
- **per-provider 版本段开关 / 显式字段**：为小众形态引入表单复杂度，YAGNI；且「无版本段代理」目前无真实客户需求，等出现再评估逃生舱。
- **对所有家族统一补 `/v1`**：破坏 OpenAI 兼容族的任意前缀语义（`https://open.bigmodel.cn/api/paas/v4` 会拼出 `/api/paas/v4/v1/chat/completions`），回归面大。
- **失败后自动回退重试（先按归一化、404 再试未归一化）**：转发目标不确定、放大故障流量、与「换号仅限首个字节之前」的确定性模型冲突。

## 后果

### 正面

- 照抄厂商公布的 anthropic / gemini base_url 即可用，H-5 的 404 排查路径消失；
- 与 Anthropic SDK / Claude Code / google-genai SDK 的拼接惯例一致，控制台示例与真实客户端行为对齐；
- 保存告警收敛，不再对合法配置误报。

### 负面 / 风险

- **「无版本段代理」不再可达**：企业网关若把 Gemini / Anthropic 暴露在干净路径（不带版本段）上，归一化会拼出多余版本段，且暂无 per-provider 逃生舱（用户可在 `base_url` 里补一个版本段来命中「已含不补」分支，但目标路径仍带该段，不等价）；同理，`v1beta1` / `v1alpha2` 等相邻版本形态因段名精确比对不算「已含版本段」，会被再补 `/v1beta`（`/v1`）——同一类边缘取舍；
- 预设 `default_base_url` 带不带版本段都成立，但语义变为「可省略」，覆盖表维护者不必再纠结；
- 排查口径变化：日志 / 排障文档中的「目标地址」不再等于 `base_url + 端点路径`，需按归一化后地址对照。
