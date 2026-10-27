# 预设目录的兼容性分类与说明元数据

状态：accepted（2026-10-23，用户裁决）

## 背景

预设目录由 models.dev 机械生成。旧生成器只映射了鉴权方式，没有映射入站格式：未显式声明的条目一律默认成 `chat_completions`。这导致两类问题：

1. **不可兼容条目仍出现**：`@ai-sdk/google`、`@ai-sdk/google-vertex`、`@ai-sdk/amazon-bedrock`、`gitlab-ai-provider` 等原生协议无法用三种入站格式直连，却仍被写入目录。
2. **Anthropic 兼容条目被误标**：MiniMax、Thinking Machines、FreeModel、Subconscious 使用 `@ai-sdk/anthropic`，真实端点是 Anthropic Messages，却被标成 `chat_completions`。

同时，目录只有 `name` / `key` / `doc`，用户无法在保存前知道某个预设是普通按量 API、编程套餐、Token 套餐还是订阅制服务。面向个人的 token plan（Z.AI Coding Plan、Kimi For Coding、MiniMax Token Plan、Xiaomi Token Plan 等）与普通 API 混在一起，误配成本高。

## 决策

1. **生成期显式分类协议**：生成器按 npm 包推导入站格式：
   - `@ai-sdk/anthropic` / `@ai-sdk/google-vertex/anthropic` → `anthropic_messages`；
   - OpenAI 兼容 npm → `chat_completions`；
   - Google 原生 / Vertex 原生 / Bedrock 原生 / GitLab Duo 原生 → 不兼容，直接剔除。
   覆盖表可显式覆盖。无法映射的条目 MUST NOT 写入目录，不再以 `chat_completions` 兜底。
2. **运行期 fail-closed**：`presets::all()` 过滤 `default_api_formats` 为空的条目；`find()` 不命中。
3. **新增说明元数据**：
   - `kind`：`api` / `coding_plan` / `token_plan` / `subscription` / `local` / `enterprise` / `gateway`；
   - `summary`：可选 i18n key（一句话说明）；
   - `note`：可选 i18n key（特殊鉴权 / 协议 / 套餐限制）；
   - `doc`：已有文档链接。
4. **文案走 i18n**：中英文在 `zh-CN` / `en-US` 维护，不把说明写进 `providers.json`。
5. **Token Plan 一等类型**：`coding_plan` / `token_plan` 使用专门提示（需订阅、套餐 Key、普通 Key 通常不可用），并在保存前展示。
6. **控制台展示**：预设选择器展示类型徽章 + 一句话说明；选中面板展示完整说明、note 与文档链接；搜索匹配说明文案。

## 被否方案

- **只在前端硬编码某几个 provider 的提示**：目录新增 token plan 时会再次遗漏；元数据应随条目走。
- **把中英文说明直接写进 `providers.json`**：目录与界面语言耦合，新增语言需改数据文件。
- **为 200+ 预设逐条写长文案**：维护成本高、重复多；类型提示 + 少量特殊 note 足够覆盖歧义。
- **保留不可兼容条目并灰显**：用户仍会尝试配置；直接移除符合“只支持三种协议”的产品边界。
- **运行期按 `base_url` 猜协议**：地址形状不可靠，且让转发行为依赖字符串匹配。

## 后果

- 目录条目减少；GitLab、Bedrock 原生、Google 原生等不再出现。
- MiniMax、Thinking Machines、FreeModel、Subconscious 修正为 `anthropic_messages`。
- 目录新增 `kind` / `summary` / `note` 字段；旧条目仍可解析，但空格式条目会被运行期过滤。
- 未来接入新协议仍需 egress 适配器；`kind` 不是协议维度，不能用于绕过 `api_formats` 的三种格式约束。
