# admin-console Specification

## Purpose
TBD - created by archiving change add-admin-console. Update Purpose after archive.

## Requirements

### Requirement: 控制台交付与托管

控制台 MUST 由 `web/` 构建为静态产物，并由 `solidway` 在同一进程内托管，使部署保持单个二进制。

`solidway` 托管静态资源时 MUST 对未知路径回退到 `index.html`（SPA fallback），且 MUST NOT 影响 `/api` 前缀的路由。

#### Scenario: 通过单二进制访问控制台
- **WHEN** 浏览器访问 `solidway` 监听的根路径
- **THEN** 返回控制台的 `index.html` 与静态资源

#### Scenario: 前端深链接刷新
- **WHEN** 浏览器直接访问任意前端路由（如 `/routes/3`）并刷新
- **THEN** 返回 `index.html`（而非 404），由前端路由接管

#### Scenario: /api 不受影响
- **WHEN** 请求 `/api/routes`
- **THEN** 由 API 处理，静态托管 MUST NOT 拦截

### Requirement: 登录与认证态

控制台 SHALL 提供登录页；未认证用户 MUST 被引导至登录页，SHALL NOT 能访问受保护页面。

控制台 MUST NOT 在前端存储或读取会话令牌；会话完全由 HttpOnly Cookie 承载，请求 MUST 携带凭据。

#### Scenario: 未登录访问受保护页面
- **WHEN** 未认证用户访问 `/routes`
- **THEN** 前端重定向至 `/login`

#### Scenario: 登录成功
- **WHEN** 用户在登录页提交正确凭据
- **THEN** 进入控制台主界面，并展示当前用户

#### Scenario: 会话失效
- **WHEN** 任一 API 返回 `401`
- **THEN** 前端清除本地认证态并重定向至 `/login`

#### Scenario: 退出登录
- **WHEN** 用户触发退出
- **THEN** 调用登出接口并返回登录页

### Requirement: 路由管理界面

控制台 SHALL 在**侧栏**中列出当前用户拥有的未归档路由并作为导航入口，同时在路由总览页列出**当前用户拥有的全部未归档**路由。

侧栏 MUST 最多直接列出 9 条路由；存在更多路由时 MUST 追加一行「查看全部（N）」入口（N 为当前用户路由总数），点击后进入路由总览页。

路由总览页（`/routes`）MUST 列出当前用户拥有的未归档路由，MUST 提供按**名称或描述**过滤的搜索框；点击某行 MUST 进入该路由详情。页面 MUST 提供“显示已归档”切换；开启后归档路由 MUST 以“已归档”标记展示。

路由详情页 MUST 仅对路由 owner 可见；其他用户（包括 admin）MUST NOT 看到该路由，直接访问 MUST 返回不可用页面或重定向。归档路由详情 MUST 只读，MUST NOT 提供恢复入口。

创建与重命名通过表单，启停通过开关；删除按钮 MUST 改为「归档」，归档 MUST 二次确认并留在详情页展示归档状态。不同用户 MAY 使用相同的路由名称。

创建路由成功后，控制台 MUST 直接进入该路由详情页 `/routes/<id>`，MUST NOT 停留在路由总览页。

路由详情页在 `GET /api/routes/<id>` 返回 `404`（路由不存在或不属于当前用户）时 MUST 重定向到路由总览页 `/routes`，SHALL NOT 渲染该路由内容或错误文案。非 `404` 的加载失败 MUST 保持可读错误提示。

路由详情页 MUST 仅在路由本身加载成功后请求其子资源（Provider 列表、API Key、选号策略、用量、故障横幅）；路由不存在时 MUST NOT 对该路由并发发起子资源请求。

路由总览页空态 MUST 同时提供创建入口与「第一次用？看快速开始」入口，后者 MUST 进入 `/welcome`。

路由详情页 MUST 识别 `?new=provider` 查询参数并自动打开「新增 Provider」对话框，随后 MUST 清除该参数。

#### Scenario: 列表
- **WHEN** 已认证用户进入控制台
- **THEN** 侧栏只列出该用户拥有的至多 9 条未归档路由，当前路由高亮

#### Scenario: 不同用户同名路由
- **WHEN** 用户 A 与用户 B 都创建了名为 `prod` 的路由
- **THEN** 两人各自只看到自己的 `prod`，互不影响

#### Scenario: admin 只看到自己的路由
- **WHEN** admin 进入控制台
- **THEN** 侧栏与路由总览只展示 admin 自己拥有的路由，不展示其他用户的路由

#### Scenario: 非 owner 访问路由详情
- **WHEN** 用户（包括 admin）直接访问他人路由的 `/routes/<id>`
- **THEN** 页面重定向到路由总览页，不展示路由内容，也不请求该路由的子资源

#### Scenario: 失效路由链接
- **WHEN** 用户打开一个已不存在的路由 `/routes/<id>`（如数据库重建前的旧链接）
- **THEN** 详情页重定向回 `/routes`，且不展示 404 错误文案

#### Scenario: 非 404 加载失败
- **WHEN** 路由详情加载返回 `5xx`
- **THEN** 详情页展示可读错误提示，不重定向

#### Scenario: 侧栏条数超出
- **WHEN** 当前用户拥有的路由为 10 条或更多
- **THEN** 侧栏只列出前 9 条，并显示「查看全部（N）」，N 等于该用户路由总数

#### Scenario: 侧栏条数未超出
- **WHEN** 当前用户拥有的路由不超过 9 条
- **THEN** 侧栏列出全部路由，不显示「查看全部」入口

#### Scenario: 进入路由总览
- **WHEN** 用户点击侧栏「路由」或「查看全部（N）」
- **THEN** 主内容区展示当前用户拥有的全部未归档路由表格（名称、描述、状态、更新时间），MUST NOT 自动跳转到某一条路由详情

#### Scenario: 按名称搜索
- **WHEN** 用户在总览页输入某个路由名的片段
- **THEN** 表格只保留名称匹配的路由

#### Scenario: 按描述搜索
- **WHEN** 用户输入的内容只出现在某些路由的 `description` 中
- **THEN** 表格保留这些路由

#### Scenario: 搜索无匹配
- **WHEN** 关键字不匹配任何路由的名称或描述
- **THEN** 表格内提示「没有匹配的路由」，侧栏与其它导航不受影响

#### Scenario: 进入详情
- **WHEN** 用户点击总览页的任意一行
- **THEN** 进入 `/routes/<id>` 并展示该路由详情

#### Scenario: 空态
- **WHEN** 当前用户尚无任何路由
- **THEN** 主内容区展示引导创建的空态，侧栏提供创建入口，并展示通往快速开始的链接

#### Scenario: 启停路由
- **WHEN** 用户在路由详情中切换启用开关
- **THEN** 立即提交该路由的新状态，并按新状态展示

#### Scenario: 创建
- **WHEN** 用户提交新路由表单
- **THEN** 创建成功、出现在自己的侧栏与总览页，并直接进入该路由详情页

#### Scenario: 名称冲突
- **WHEN** 创建或重命名的名称与当前用户已有路由重复（API 返回 `409`）
- **THEN** 表单内联提示名称冲突，SHALL NOT 以通用错误掩盖

#### Scenario: 删除
- **WHEN** 用户在自己的路由详情中触发删除
- **THEN** 需二次确认，确认后该路由被归档并留在详情页展示“已归档（不可恢复）”状态

#### Scenario: 显示已归档
- **WHEN** 用户在路由总览开启“显示已归档”
- **THEN** 归档路由出现在列表中，并带“已归档”标记

#### Scenario: 直达新增 Provider
- **WHEN** 用户打开 `/routes/<id>?new=provider`
- **THEN** 页面加载后自动打开「新增 Provider」对话框，且地址栏中的 `new=provider` 随即被移除

### Requirement: Provider 管理界面

控制台 SHALL 在路由详情中提供 Provider 的列表、新增、编辑、启停、排序与删除。

列表 MUST 展示每个 Provider 的**健康状态**（正常 / 冷却中 / 已退役 / 不可探测）；存在标记时 MUST 展示原因，且对 `admin` 提供「解除标记」入口。

新增 / 编辑表单中的 Provider 预设 MUST 从内置预设目录（`GET /api/providers`）选择；选择后 MUST 带出只读的端点（由模板与变量展开）、只读的 API 格式与鉴权方式，以及可填的端点变量。表单 MUST NOT 允许在预设模式下覆盖 `base_url`、`api_formats`、`auth_scheme`；需要差异 MUST 使用「复制为自定义」或「转为自定义」。表单 MUST 提供模型清单（`models`）编辑与「获取模型」入口。

「获取模型」MUST 使用表单中的 `base_url` 与鉴权方式发起：新建态使用表单填写的 api_key；编辑态 api_key 留空时 MUST 使用该 Provider **已保存**的凭据（与「留空表示不修改」语义一致），MUST NOT 要求用户重新粘贴凭据。仅当既无表单 api_key 又无已保存凭据（新建未保存）时才要求输入。

「获取模型」入口 MUST 对支持清单拉取的协议族保留（含 anthropic 族）；对 Service Account 凭据 MUST 禁用并说明原因（服务端不支持该能力）；获取失败时界面 MUST 按响应 `error_code` 显示明确提示——`unsupported_model_list` 显示「上游不支持模型清单拉取，请手动添加模型」（`en-US` 用等义文案），`unsupported_credential_kind` 显示「该凭据类型不支持模型清单拉取」；其余错误按 `errors.<code>` i18n 回退到 `error` 文案；提示后 MUST 保留手动添加模型的路径。

**创建保存时的模型清单自动获取**：新建 Provider 且模型清单为空时，服务端会在创建时自动拉取一次模型清单（同名映射，见 `management-api`「provider 凭据的模型清单」），创建响应携带结果 `autodiscover`；界面 MUST 据此反馈：

- `ok`：MUST NOT 额外提示，清单随创建结果直接体现在 Provider 列表（同名映射按既有口径展示）；
- `failed`：MUST 以 toast 提示失败——`autodiscover.error_code = unsupported_model_list` 时提示「上游不支持模型清单拉取，请手动添加模型」，否则提示「模型自动拉取失败，可稍后手动获取」（`en-US` 用等义文案）；Provider 仍创建成功、清单留空，用户可稍后用「获取模型」补齐；
- `skipped`：MUST 不提示。

保存按钮的等待态 MUST 覆盖自动拉取耗时（服务端有界等待，上限 5 秒），MUST NOT 弹出额外的中间对话框。编辑已有 Provider、或创建时已填写模型清单 MUST NOT 自动拉取。

Provider 列表 MUST 只展示凭据掩码，SHALL NOT 展示或请求明文。列表主标识 MUST 为备注名（`name`），缺省回退到预设显示名或短标识；MUST 以次要信息展示预设 key、端点变量摘要与已声明格式。

#### Scenario: 选择 Provider 预设
- **WHEN** 用户在新增 Provider 表单中选择一个 Provider 预设
- **THEN** 表单自动填充该预设的默认 `base_url` 与协议格式，且可被覆盖

#### Scenario: 提交 Provider
- **WHEN** 用户填写并提交 provider 与 api_key
- **THEN** 该 Provider 被创建并出现在列表中

#### Scenario: 启停 Provider
- **WHEN** 用户切换某个 Provider 的启用开关
- **THEN** 立即提交该 Provider 的新状态，并按新状态展示

#### Scenario: 排序 Provider
- **WHEN** 用户设置某个 Provider 的排序值
- **THEN** 保存后列表按排序值展示

#### Scenario: 展示掩码
- **WHEN** 展示 Provider 列表
- **THEN** 只显示 api_key 掩码（如末 4 位），不提供明文查看

#### Scenario: 删除 Provider
- **WHEN** 用户删除某个 Provider
- **THEN** 需二次确认，确认后从列表移除

#### Scenario: 展示健康状态
- **WHEN** 某个 Provider 处于冷却或退役
- **THEN** 列表对应行展示状态与原因

#### Scenario: admin 解除标记
- **WHEN** admin 点击「解除标记」
- **THEN** 标记被清除并即时反映到界面

#### Scenario: 非 admin 无解除入口
- **WHEN** 非 admin 查看 Provider 列表
- **THEN** 不展示「解除标记」入口

#### Scenario: 获取模型清单
- **WHEN** 用户填写 `base_url` 与 api_key 后点击「获取模型」（新建态）
- **THEN** 表单模型清单被填充；获取失败时给出可读提示且允许手动填写

#### Scenario: 编辑态用已存凭据获取
- **WHEN** 编辑已有 Provider 时 api_key 留空、`base_url` 有值并点击「获取模型」
- **THEN** 用已保存凭据 + 表单 `base_url` 发起获取并填充清单，不提示「请先填写 API Key」

#### Scenario: 新建态缺凭据提示
- **WHEN** 新建 Provider 时 api_key 为空并点击「获取模型」
- **THEN** 提示先填写 API Key，不发起请求

#### Scenario: 保存即拉取成功
- **WHEN** 新建 Provider 且模型清单为空，上游可正常返回模型
- **THEN** 创建成功后 Provider 列表直接展示自动拉取到的模型（同名映射），不出现额外提示

#### Scenario: 保存即拉取失败提示
- **WHEN** 新建 Provider 且模型清单为空，自动拉取失败（含超时），且响应未带 `unsupported_model_list`
- **THEN** 创建成功、清单留空，toast 提示「模型自动拉取失败，可稍后手动获取」，用户可用「获取模型」补齐

#### Scenario: 自动拉取失败提示按错误码
- **WHEN** 新建 anthropic 族 Provider 且自动拉取返回 `error_code = "unsupported_model_list"`
- **THEN** 创建成功、清单留空，toast 提示「上游不支持模型清单拉取，请手动添加模型」（`en-US` 等义文案）

#### Scenario: 获取失败的不支持提示
- **WHEN** 用户点「获取模型」，响应 `error_code = "unsupported_model_list"`
- **THEN** 显示「上游不支持模型清单拉取，请手动添加模型」（`en-US` 等义文案），「获取模型」入口保留且允许手动添加模型

#### Scenario: 已填清单不自动拉取
- **WHEN** 新建 Provider 时已手动填写模型清单并保存，或编辑已有 Provider 保存
- **THEN** 不自动拉取，保存行为与此前一致

#### Scenario: Service Account 禁用获取模型
- **WHEN** 表单识别出凭据为 Service Account
- **THEN** 「获取模型」禁用并提示该凭据类型不支持，不发起请求

#### Scenario: 预设端点只读
- **WHEN** 用户选择 `openai` 预设
- **THEN** Base URL、API 格式与鉴权方式均以只读形式展示，仅提供「复制为自定义」入口

#### Scenario: 不可探测态
- **WHEN** 列表中存在 Service Account 凭据或模型清单为空的凭据
- **THEN** 其健康列显示「不可探测」及原因，不显示「正常」

### Requirement: 深浅色主题

控制台 SHALL 支持浅色、深色与「跟随系统」三种主题模式，并提供常驻的用户切换入口。

首次访问且无持久化选择时 MUST 默认使用「跟随系统」。用户显式选择后 MUST 持久化选择本身（而非固化为当时的浅 / 深）；选择「跟随系统」时，界面 MUST 实时跟随系统偏好（`prefers-color-scheme`）变化。

#### Scenario: 首次访问
- **WHEN** 用户首次访问且未保存过主题
- **THEN** 以「跟随系统」模式展示，并按当前系统偏好应用浅色或深色

#### Scenario: 选择「跟随系统」并实时跟随
- **WHEN** 用户选择「跟随系统」，随后系统主题变化
- **THEN** 界面随之切换，无需刷新

#### Scenario: 持久化「跟随系统」
- **WHEN** 用户选择「跟随系统」后刷新页面
- **THEN** 仍为「跟随系统」模式（而非被固化为当时的浅 / 深）

#### Scenario: 切换并持久化
- **WHEN** 用户显式选择浅色或深色后刷新页面
- **THEN** 保持其上次选择的模式

### Requirement: 中英文国际化

控制台 SHALL 支持中文（`zh-CN`）与英文（`en-US`）两种界面语言，且 MUST NOT 硬编码界面文案。

首次访问时 MUST 依据浏览器语言决定初始语言；用户显式选择后 MUST 持久化。当浏览器语言既非中文也非英文时，MUST 回退到默认语言（中文）。

#### Scenario: 切换语言
- **WHEN** 用户切换界面语言
- **THEN** 界面文案即时切换为所选语言，并在刷新后保持

#### Scenario: 语言回退
- **WHEN** 浏览器语言既非中文也非英文
- **THEN** 以默认语言（中文）展示

#### Scenario: 无硬编码文案
- **WHEN** 检查界面
- **THEN** 所有用户可见文案均来自 i18n 资源

### Requirement: API 契约与错误反馈

控制台 SHALL 统一封装 `/api` 调用，请求 MUST 携带 Cookie 凭据；对失败响应 MUST 解析统一错误体 `{ "error": { "code", "message" } }` 并向用户反馈可读信息。

#### Scenario: 展示后端错误
- **WHEN** API 返回统一错误体
- **THEN** 控制台以当前语言展示对应提示

#### Scenario: 网络或未知错误
- **WHEN** 请求失败且无统一错误体
- **THEN** 控制台展示通用错误提示，SHALL NOT 崩溃或白屏

### Requirement: 控制台外壳

控制台 SHALL 采用「固定侧栏 + 顶部栏 + 主内容区」的布局，侧栏 SHALL NOT 折叠。

控制台 MUST 占满一屏高度，侧栏 MUST 始终充满视口上下空间并保持固定；仅主内容区可滚动，MUST NOT 出现页面级滚动。

侧栏 MUST 承载品牌、**分组导航**与当前用户菜单（含退出）；顶部栏 MUST 承载面包屑、主题 / 语言切换与**帮助入口**。

帮助入口 MUST 至少包含「快速开始」与「帮助中心」两项。顶部栏的帮助入口 SHALL 采用下拉菜单，MUST NOT 为每个帮助项单独占用顶部栏按钮。

#### Scenario: 侧栏承载导航与用户菜单
- **WHEN** 已认证用户进入控制台
- **THEN** 侧栏包含品牌、分组导航与当前用户菜单（含退出登录）

#### Scenario: 仅主内容区滚动
- **WHEN** 页面内容高度超过视口
- **THEN** 侧栏保持充满视口且不随内容滚动，页面级不出现滚动条，仅主内容区滚动

#### Scenario: 顶部栏承载上下文与切换
- **WHEN** 已认证用户浏览某路由
- **THEN** 顶部栏显示「路由 / <当前路由名>」面包屑，并提供主题与语言切换

#### Scenario: 顶部栏帮助入口
- **WHEN** 已认证用户点击顶部栏帮助入口
- **THEN** 展开下拉菜单，包含「快速开始」「帮助中心」与「接入示例」，选择后进入对应页面或以对话框展示接入示例

#### Scenario: 分组导航
- **WHEN** 已认证用户进入控制台
- **THEN** 侧栏按分组展示入口：管理（路由、API Keys）、统计（使用和统计）、设置（个人，以及仅 admin 可见的系统与用户管理）、帮助（快速开始、帮助中心）

#### Scenario: 帮助页面包屑
- **WHEN** 用户位于 `/help` 或 `/welcome`
- **THEN** 顶部栏面包屑分别展示「帮助」与「快速开始」，且不回落到「路由」

#### Scenario: admin 可见全局统计
- **WHEN** admin 进入控制台
- **THEN** 统计分组额外展示「全局统计」入口

#### Scenario: 非 admin 不可见全局统计
- **WHEN** 非 admin 进入控制台
- **THEN** 统计分组不展示「全局统计」入口

### Requirement: API Keys 界面

控制台 SHALL 提供「API Keys」页面，列出**当前用户路由下**的 API Key 及其所属路由、名称、前缀、掩码与状态。

页面 MUST 支持创建（路由选择只包含当前用户拥有的路由）、改名、启停与删除。创建成功后 MUST 一次性展示明文并提示「仅显示一次」，SHALL NOT 在之后任何界面再展示明文。

创建对话框的路由字段 MUST 按可选路由数渲染：恰好 1 条时 MUST 默认选中该路由并以**纯文本**展示路由名（不渲染下拉，创建按钮直接可用）；2 条及以上 MUST 渲染下拉选择。路由固定入口（路由详情页内「新建」）不出路由字段，行为不变。

页面 MUST 识别 `?new=1` 查询参数并自动打开创建对话框，随后 MUST 清除该参数。

#### Scenario: 列表
- **WHEN** 已认证用户进入 API Keys 页面
- **THEN** 只展示自己路由下的 Key 及其所属路由、掩码与状态；空列表展示空态

#### Scenario: 创建后一次性展示
- **WHEN** 用户创建 key 成功
- **THEN** 界面一次性展示明文（可复制），并提示关闭后无法再次查看

#### Scenario: 启停与删除
- **WHEN** 用户切换某 key 的状态或删除某 key
- **THEN** 立即提交；删除需二次确认

#### Scenario: 单路由默认选中
- **WHEN** 当前用户只有一条路由并在 API Keys 页打开创建对话框
- **THEN** 路由字段显示该路由名（纯文本、不可下拉），无需选择即可创建

#### Scenario: 多路由下拉
- **WHEN** 当前用户有两条及以上路由并打开 API Keys 页的创建对话框
- **THEN** 路由字段渲染为下拉选择

#### Scenario: admin 不看到他人 Key
- **WHEN** admin 进入 API Keys 页面
- **THEN** 只展示 admin 自己路由下的 Key

#### Scenario: 直达创建
- **WHEN** 用户打开 `/api-keys?new=1`
- **THEN** 页面加载后自动打开创建对话框，且地址栏中的 `new=1` 随即被移除

### Requirement: 使用和统计界面

控制台 SHALL 提供「使用和统计」页面，展示**当前用户拥有的路由**的用量：KPI（请求数 / 错误率 / 总 token / 输入 token / 输出 token / P95 延迟 / 换号次数）、按时间的趋势图、分组表与最近请求表。

页面 SHALL 支持时间范围切换（近 24 小时 / 今天 / 昨天 / 近 7 天 / 近 30 天 / 本月 / 上月 / 自定义）。

分组表 MUST 支持按路由、API Key、模型与 Provider 分组；Provider 分组 MUST 展示输入 / 输出 token。

页面 MUST NOT 展示其他用户路由的用量；admin 也不例外。admin 在页面上 MUST 看到前往「全局统计」的提示或入口。

窗口内无数据时 MUST 展示空态，SHALL NOT 展示伪造数值。

#### Scenario: 展示用量
- **WHEN** 用户进入「使用和统计」且窗口内有数据
- **THEN** 展示 KPI、趋势图、分组表与最近请求

#### Scenario: admin 自身范围
- **WHEN** admin 进入「使用和统计」
- **THEN** 只展示 admin 自己路由的用量

#### Scenario: 时间范围
- **WHEN** 用户选择「本月」
- **THEN** 页面按本地时区的本月起点到当前时间重新统计

#### Scenario: 切换分组维度
- **WHEN** 用户把分组从「按路由」切换为「按模型」或「按 Provider」
- **THEN** 分组表按所选维度重新展示；Provider 行包含 token

#### Scenario: 暂无数据
- **WHEN** 窗口内无数据
- **THEN** 页面展示空态，不展示任何数值

### Requirement: 设置界面

控制台 SHALL 提供「设置」分组下的「个人」页面，承载**个人**设置（昵称、头衔、口令）。系统运行信息与全部系统设置 SHALL 由独立的「系统」页面承载（仅 admin）。

个人设置 MUST 支持修改昵称、头衔与口令，且 MUST 通过**对话框**编辑：页面本身只展示只读信息与入口按钮，MUST NOT 内联常驻编辑表单。

资料区 MUST 展示头像、显示名、`@登录名`、角色徽标与头衔（存在时），并提供「编辑资料」入口；点击后 MUST 以对话框编辑昵称与头衔，取消或关闭对话框 MUST NOT 提交任何修改。

口令区 MUST 展示当前状态与一个入口按钮，点击后以对话框完成操作。口令区 MUST 按**会话来源**分流：

- 口令登录会话（含员工首次登录、`standalone` 的 admin）：入口为「修改口令」，对话框中 MUST 包含当前口令、新口令与确认新口令；
- 桌面令牌会话：入口为「设置口令」，对话框中 MUST NOT 包含任何「当前口令」输入；提交 MUST 携带本机桌面登录令牌；口令区 MUST 展示当前是否已设置口令（取 `GET /api/auth/me` 的 `has_password`）。

桌面令牌会话的口令区 MUST 同时展示兜底提示：忘记口令时可在服务器上执行 `solidway <data_root> --reset-password` 重置后重新设置。

口令修改成功或设置成功后，界面 MUST 立即反映 `has_password` 的新状态，MUST NOT 要求刷新页面。

#### Scenario: 修改个人资料
- **WHEN** 用户在「编辑资料」对话框中提交新的昵称 / 头衔
- **THEN** 保存成功、对话框关闭，身份区即时反映新值

#### Scenario: 取消编辑资料
- **WHEN** 用户打开「编辑资料」对话框后点击取消或关闭
- **THEN** 不提交任何修改，再次打开时输入框回到当前值

#### Scenario: 修改口令
- **WHEN** 用户以口令登录，在「修改口令」对话框中提交当前口令与新口令
- **THEN** 成功后提示并关闭对话框；当前口令错误时在对话框内给出可读错误

#### Scenario: 非 admin 不渲染系统设置
- **WHEN** 非 admin 打开「个人」页
- **THEN** 页面只包含个人资料与口令，不包含任何系统设置

#### Scenario: 桌面会话设置口令无需旧口令
- **WHEN** 桌面令牌会话下的用户点击口令区的「设置口令」
- **THEN** 对话框只包含「新口令」「确认新口令」，MUST NOT 展示「当前口令」输入

#### Scenario: 未设置口令的状态
- **WHEN** `has_password` 为 `false`
- **THEN** 口令区明确提示当前尚未设置口令（只能在本机桌面客户端进入），并说明设置后可用于其他登录方式

#### Scenario: 已设置口令的状态
- **WHEN** `has_password` 为 `true`
- **THEN** 口令区提示已设置口令，仍允许直接设置新口令（桌面令牌授权）

#### Scenario: 设置成功后即时切换
- **WHEN** 桌面会话中设置口令成功
- **THEN** 口令区状态立即变为「已设置」，无需刷新页面

#### Scenario: 口令登录仍走修改口令
- **WHEN** 员工在桌面客户端内退出登录后以「用户名 + 初始口令」登录
- **THEN** 口令区展示「当前口令 / 新口令 / 确认新口令」表单，提交需携带当前口令

### Requirement: 用户管理界面（仅 admin）

控制台 SHALL 提供「用户管理」页面。该入口 MUST 仅对 `admin` 用户可见；非 admin MUST NOT 看到入口，直接访问 MUST 被拒绝（重定向）。

页面 MUST 支持列出、创建、编辑（角色 / 停用 / 昵称 / 头衔）、重置口令、归档与 offboard。服务端生成的口令 MUST 一次性展示。归档用户默认隐藏，页面 MUST 提供“显示已归档”切换；归档用户 MUST 展示“已归档（不可恢复）”标记，MUST NOT 提供恢复入口。

offboard MUST 要求选择继任者，并在一次操作中转移该用户全部活跃路由、归档用户并撤销会话。

#### Scenario: admin 可见
- **WHEN** admin 用户进入控制台
- **THEN** 侧栏「设置」分组包含「用户管理」

#### Scenario: 非 admin 不可见且不可访问
- **WHEN** 非 admin 用户查看侧栏或直接访问 `/users`
- **THEN** 侧栏无该入口，且访问被重定向

#### Scenario: 一次性展示生成的口令
- **WHEN** 创建用户或重置口令后
- **THEN** 一次性展示生成的口令并提示仅显示一次

#### Scenario: 归档用户
- **WHEN** admin 归档一个没有活跃路由的用户
- **THEN** 用户从默认列表消失，旧会话立即失效

#### Scenario: 显示归档用户
- **WHEN** admin 切换“显示已归档”
- **THEN** 归档用户出现在列表，但只读且不提供恢复入口

### Requirement: 系统界面（仅 admin）

控制台 SHALL 提供独立的「系统」页面，展示运行信息（版本、前端产物来源与目录、数据目录）与全部系统设置（网络、会话与安全、转发参数、用量统计）。

「系统」入口 MUST 仅对 `admin` 用户可见；非 admin MUST NOT 看到入口，直接访问 `/system` MUST 被拒绝（重定向）。

#### Scenario: 从侧栏进入
- **WHEN** admin 用户点击侧栏「系统」
- **THEN** 打开系统页面，展示运行信息与全部系统设置

#### Scenario: 非 admin 不可见且不可访问
- **WHEN** 非 admin 用户查看侧栏或直接访问 `/system`
- **THEN** 侧栏无该入口，且访问被重定向

#### Scenario: 系统信息只读
- **WHEN** 用户查看系统页中的运行信息
- **THEN** 只读展示，不提供编辑入口

### Requirement: 系统设置界面（仅 admin）

控制台 SHALL 在「系统」页提供「系统设置」区，用于编辑系统设置（首个为用量统计设置）。

该区 MUST NOT 对非 admin 渲染；保存 MUST 调用 `PATCH /api/settings`，非法值 MUST 以可读错误提示。

#### Scenario: admin 可见
- **WHEN** admin 打开「系统」页
- **THEN** 页面包含「系统设置」区并可编辑用量设置

#### Scenario: 非 admin 不可见
- **WHEN** 非 admin 直接访问「系统」页
- **THEN** 重定向，且不渲染「系统设置」区

#### Scenario: 保存
- **WHEN** admin 修改并保存用量设置
- **THEN** 保存成功并有提示；服务端校验失败时展示可读错误

### Requirement: 品牌 mark 与主题一致

控制台内的品牌 mark（侧栏与登录页）MUST 通过主题令牌 `--brand-mark` 着色，SHALL NOT 硬编码单一色值。

深色主题为品牌绿 `#00e5a0`；浅色主题为加深绿 `#059669`，以保证与浅色背景的对比度 ≥3:1（WCAG 1.4.11 图形对比度）。

自带深色底色的方块版本 MUST 只用于图标位（favicon / 应用图标 / 托盘）；页面内 SHALL NOT 使用带底色的 logo。

#### Scenario: 浅色主题下可辨识
- **WHEN** 用户在浅色主题下查看侧栏或登录页
- **THEN** 品牌 mark 使用浅色令牌 `#059669`，与背景对比度 ≥3:1

#### Scenario: 深色主题保持品牌绿
- **WHEN** 用户在深色主题下查看侧栏或登录页
- **THEN** 品牌 mark 使用深色令牌 `#00e5a0`

#### Scenario: 切换主题即时生效
- **WHEN** 用户在浅色与深色之间切换
- **THEN** 品牌 mark 颜色随之变化，无需刷新页面

#### Scenario: 图标位自带底色
- **WHEN** 检查 favicon 或应用 / 托盘图标
- **THEN** 其为自带深色底色的方块版本，不依赖所在位置的背景色

### Requirement: 网络设置界面

控制台 SHALL 在系统页提供网络设置，其中「绑定地址」MUST 为**多选芯片 + 手动补充输入**：

- 候选来自 `GET /api/network/addresses`：本机回环、任何地址、探测到的本机地址；
- 探测到的本机地址 MUST 展示网卡名；默认只列出「每张物理网卡 × 每个协议族」的第一个地址，`virtual_nic = true` 与同网卡其余地址（临时 IPv6 等）MUST 默认收起，并提供「显示其余地址」开关；
- 已选地址以芯片展示，可逐个移除；手动输入 MUST 校验为合法 IP；
- 选择通配地址（`0.0.0.0` / `::`）时 MUST 自动清空其它选项并禁止再选其它地址，并给出说明；
- 保存前 MUST 在前端校验：至少一个地址、无重复、通配符不与其他地址共存；
- 保存后 MUST 提示"需要重启生效"（与其他网络项一致）。

#### Scenario: 选择多个地址
- **WHEN** 用户勾选 `127.0.0.1` 与探测到的局域网地址并保存
- **THEN** 两者以芯片展示，保存成功并提示重启

#### Scenario: 通配符互斥
- **WHEN** 用户勾选 `0.0.0.0`
- **THEN** 其它地址被清空且不可再选，界面说明"任何地址已包含全部"

#### Scenario: 手动输入
- **WHEN** 用户手动输入 `10.0.0.5` 并回车
- **THEN** 该地址成为芯片；输入非 IP 时给出内联错误且不添加

#### Scenario: 空列表阻止保存
- **WHEN** 用户移除全部地址
- **THEN** 保存被阻止并提示"至少需要一个绑定地址"

#### Scenario: 展示生效状态
- **WHEN** 管理员打开「系统」页
- **THEN** 网络分区显示 `effective`（当前实际监听，多个地址用逗号分隔）与 `source`（数据库 / 命令行）

#### Scenario: 命令行覆盖时仍可编辑
- **WHEN** `source` 为 `command_line`
- **THEN** 地址与端口输入区仍可编辑，并说明修改会保存到数据库、需在不带 `--listen` 重启后生效

### Requirement: 重启提示按模式区分

保存网络设置后，若需重启：`can_restart` 为真时 MUST 显示「立即重启服务」按钮；为假时 MUST 只提示由管理员重启服务。

#### Scenario: 桌面模式提供按钮
- **WHEN** `GET /api/app/info` 返回 `can_restart = true` 且改动需重启
- **THEN** 显示重启按钮，点击后调用 `POST /api/app/restart` 并等待恢复

#### Scenario: 独立模式只提示
- **WHEN** `can_restart = false` 且改动需重启
- **THEN** 提示「需管理员重启 solidway 后生效」，不显示按钮

### Requirement: 设置口令界面

控制台 SHALL 提供公开路由 `/bootstrap` 的「设置管理员口令」页面，无需登录即可访问。

页面 MUST 从 URL fragment 读取引导令牌，MUST NOT 把令牌写入 URL query、`localStorage` 或页面标题。

页面 MUST 提供「新口令」「确认新口令」两个输入框，校验新口令不少于 8 位且两次一致。

提交成功 MUST 直接进入控制台（已登录态），SHALL NOT 再要求单独登录。

#### Scenario: 设置成功后进入控制台
- **WHEN** 用户在 `/bootstrap#<token>` 提交合法新口令
- **THEN** 页面清空 fragment 并跳转到 `/routes`，用户处于已登录状态

#### Scenario: 口令不一致
- **WHEN** 两次输入不一致
- **THEN** 页面就地提示错误，不发起请求

#### Scenario: 令牌失效
- **WHEN** 访问 `/bootstrap` 时令牌已被消费或不存在，且 `GET /api/auth/bootstrap` 返回 `pending = false`
- **THEN** 页面提示已失效并跳转到 `/login`

#### Scenario: 令牌未带但仍有待引导
- **WHEN** 访问 `/bootstrap` 无 fragment，且 `pending = true`
- **THEN** 页面提示「请从托盘『打开控制台』重新进入」，不展示任何令牌

### Requirement: 版本号对所有角色可见

控制台 SHALL 在侧栏底部（当前用户区域）对所有已认证角色展示当前版本号，格式为 `v<major.minor.patch>`。

版本号 MUST 来自 `GET /api/version`；MUST NOT 要求 admin 权限，也 MUST NOT 依赖 `GET /api/system`（该接口仅 admin）。

版本号 SHOULD 以次要样式（`text-muted-foreground`、小号字）呈现，且 MUST NOT 额外增加侧栏高度。

#### Scenario: 普通用户可见
- **WHEN** role 为 `user` 的用户进入控制台
- **THEN** 侧栏底部显示当前版本号，且侧栏不出现「系统」「用户管理」入口

#### Scenario: admin 可见
- **WHEN** admin 进入控制台
- **THEN** 侧栏底部同样显示版本号

#### Scenario: 与系统页一致
- **WHEN** admin 打开「系统」页查看「版本」
- **THEN** 该值与侧栏底部的版本号相同

#### Scenario: 取不到版本时不显示
- **WHEN** `GET /api/version` 失败
- **THEN** 侧栏不渲染版本号，其余布局与功能不受影响

### Requirement: API Key 有效期与列表筛选

创建 API Key 对话框 SHALL 提供有效期选择：永不过期（默认）、7 天、30 天、90 天、自定义日期。自定义模式未选日期时 MUST 禁止提交。

API Key 列表 MUST 展示到期时间；已过期 MUST 显示「已过期」徽标，7 天内到期 SHOULD 显示「即将过期」徽标。

路由管理对话框与独立 API Keys 页 SHALL 提供「修改有效期」（延长/清空/自定义）。

独立 API Keys 页 SHALL 支持按名称过滤、按路由过滤；过滤结果超过 20 条 SHALL 分页展示。

路由管理对话框的「查看全部」SHALL 跳转独立页并带上当前路由过滤。

#### Scenario: 创建带有效期
- **WHEN** 用户选择 30 天并创建
- **THEN** 列表显示约 30 天后的到期时间

#### Scenario: 自定义日期未选
- **WHEN** 用户选择自定义日期但未填写
- **THEN** 创建按钮禁用

#### Scenario: 已过期
- **WHEN** 某 Key 的到期时间已过
- **THEN** 列表显示「已过期」徽标

#### Scenario: 按名称/路由过滤
- **WHEN** 用户输入名称关键字或选择路由
- **THEN** 表格只展示匹配的 Key，超过 20 条时显示分页

#### Scenario: 从路由查看全部
- **WHEN** 用户在路由的 Key 管理对话框点击「查看全部」
- **THEN** 进入独立页并预选该路由过滤

### Requirement: HTTPS 设置与状态

控制台 SHALL 在系统页（仅 admin）提供「HTTPS」设置区块：

- **启用开关**：默认关闭；桌面模式与独立模式一致；
- **ACME 邮箱**；
- **Challenge**：默认 `HTTP-01`，可选 `TLS-ALPN-01`；
- **Directory**：`production` / `staging`，选择 staging 时 MUST 显示醒目提示；
- **端口**：HTTP / HTTPS，默认 80 / 443；当 challenge 所需公网端口（`http-01` 为 80、`tls-alpn-01` 为 443）与所选端口不一致时 MUST 展示警告，说明需要端口映射；
- **HSTS**：默认关闭，附带风险说明；
- **证书域名**：只读展示来自 `access.public_origins` 的合法域名；IP / 非法项 MUST 显示为「已跳过」并给出原因；
- **状态**：证书域名、到期时间、上次续期、最后错误、`restart_required`、`cert.state`（`ok` / `waiting` / `paused`）；
- **重试**：`cert.state=paused` 时 MUST 提供「立即重试」入口，调用 `POST /api/tls/retry` 后展示受理反馈；证书未就绪时 MUST 展示等待说明，MUST NOT 提示用户「已可访问」。

启用前 MUST 校验：至少一个合法域名、邮箱非空、端口合法且不冲突；不满足时 MUST 阻止启用并给出内联错误。

修改启用状态或端口后 MUST 提示需要重启；证书续期 MUST NOT 要求重启。

#### Scenario: 没有合法域名
- **WHEN** `public_origins` 只有 IP，用户尝试启用
- **THEN** 启用被阻止，提示缺少 DNS 域名

#### Scenario: IP 被跳过
- **WHEN** `public_origins` 同时包含域名与 IP
- **THEN** 域名用于证书，IP 显示为「已跳过」及原因

#### Scenario: HSTS 默认关闭
- **WHEN** 用户首次打开 HTTPS 设置
- **THEN** HSTS 开关为关闭状态

#### Scenario: 需要重启
- **WHEN** 用户修改启用状态或端口并保存
- **THEN** 界面提示需要重启，并展示 `restart_required=true`

#### Scenario: 端口映射警告
- **WHEN** 用户选择 `HTTP-01` 且把 HTTP 端口改为 8080
- **THEN** 表单展示「需要 80→8080 端口映射」警告，但不阻止保存

#### Scenario: 暂停后重试
- **WHEN** 证书状态为 `paused`
- **THEN** 展示最后错误与「立即重试」按钮，点击后给出受理反馈

#### Scenario: 等待签发
- **WHEN** 证书尚未签发成功
- **THEN** 展示等待提示，不提供对外访问地址为可用的结论

#### Scenario: 证书状态
- **WHEN** 证书已签发
- **THEN** 展示证书域名、到期时间、上次续期时间；失败时展示可读错误

### Requirement: Provider 的 API 格式多选

控制台 SHALL 在自定义模式下以多选（Checkbox）展示**三种可入站**的 API 格式：`Chat Completions`、`Responses`、`Anthropic Messages`。三者均有对应的入站端点，均可被入站请求选中。

`provider_native` 与 `gemini_generate_content` MUST NOT 出现在多选中，也 MUST NOT 出现在提交值中；历史记录中的旧取值 MUST NOT 由界面重新提交。

预设模式下 MUST 以只读形式展示所选预设声明的格式，MUST NOT 提供勾选控件；需要不同格式 MUST 通过「复制为自定义」或「转为自定义」。选择预设时 SHALL 同时只读带出按预设展开的 `base_url` 与 `auth_scheme`。

表单 MUST NOT 提供自由文本的协议输入；协议信息不落库、不可编辑。

提交 MUST 携带 `api_formats`。未勾选任何格式时 SHALL 允许保存，并 MUST 在表单内提示该 Provider 暂时不会被任何入站请求选中。

#### Scenario: 预设预填
- **WHEN** 用户选择 `openai` 预设
- **THEN** `Chat Completions` 与 `Responses` 均被勾选，`base_url` 预填为该预设的默认根

#### Scenario: 兼容上游
- **WHEN** 用户选择 `openai-compatible` 预设
- **THEN** 仅展示 `Chat Completions`（只读）；需要追加其他格式须复制为自定义

#### Scenario: Anthropic 预设
- **WHEN** 用户选择 `anthropic` 预设
- **THEN** 勾选 `Anthropic Messages`，`base_url` 预填 `https://api.anthropic.com/v1`，`auth_scheme` 预填 `x-api-key`

#### Scenario: 全部取消
- **WHEN** 用户取消全部勾选并保存
- **THEN** 保存成功，表单提示该 Provider 当前不服务任何入站格式

#### Scenario: Gemini 原生预设已移除
- **WHEN** 用户打开预设选择器
- **THEN** 不存在 `google`、`google-vertex`、`google-vertex-global` 预设，无法勾选 `Gemini GenerateContent`

#### Scenario: 历史旧格式不再提交
- **WHEN** 记录历史上含 `provider_native` 或 `gemini_generate_content`，用户在自定义模式保存
- **THEN** 界面不展示这两个旧取值，提交值只包含当前勾选的三格式之一

#### Scenario: 预设格式只读
- **WHEN** 用户选择 `openai` 预设
- **THEN** `Chat Completions` 与 `Responses` 以只读形式展示，无勾选框

#### Scenario: Vertex 拆分后只读格式
- **WHEN** 用户选择 `google-vertex-anthropic` 或 `google-vertex-global-anthropic`
- **THEN** 只读展示 `Anthropic Messages`

#### Scenario: 自定义多选
- **WHEN** 用户在自定义模式下勾选 `Anthropic Messages` 并保存
- **THEN** 提交该格式并原样回显

### Requirement: 模型别名编辑

控制台 SHALL 在 Provider 表单中以两列编辑器维护模型清单：客户端模型名、上游模型 id，并 SHALL 支持逐行添加与删除。

「获取模型」SHALL 把上游返回的模型 id 同时填入两列（默认同名），并 MUST 跳过已存在的客户端模型名。

控制台 MUST 只提交 `name` 非空的行；`upstream` 留空时由服务端归一化为同名。

Provider 列表 SHALL 展示映射关系：同名显示一个名字，不同名显示为 `name → upstream`。

#### Scenario: 添加映射
- **WHEN** 用户添加一行，客户端名 `gpt-4o`、上游 id `gpt-4o-2024-08-06` 并保存
- **THEN** 提交对应对象数组，列表显示 `gpt-4o → gpt-4o-2024-08-06`

#### Scenario: 获取模型不重复
- **WHEN** 模型清单已含 `gpt-5`，用户点击「获取模型」且上游也返回 `gpt-5`
- **THEN** 不追加重复行

#### Scenario: 空行丢弃
- **WHEN** 用户添加一行但客户端模型名为空并保存
- **THEN** 该行不提交，也不报错

### Requirement: 对外访问地址设置

控制台 SHALL 在系统页（仅 admin）提供「对外访问地址」设置：可增删多条域名 / 路径前缀并保存。

输入项 MUST 允许只填域名或前缀（如 `https://gw.example.com`、`https://host/solidway`），MUST NOT 要求用户手工补 `/v1`；已填写 `/v1` 也 MUST 接受。

第一项 MUST 作为默认地址；列表为空时 MUST 提示将回退为当前控制台 origin + `/v1`。

设置区 MUST 说明：接入信息会自动拼接 `/v1`；HTTPS 由 URL 的 scheme 表示；本服务本身仍监听 HTTP，TLS 需在前置反向代理终止。

#### Scenario: 添加地址
- **WHEN** admin 添加 `https://gw.example.com` 并保存
- **THEN** 保存成功，路由接入信息默认展示 `https://gw.example.com/v1`

#### Scenario: 已带 /v1
- **WHEN** admin 保存 `https://gw.example.com/v1`
- **THEN** 接入信息仍展示 `https://gw.example.com/v1`，不重复拼接

#### Scenario: 清空列表
- **WHEN** admin 删除全部地址并保存
- **THEN** 保存成功，接入信息回退为 origin + `/v1`

#### Scenario: 非法地址
- **WHEN** admin 保存缺少 scheme、非 http(s) 或含 query/fragment 的地址
- **THEN** 请求返回 `422`，界面保留草稿并提示错误

### Requirement: 路由接入信息

控制台 SHALL 在路由详情页展示该路由的接入信息，包含：

- **Base URL**：读取 `GET /api/access` 的对外 origin 列表（第一项为默认），展示时拼接 `/v1`，提供复制入口；未配置时回退为当前控制台 origin + `/v1`。配置多个时 SHALL 允许切换；
- **API Key**：该路由下已签发 Key 的掩码与数量；MUST 提供上下文内的「新建」入口（对话框，路由固定）与「管理」入口（该路由 Key 列表，可启停/删除）；创建成功后 MUST 在同一页面展示一次性明文；无 Key 时 MUST 提示并提供创建入口；
- **可用模型**：该路由下启用中 Provider 声明的客户端模型名（去重、排序），按入站格式归类；示例中使用的模型名 MUST 取自该示例所需格式的模型清单；
- **格式徽章**：展示该路由启用中 Provider 实际声明的入站格式；
- **调用示例与 Agent 接入**：MUST 以单一入口按钮打开对话框承载，对话框内以两级标签组织，两组共用同一个代码区与复制按钮：
  - **SDK 调用**组：二级标签至少覆盖 `curl` / `Python` / `Node`，按路由实际支持的 API 格式（Chat Completions / Responses / Anthropic Messages）切换，示例使用各家族主流 SDK（OpenAI / Anthropic 的 Python 与 Node），可复制；
  - **终端 Agent**组：二级标签覆盖 `Claude Code`、`Codex`、`OpenCode`、`pi` 与「其他（OpenAI 兼容）」，各给出可直接复制的配置片段（Claude Code 的环境变量式接入、Codex 的 `~/.codex/config.toml` 的 `model_providers` 段、OpenCode 的 `opencode.json`、pi 的 `~/.pi/agent/models.json`、通用 `OPENAI_BASE_URL` / `OPENAI_API_KEY`），并 MUST 标注各自所需的入站格式；Codex 的 `wire_api` MUST 按路由可用格式自动选择（有 Responses 用 `responses`，否则 `chat`）；某 Agent 所需格式未在路由内声明时 MUST 提示先给 Provider 勾选该格式，「其他（OpenAI 兼容）」在缺少 `Chat Completions` 时 MUST NOT 给出不可用配置，改为指向可用端点；
  - 代码 SHOULD 进行语法高亮（shell / Python / JavaScript / JSON / TOML 配置）。

鉴权头 MUST 按家族展示：OpenAI 家族（Chat Completions / Responses）用 `Authorization: Bearer <API Key>`；Anthropic 家族用 `x-api-key: <API Key>`。终端 Agent 配置中的凭据 MUST 为占位符，指向本接入信息签发的 API Key，MUST NOT 指向上游 Provider 的 key。

端点说明：MUST 注明三格式各自的入站端点——`POST /v1/chat/completions`、`POST /v1/responses`、`POST /v1/messages`——并按该路由实际声明的格式展示。示例 MUST 只覆盖该路由启用中 Provider 实际声明的格式；无可用入站格式时 MUST 提示先勾选格式，而不是给出不可用的示例。

地址口径（照抄即可用）：示例中的 URL / base_url MUST 与所用客户端的拼接惯例一致，MUST NOT 产生重复版本段：

- **curl**：URL MUST 写全（含家族根与端点路径），如 `https://gw.example.com/v1/messages`；
- **Anthropic SDK（Python / Node）与 Claude Code（`ANTHROPIC_BASE_URL`）**：MUST 使用去掉 `/v1` 的根，由 SDK / Agent 自行拼 `/v1/messages`；
- **OpenAI SDK / Codex / OpenCode / pi / 通用环境变量**：MUST 使用含 `/v1` 的 Base URL，由客户端拼 `/chat/completions`、`/responses`。

SDK / Agent 示例 SHOULD 以注释给出该客户端实际请求的完整 URL（含版本段），便于对照排查。

接入信息 MUST NOT 展示或请求 API Key 明文；示例中的 Key MUST 使用占位符。

#### Scenario: 多个对外地址
- **WHEN** 系统设置中配置了多个对外 Base URL
- **THEN** 接入信息提供地址切换，示例与复制使用当前选中的地址

#### Scenario: 已有 Key
- **WHEN** 路由下存在一条启用中的 API Key
- **THEN** 展示其掩码与 Key 总数，并显示「管理」入口

#### Scenario: 没有 Key
- **WHEN** 路由下没有任何 API Key
- **THEN** 展示提示与「创建 API Key」入口，点击直接在当前页打开创建对话框

#### Scenario: 在路由页创建
- **WHEN** 用户在路由页创建 API Key
- **THEN** 路由固定为当前路由，创建成功后在同一页展示一次性明文，可复制；不跳转

#### Scenario: 管理该路由的 Key
- **WHEN** 用户点击接入信息中的「管理」
- **THEN** 打开该路由的 Key 列表，支持启停与删除，并保留「查看全部」入口到独立 API Keys 页

#### Scenario: 对话框入口
- **WHEN** 路由声明至少一种入站格式
- **THEN** 接入信息展示格式徽章与「调用示例与 Agent 接入」入口按钮，点击打开对话框；对话框内 SDK 调用与终端 Agent 两组共用同一个代码区与复制按钮

#### Scenario: 两种格式都支持
- **WHEN** 路由内 Provider 同时声明两种及以上入站格式（如 `chat_completions` 与 `anthropic_messages`）
- **THEN** SDK 调用组提供格式切换，示例的端点、请求体与鉴权头随格式变化（如 Anthropic 示例给出 `POST {Base URL}/messages` 完整 URL 与 `x-api-key` 头）

#### Scenario: Gemini 格式已移除
- **WHEN** 路由历史上声明过 `gemini_generate_content`（该格式已移除）
- **THEN** 接入信息不展示 Gemini 格式徽章、SDK 示例或 `x-goog-api-key` 鉴权头；`/v1beta/models/...` 调用返回 404
#### Scenario: 终端 Agent 配置示例
- **WHEN** 用户切换到「终端 Agent」组
- **THEN** 提供 Claude Code / Codex / OpenCode / pi / 其他（OpenAI 兼容）五个标签，各标签给出可复制的配置片段并标注所需入站格式

#### Scenario: Claude Code 示例
- **WHEN** 用户选择 Claude Code 标签
- **THEN** 示例以去掉 `/v1` 的根作为 `ANTHROPIC_BASE_URL`，鉴权写入 `ANTHROPIC_AUTH_TOKEN` 占位符，模型写入 `ANTHROPIC_MODEL`，所需格式为 Anthropic Messages

#### Scenario: Anthropic SDK 示例地址
- **WHEN** 用户查看 Anthropic Messages 格式的 Python / Node SDK 示例
- **THEN** `base_url` 为去掉 `/v1` 的根（SDK 自行拼 `/v1/messages`），并注明实际请求为 `POST {根}/v1/messages`，照抄即可用

#### Scenario: curl 示例地址写全
- **WHEN** 用户查看任一格式的 curl 示例
- **THEN** URL 写全（含家族根与端点路径，如 `https://gw.example.com/v1/messages`），照抄即可用

#### Scenario: 不出现重复版本段
- **WHEN** 用户查看任一示例（SDK / 终端 Agent / curl）
- **THEN** 不出现 `/v1/v1`、`/v1beta/v1beta` 这类重复版本段

#### Scenario: Codex wire_api 自动选择
- **WHEN** 路由同时声明 `responses` 与 `chat_completions`
- **THEN** Codex 配置 `wire_api` 取 `responses`；只声明 `chat_completions` 时取 `chat`

#### Scenario: 终端 Agent 缺格式提示
- **WHEN** 用户选择 Claude Code 而路由内没有 Provider 声明 `anthropic_messages`
- **THEN** 提示先给 Provider 勾选该格式，而不是把配置当作可用配置引导接入

#### Scenario: 示例模型按格式取
- **WHEN** 路由声明多种格式且用户切换示例
- **THEN** 示例模型名取自该示例所需格式的客户端模型名清单，跨格式不取错

#### Scenario: 代码高亮
- **WHEN** 展示调用示例或 Agent 配置
- **THEN** shell / Python / JavaScript / JSON / TOML 配置进行语法高亮，且随明暗主题保持可读

#### Scenario: 只有 Responses 格式
- **WHEN** 路由内启用中 Provider 只声明一种入站格式（如 `responses`）
- **THEN** 示例固定使用该格式（如 `POST /v1/responses`），不展示格式切换

#### Scenario: 没有可用入站格式
- **WHEN** 路由内 Provider 的 `api_formats` 均为空（或没有任何启用中 Provider）
- **THEN** 提示先在 Provider 表单勾选入站格式，不给出任何调用示例

#### Scenario: 没有模型
- **WHEN** 路由下没有启用中 Provider 声明的模型
- **THEN** 可用模型区域提示先添加 Provider 与模型，示例中的模型名使用占位符

#### Scenario: 复制
- **WHEN** 用户点击 Base URL 或调用示例 / Agent 配置的复制按钮
- **THEN** 对应内容写入剪贴板，并给出短暂的成功反馈

### Requirement: 全局统计界面（仅 admin）

控制台 SHALL 提供独立的「全局统计」页面 `/usage/global`，展示全站用量。

该入口 MUST 仅对 `admin` 可见；非 admin MUST NOT 看到入口，直接访问 MUST 被拒绝并重定向。

页面 MUST 支持与普通统计相同的时间范围，并额外支持按归属用户、路由、API Key、模型与 Provider 筛选。

页面 MUST 支持按用户、路由、模型、Provider 与 API Key 分组；按路由分组时 MUST 展示 owner 标识以区分同名路由。

全局统计页面 MUST 为只读，MUST NOT 提供修改路由、Provider 或 API Key 的入口，也 MUST NOT 提供进入他人路由详情页的链接。

#### Scenario: admin 进入全局统计
- **WHEN** admin 点击侧栏「全局统计」
- **THEN** 打开 `/usage/global` 并展示全站 KPI、趋势与分组

#### Scenario: 非 admin 不可见且不可访问
- **WHEN** 非 admin 查看侧栏或直接访问 `/usage/global`
- **THEN** 侧栏无入口，且访问被重定向

#### Scenario: 按用户分组
- **WHEN** admin 将分组切换为「用户」
- **THEN** 按路由归属用户展示请求、token 与错误

#### Scenario: 同名路由区分
- **WHEN** 两个用户各有一条名为 `prod` 的路由
- **THEN** 全局统计按路由分组时通过 owner 区分两行

#### Scenario: 只读
- **WHEN** admin 查看全局统计
- **THEN** 页面不提供编辑路由、Provider 或 API Key 的操作

### Requirement: 路由详情用量概览

路由详情页 SHALL 提供当前路由的用量概览区块，仅对路由 owner 渲染。

区块 MUST 支持时间范围切换（近 24 小时 / 近 7 天 / 近 30 天 / 自定义），并展示：

- KPI：请求数、错误率、总 token、P95 延迟；
- 趋势：请求数与 token；
- 模型 Top N；
- Provider Top N，且 MUST 包含输入 / 输出 token。

窗口内无数据时 MUST 展示空态。区块加载失败 MUST NOT 阻塞路由详情其他部分。

#### Scenario: 展示路由用量
- **WHEN** 路由 owner 打开路由详情
- **THEN** 页面展示该路由的用量概览

#### Scenario: Provider token
- **WHEN** 路由 owner 查看 Provider Top N
- **THEN** 每行展示输入 / 输出 token

#### Scenario: 非 owner 不可见
- **WHEN** 非 owner（包括 admin）打开他人路由详情
- **THEN** 不展示该路由详情与用量概览

#### Scenario: 无数据
- **WHEN** 当前窗口内该路由无用量
- **THEN** 用量区块展示空态

### Requirement: 路由选号策略界面

控制台 SHALL 在路由详情页提供「选号策略」区块（位于接入信息之后、Provider 列表之前），包含：

- **策略选择**：`轮询` / `可用性优先` / `首字节优先` / `综合加权` 四项，每项 MUST 附带一句说明（含"只影响选号，不影响请求与响应字节"）；
- **权重与阈值**：仅「综合加权」可编辑（`uptime` 与 `latency` 两个权重、`explorationRate`；惩罚阈值与兜底值可折叠在「高级」内）；其它策略 MUST 展示"该策略使用固定权重"并只读显示生效值；
- **实时指标表**：Provider / 优先级 / 首字节时延 / uptime / 样本数 / 状态（新鲜 / 陈旧 / 无样本）/ 当前分数，SHOULD 每 10 秒刷新；Provider 列 MUST 显示与 Provider 列表一致的显示名（`name`，空则 Provider 标识；复用同一 id→label 映射），MUST NOT 直接渲染 `provider_id`；指标行无法映射到已知 Provider 时 MUST 回退为 `provider_id` 的掩码前缀（如前 8 位）；
- **保存**：保存成功后 MUST 提示"已生效，无需重启"；MUST NOT 出现重启提示。

策略或权重导致数据面回退轮询时（`resolved.fallback = true`），界面 MUST 显示可读提示。

#### Scenario: 默认策略
- **WHEN** 路由从未配置策略
- **THEN** 展示「轮询」为选中项，且不展示可编辑权重

#### Scenario: 切换到综合加权
- **WHEN** 用户选择「综合加权」
- **THEN** 展示两个权重与探索率输入，默认值来自服务端 `resolved`

#### Scenario: 保存即时生效
- **WHEN** 用户保存策略
- **THEN** 提示"已生效，无需重启"，指标表按新策略显示分数

#### Scenario: 全零权重提示
- **WHEN** 用户把两个权重都设为 0 并保存
- **THEN** 保存成功，并显示"权重全为 0，当前按轮询选号"的提示

#### Scenario: 指标缺失
- **WHEN** 某 Provider 还没有样本
- **THEN** 该行时延与分数显示为「—」，状态为「无样本」

#### Scenario: 指标表 Provider 显示名
- **WHEN** 路由下存在名为 `alibaba-cn` 的 Provider 且指标表出现其指标行
- **THEN** 该行 Provider 列显示 `alibaba-cn`（与 Provider 列表一致），不显示 UUID

#### Scenario: 指标行无法映射
- **WHEN** 指标行的 `provider_id` 无法映射到当前 Provider 列表
- **THEN** Provider 列回退显示 `provider_id` 的掩码前缀（如前 8 位）

### Requirement: Provider 优先级界面

控制台 SHALL 在路由详情的 Provider 列表中展示「优先级」列，并在新增 / 编辑 Provider 表单中提供优先级输入（数字，默认 `1`）。表单 MUST NOT 再出现「权重」与「排序」输入，列表 MUST NOT 再出现「排序」列。

表单 MUST 以可读文案说明该字段的语义，至少包含两点：**数字越大越优先**、**同档位才由选号策略决定先后**；SHOULD 给出示例（如"便宜的直连账号填 2，兜底账号填 1"）。

优先级输入的合法范围 MUST 为 `1-1000`，MUST NOT 接受 `0`；「暂时不参与选号」MUST 引导用户使用 Provider 的启用状态开关，而不是档位。

- 列表的「优先级」列直接展示档位数值；停用状态由「状态」开关与既有健康列表达，不再用档位表达；
- 语义提示：优先级是**档位**——高档位全部排在低档位之前，档内再按路由的选号策略决定先后。

#### Scenario: 展示优先级
- **WHEN** 路由下三个 Provider 的优先级分别为 1、2、3
- **THEN** 列表按优先级列展示数值，顺序与选号顺序一致

#### Scenario: 填写说明
- **WHEN** 用户打开 Provider 表单
- **THEN** 优先级字段旁展示「数字越大越优先 / 同档位才按策略 / 停用请用状态开关」的说明与示例

#### Scenario: 不接受零
- **WHEN** 用户把优先级填成 0
- **THEN** 表单校验阻止提交，并提示改用「状态」停用该 Provider

#### Scenario: 编辑优先级
- **WHEN** 用户把某 Provider 的优先级改为 0 并保存
- **THEN** 保存成功，列表即时反映，策略指标表中该行不再参与排序

#### Scenario: 无权重与排序
- **WHEN** 打开 Provider 表单或查看 Provider 列表
- **THEN** 只有「优先级」，不存在「权重」输入与「排序」列/输入

### Requirement: 故障记录界面

控制台 SHALL 提供独立的 Provider 故障页面 `/incidents`（仅 admin），并从系统页移除故障区块。

页面 MUST 支持：

- 状态筛选：全部 / 未解决 / 已关闭；
- 路由筛选与 Provider 筛选；
- 时间范围筛选：全部 / 近 24 小时 / 近 7 天 / 近 30 天；
- 展示故障原因、级别、开始 / 结束时间、持续时长、次数、处置方式；
- 对未解决记录提供「解除标记」或「关闭记录」；已关闭历史 MUST 仍可查询。

Route 总览页 MUST 为每条路由展示健康徽标：正常 / N 个故障 / N 个需人工 / N 个异常。
Route 详情页 MUST 在存在未解决故障时展示横幅；Provider 列表 MUST 展示「健康」与「24h 故障」列。

#### Scenario: 查看已关闭历史
- **WHEN** admin 在 `/incidents` 选择「已关闭」
- **THEN** 展示已关闭故障记录及 `resolved_by`

#### Scenario: 按路由与 Provider 筛选
- **WHEN** admin 选择某条路由和某个 Provider
- **THEN** 列表只保留同时匹配两者的故障记录

#### Scenario: 时间范围筛选
- **WHEN** admin 选择「近 7 天」
- **THEN** 只展示 `last_seen_at` 落在最近 7 天内的记录

#### Scenario: Route 健康徽标
- **WHEN** 某路由存在未解决故障
- **THEN** Route 总览对应行展示故障徽标；需要人工时使用更高警示样式

#### Scenario: 长期故障置顶
- **WHEN** 同时存在 `manual_action` 与 `degraded` 的未解决记录
- **THEN** `manual_action` 排在最前，并高亮级别

#### Scenario: 路由横幅
- **WHEN** 路由下某 Provider 存在未解决故障
- **THEN** 路由详情顶部展示横幅

#### Scenario: 解除后即时消失
- **WHEN** admin 点击「解除标记」或「关闭记录」
- **THEN** 该记录从「未解决」筛选结果消失，并刷新 Provider 健康状态

#### Scenario: 系统页不再展示故障
- **WHEN** admin 打开 `/system`
- **THEN** 页面不包含「未解决故障」区块

### Requirement: 告警设置界面

控制台 SHALL 在系统页提供「告警」设置区块（仅 admin）：Webhook URL、是否发送恢复通知、最小发送间隔（分钟）。

保存 MUST 立即生效；`webhook_url` 非空时 MUST 校验为 `http` 或 `https` 绝对 URL，非法时阻止保存并给出内联错误。

#### Scenario: 保存 Webhook
- **WHEN** admin 填入 `https://hooks.example.com/gw` 并保存
- **THEN** 保存成功，之后的新增长期故障会触发推送

#### Scenario: 非法 URL
- **WHEN** admin 填入 `ftp://x` 或相对地址
- **THEN** 阻止保存并提示只支持 http/https

### Requirement: 重资源归档界面

控制台 SHALL 对 Route 与 User 使用“停用 / 归档”语义，而不是物理删除；归档 MUST 明确标注为不可恢复。

- 路由详情 MUST 提供「归档」；归档后 MUST 展示“已归档（不可恢复）”横幅，MUST NOT 提供恢复入口；
- 路由总览 MUST 默认隐藏归档路由，并 MUST 提供“显示已归档”切换；
- 用户页 MUST 提供「归档」；归档前 MUST 展示前置条件（活跃路由数量）；MUST NOT 提供恢复入口；
- 用户页 MUST 提供 offboard 入口：选择继任者并一次完成“转移全部活跃路由 + 归档 + 撤销会话”；
- 所有归档操作 MUST 二次确认，确认文案 MUST 说明“不可恢复、立即停止服务、历史保留、凭据销毁”。

#### Scenario: 归档路由
- **WHEN** owner 在路由详情点击「归档」并确认
- **THEN** 路由立即停止服务，名称释放，页面显示“已归档（不可恢复）”横幅

#### Scenario: 归档后重新使用同名
- **WHEN** owner 归档 `prod` 后新建同名路由
- **THEN** 新建成功；归档路由以“prod（已归档）”只读展示

#### Scenario: 归档仍有活跃路由的用户被拒
- **WHEN** admin 尝试归档仍有活跃路由的用户
- **THEN** 界面展示错误并提示先转移或归档路由

#### Scenario: Offboard 用户
- **WHEN** admin 选择继任者并执行 offboard
- **THEN** 用户全部活跃路由转移给继任者，用户被归档，旧会话失效

### Requirement: 历史资源标签

用量、明细与故障界面 MUST 展示资源当前身份，并标记状态：

- 归档 Route 显示原名称 + “已归档”；
- 已删除 Route / Provider / API Key 显示墓碑标签 + “已删除”；
- 无法解析时回退 id，MUST NOT 让整行消失或查询失败。

#### Scenario: 归档路由历史
- **WHEN** 用户查看已归档路由的历史用量
- **THEN** 行仍然可见，并带“已归档”标记

#### Scenario: 已删除 Provider 历史
- **WHEN** admin 查看已删除 Provider 的历史故障
- **THEN** 名称从墓碑显示，并带“已删除”标记

### Requirement: 归档资源的只读访问

控制台 MUST 允许 owner 查看归档 Route 的详情与历史，但 MUST 禁止在归档 Route 下新增或修改 Provider / API Key。

API Keys 页面 MUST NOT 列出归档 Route 下的 Key；归档路由详情 MAY 以只读方式展示其 Key。

#### Scenario: 归档路由只读
- **WHEN** owner 打开归档路由详情
- **THEN** 可以看到 Provider / Key / 历史，但新增与编辑入口不可用

### Requirement: Provider 预设搜索选择

新增 / 编辑 Provider 时，预设选择器 MUST 为可搜索 Combobox，而不是普通下拉框。

- 默认 MUST 展示全部预设，并按“常用（featured）”与“其他”分组；
- 同一预设族的变体（目录 `group` 相同）MUST 在该分组内相邻展示，并以 `variant_label` 区分（如 Vertex AI 的 Regional / Global）；
- 每个选项 MUST 展示名称、key、`kind` 类型徽章与一句话说明；
- MUST NOT 使用五角星等收藏语义的标记；
- 搜索 MUST 同时匹配 `name`、`key`、`variant_label` 与说明文案，大小写不敏感；
- MUST 保留「自定义 Provider」入口，并固定在列表底部，滚动预设列表时保持可见；
- MUST 支持键盘上下选择、Enter 确认、Escape 关闭。

#### Scenario: 按名称搜索
- **WHEN** 用户输入 `open`
- **THEN** 列表只保留名称或 key 包含 `open` 的预设

#### Scenario: 按 key 搜索
- **WHEN** 用户输入 `azure`
- **THEN** 列表保留 key 包含 `azure` 的预设

#### Scenario: 分组展示
- **WHEN** 用户打开选择器且未输入关键字
- **THEN** 列表按「常用 / 其他」分组展示，不在每行重复推荐标记

#### Scenario: 自定义 Provider
- **WHEN** 用户选择「自定义 Provider」
- **THEN** 进入自定义模式并聚焦 Provider 标识输入框

#### Scenario: 自定义入口固定
- **WHEN** 预设列表较长并滚动到底部
- **THEN** 「自定义 Provider」始终固定在弹层底部可见

#### Scenario: 无匹配
- **WHEN** 关键字不匹配任何预设
- **THEN** 展示无匹配提示，但「自定义 Provider」仍可选择

#### Scenario: 族内变体相邻
- **WHEN** 用户输入 `vertex`
- **THEN** `google-vertex-anthropic` 与 `google-vertex-global-anthropic` 两条条目相邻展示，并分别标注 Regional / Global

### Requirement: 预设端点与自定义副本

Provider 表单 MUST 区分三种来源，且 MUST 由所选 presets 决定端点：

1. **预设条目**：`provider` 锁定为预设 key，`base_url`、`api_formats`、`auth_scheme` 均只读展示，唯一可填的是端点模板变量；
2. **复制为自定义**：以某预设为起点新建独立条目，`provider` 默认 `<预设key>-custom` 且可编辑，`base_url`、`api_formats`、`auth_scheme` 全部可编辑；
3. **自定义**：`provider` 由用户输入，字段全部可编辑。

表单 MUST NOT 再根据 `base_url` 是否偏离预设自动切换模式，MUST NOT 自动改写 `provider`。需要不同端点或不同格式时，MUST 通过显式的「复制为自定义」入口表达。

编辑预设条目时，MUST 额外提供「转为自定义」入口：执行后该条目 `provider` 改为 `<预设key>-custom`（可编辑），端点、格式、鉴权转为可编辑，并 MUST 二次确认。

#### Scenario: 预设端点只读
- **WHEN** 用户选择 `openai` 预设
- **THEN** Base URL 以只读形式展示为 `https://api.openai.com/v1`，格式与鉴权以只读形式展示，仅显示「复制为自定义」入口

#### Scenario: 复制为自定义
- **WHEN** 用户点击「复制为自定义」
- **THEN** 表单切换为自定义模式，`provider` 预填 `openai-custom` 且可编辑，Base URL 预填该预设已展开的地址，格式与鉴权预填预设值且可编辑

#### Scenario: 转为自定义
- **WHEN** 用户编辑一条预设条目并点击「转为自定义」并在确认框确认
- **THEN** `provider` 变为 `<预设key>-custom`，锁定字段转为可编辑，保存后该条目与预设解绑

#### Scenario: 不再自动改名
- **WHEN** 用户在任何模式下修改 `base_url`、格式或鉴权
- **THEN** `provider` MUST NOT 被自动改写，MUST NOT 出现「已脱离预设」类自动提示

#### Scenario: 标识改写即解绑提示
- **WHEN** 用户把 `provider` 从 `<预设key>-custom` 改为其他名称
- **THEN** 表单 MUST inline 提示「修改标识将解除与预设的关联：Base URL 转为自由编辑，变量表单不再可用，转发行为不变」

### Requirement: 端点变量与路径参数录入

当所选预设声明了 `base_url_variables` 或 `path_variables` 时，表单 MUST 为每个参数渲染独立输入项（按目录声明的 `label` 与 `example`），MUST NOT 要求用户手改 URL 或路径字符串。

同名参数（如 `location` 同时出现在主机名与项目路径）MUST 只渲染一次，提交时同时写入 `base_url_vars` 与 `path_vars`。

Base URL MUST 在参数旁以只读形式实时展示展开结果；必填项留空时 MUST 阻止保存并指向缺失参数。`kind` 为 `host-only` / `full-url` 时 MUST 给出填写提示。

`{model}` / `{action}` / `{endpoint}` 属于请求级占位符，MUST NOT 与参数混在同一区域、MUST NOT 可填。表单 MUST NOT 提供 `path_template` 编辑入口。

#### Scenario: Vertex 路径参数
- **WHEN** 用户选择 `google-vertex-anthropic` 预设
- **THEN** 表单展示 `location`（主机名与路径共用）与 `project` 两个输入项，Base URL 仅读展示展开结果

#### Scenario: Azure 部署参数
- **WHEN** 用户选择 `azure` 预设
- **THEN** 表单展示 `resource`、`deployment`、`api_version` 三个输入项

#### Scenario: Azure v1 无部署参数
- **WHEN** 用户选择 `azure-v1` 预设
- **THEN** 只展示 `resource`，不出现 `deployment` / `api_version`

#### Scenario: 参数缺失阻止保存
- **WHEN** `google-vertex-anthropic` 的 `project` 留空并点击保存
- **THEN** 阻止提交并提示需填写 `project`

#### Scenario: 无模板入口
- **WHEN** 表单处于任何模式
- **THEN** MUST NOT 出现路径模板编辑输入框

### Requirement: Provider 健康状态的不可探测态

Provider 列表的健康列 MUST 区分四态：正常、冷却中、已退役、**不可探测**。

不可探测 MUST 用于：凭据类型为 Service Account、模型清单为空、或 `api_formats` 为空的记录。此类记录 MUST NOT 显示为“正常”，MUST 以中性样式标注并提供原因提示（如「该凭据类型不支持健康探测」）。

#### Scenario: Service Account 不可探测
- **WHEN** 列表中存在 Service Account 凭据
- **THEN** 其健康列显示“不可探测”及原因，不显示“正常”

#### Scenario: 空清单或空格式不可探测
- **WHEN** 某凭据模型清单为空或 `api_formats` 为空
- **THEN** 其健康列显示“不可探测”，不显示“正常”

#### Scenario: 健康凭据保持正常
- **WHEN** 某凭据有模型清单、非 Service Account、`api_formats` 非空，且无健康标记
- **THEN** 健康列显示“正常”

### Requirement: AWS 凭据录入

凭据输入 MUST 支持粘贴 AWS 凭据 JSON（含 `access_key_id` / `secret_access_key` / 可选 `session_token`），并 MUST 自动识别为 `aws_sigv4`，MUST NOT 要求用户单独选择凭据类型。

识别为 AWS 凭据后，表单 MUST 只读展示掩码（`aws:<后 4 位>`），并 MUST 提示该凭据类型不支持模型清单拉取与健康探测（「获取模型」MUST 禁用）。

区域 (`region`) MUST 由端点变量输入，MUST NOT 要求重复填写签名区域。

#### Scenario: 粘贴 AWS 凭据
- **WHEN** 用户在凭据输入框粘贴含 `access_key_id` / `secret_access_key` 的 JSON
- **THEN** 表单展示掩码与「AWS Signature V4」提示，「获取模型」禁用，保存后 `credential_kind = aws_sigv4`

#### Scenario: 缺字段提示
- **WHEN** 粘贴的 AWS JSON 缺少 `secret_access_key`
- **THEN** 保存时返回可读错误（`code = invalid_request`），表单保留已填内容

#### Scenario: 区域只填一次
- **WHEN** 用户选择 `amazon-bedrock` 预设
- **THEN** 表单只出现一个 `region` 输入项（用于主机名与签名），不出现第二个签名区域字段

### Requirement: 预设说明的展示

预设选择器中的每个条目 MUST 展示预设名称、key、`kind` 类型徽章与一句话说明。说明优先使用 `summary` i18n 文案，缺省使用 `kind` 通用提示；搜索 MUST 匹配名称、key、`variant_label` 与说明文案。

选中预设后的只读信息块 MUST 展示类型徽章、完整说明、`note`（存在时）与 `doc` 文档链接（存在时），再展示现有 API 格式与锁定提示。

#### Scenario: 搜索命中文案
- **WHEN** 用户输入“套餐”或“coding”
- **THEN** 说明文案命中的编程套餐 / Token 套餐条目出现在结果中

#### Scenario: 文档链接
- **WHEN** 预设带 `doc`
- **THEN** 选中信息块提供可点击的文档链接

#### Scenario: 翻译回退
- **WHEN** 某个 `summary` / `note` 缺少当前语言翻译
- **THEN** 回退到 `kind` 通用提示，不显示裸 i18n key

### Requirement: 套餐与特殊鉴权说明

`coding_plan` / `token_plan` 的提示 MUST 明确说明“需订阅对应套餐、使用套餐专用 Key、普通按量 Key 通常不可用”，MUST NOT 只用“API Key”概括。

特殊鉴权（GitHub Copilot、MiniMax Anthropic 兼容、Kimi Coding、Bedrock / Vertex 托管）MUST 展示对应 `note`。

#### Scenario: 编程套餐说明
- **WHEN** 用户选择 `zai-coding-plan` 预设
- **THEN** 选择器与选中信息块展示“编程套餐”徽章与“需订阅套餐、使用套餐 Key”的说明

#### Scenario: Token 套餐说明
- **WHEN** 用户选择 `xiaomi-token-plan-cn` 预设
- **THEN** 展示“Token 套餐”徽章与套餐专用 Key 说明

#### Scenario: 特殊鉴权说明
- **WHEN** 用户选择 `github-copilot`、`minimax` 或 `bedrock-anthropic` 预设
- **THEN** 选中信息块展示对应的特殊注意事项（Copilot Token / Anthropic 兼容 / 云凭据）

### Requirement: 预设名称按语言展示

控制台 MUST 按当前界面语言展示预设名称：

- `zh-CN`：优先展示 `name_zh`；
- `en-US`：展示 `name`；
- `name_zh` 缺失或为空时 MUST 回退到 `name`。

预设选择器、选中预设信息块与 Provider 列表 MUST 使用同一套本地化名称规则。搜索 MUST 同时匹配 `name` 与 `name_zh`，大小写不敏感。

#### Scenario: 中文环境显示中文名
- **WHEN** 界面语言为 `zh-CN` 且预设带 `name_zh`
- **THEN** 选择器、选中信息块与 Provider 列表展示 `name_zh`

#### Scenario: 英文环境显示英文名
- **WHEN** 界面语言为 `en-US`
- **THEN** 即使预设带 `name_zh`，也展示 `name`

#### Scenario: 搜索双语命中
- **WHEN** 用户在中文界面输入英文名称或中文名称的一部分
- **THEN** 对应预设都能出现在搜索结果中

#### Scenario: 缺失回退
- **WHEN** 预设没有 `name_zh`
- **THEN** 中文界面也回退展示 `name`，不显示空白或裸 key

### Requirement: 桌面自动登录页

控制台 SHALL 提供公开路由 `/desktop`，用于桌面壳注入的自动登录：读取 URL fragment 中的令牌，调用 `POST /api/auth/desktop` 换取会话，成功后进入控制台；成功后 MUST 清除 fragment，令牌 MUST NOT 留在地址栏或历史记录中。

令牌缺失或换取失败时，页面 MUST 展示可读原因并提供「前往登录」入口，MUST NOT 卡在空白或无限加载状态。

桌面壳导航到 `/desktop` 时，前端 MUST 使用 fragment 中的令牌，MUST NOT 依赖任何持久化的凭据。

#### Scenario: 自动进入
- **WHEN** 桌面壳打开 `/desktop#<token>` 且令牌有效
- **THEN** 换取会话成功后进入控制台，地址栏不含令牌

#### Scenario: 令牌缺失
- **WHEN** 用户直接访问 `/desktop` 且没有令牌
- **THEN** 展示缺失说明与「前往登录」按钮

#### Scenario: 换取失败
- **WHEN** 令牌无效或 admin 尚未设置口令
- **THEN** 展示失败原因与「前往登录」按钮，不进入控制台

### Requirement: 登录页的桌面提示

`/desktop` 落地页被桌面壳访问时 MUST 在客户端写入一个本地标记；登录页 SHALL 仅在该标记存在时提示可通过托盘菜单「以管理员自动登录」免密进入。

该提示 MUST 由客户端本地标记驱动，MUST NOT 依赖服务端公开接口或运行模式判断；外部浏览器（从未访问过 `/desktop`）MUST NOT 看到该提示。

「退出登录」MUST NOT 清除该标记（退出后正是需要提示的场景）。

#### Scenario: 桌面壳退出后提示
- **WHEN** 桌面壳访问过 `/desktop`，用户随后在控制台退出登录
- **THEN** 登录页展示托盘自动登录提示

#### Scenario: 外部浏览器无提示
- **WHEN** 外部浏览器直接打开登录页（从未经过 `/desktop`）
- **THEN** 页面不展示任何桌面相关提示

#### Scenario: 独立模式无提示
- **WHEN** standalone 模式下用户打开登录页
- **THEN** 页面不展示桌面相关提示

### Requirement: 桌面应用设置界面（仅 admin）

系统页（`/system`）SHALL 提供「桌面应用」设置区块，包含「开机自启动」与「关闭窗口时最小化到托盘」两个开关。

数据 MUST 来自 `GET /api/app/desktop`；修改 MUST 通过 `PUT /api/app/desktop` 提交，MUST NOT 直接写 `<data_root>/desktop.toml`。

仅当 `available = true`（desktop 模式）时控件 MUST 可编辑；`available = false`（standalone 模式）时 MUST 只展示「该设置仅在桌面应用中可用」的说明，MUST NOT 提供可编辑控件。

界面 MUST 在 5 秒内反映托盘或其他窗口造成的偏好变化。全部文案 MUST 中英文双语。

#### Scenario: 桌面模式下展示设置
- **WHEN** admin 在 desktop 模式打开系统页
- **THEN** 展示两个开关，勾选状态与 `GET /api/app/desktop` 返回的偏好一致

#### Scenario: 独立模式不可编辑
- **WHEN** 服务以 standalone 模式运行，admin 打开系统页
- **THEN** 桌面应用区块只显示不可用说明，没有可操作开关

#### Scenario: 托盘改动同步到控制台
- **WHEN** 用户在托盘关闭「开机自启动」，控制台停留在系统页
- **THEN** 5 秒内开关与状态刷新为关闭

### Requirement: 桌面偏好修改的等待与回滚

提交偏好修改后，界面 MUST 展示「应用中」状态，并按响应中的 `apply_id` 等待回执：

- 回执 `ok = true` 且 `apply_id` 匹配 → MUST 提示成功并刷新为实际生效值；
- 回执 `ok = false` 且 `apply_id` 匹配 → MUST 把开关回滚到 `applied` 的值并展示 `error` 文案；
- 等待超时 → MUST 提示「桌面应用未响应」并重新拉取状态。

等待期间 MUST 禁用开关，MUST NOT 允许重复提交同一修改。

#### Scenario: 修改成功
- **WHEN** admin 关闭「开机自启动」且壳注册注销成功
- **THEN** 开关保持关闭并提示成功，系统登录项被移除

#### Scenario: 注册失败回滚
- **WHEN** 系统拒绝注册登录项
- **THEN** 开关回滚为原值，并展示回执中的失败原因

#### Scenario: 回执超时
- **WHEN** 壳未在等待窗口内写回匹配的回执
- **THEN** 提示「桌面应用未响应」，并按重新拉取的结果显示开关状态

### Requirement: 个人页身份区

控制台 SHALL 在「个人」页的编辑表单之前展示当前登录用户的身份区，至少包含头像、显示名与角色；头衔存在时 MUST 一并展示。

显示名 MUST 取 `display_name`，为空时回落 `username`。登录名 MUST 以 `@<username>` 形式单独展示，MUST NOT 与显示名混排。角色 MUST 以中文标签展示（`admin` → 管理员、`user` → 用户）。

用户保存昵称或头衔成功后，身份区 MUST 即时反映新值，MUST NOT 要求刷新页面。

#### Scenario: 有昵称与头衔
- **WHEN** 用户打开「个人」页且设置了昵称与头衔
- **THEN** 身份区展示「头像 + 昵称 + @登录名 + 角色徽标 + 头衔」

#### Scenario: 未设置昵称
- **WHEN** 用户的 `display_name` 为空
- **THEN** 身份区用 `username` 作为显示名，`@username` 仍单独展示

#### Scenario: 未设置头衔
- **WHEN** 用户的 `title` 为空
- **THEN** 身份区不渲染头衔行，也不留空白占位

#### Scenario: 保存后即时更新
- **WHEN** 用户在同一页保存新的昵称
- **THEN** 身份区的显示名立即变为新昵称，无需刷新

#### Scenario: 角色标签
- **WHEN** 当前用户是 admin / 普通用户
- **THEN** 身份区分别展示「管理员」/「用户」徽标

### Requirement: 侧栏用户区

侧栏底部用户区 MUST 展示当前登录用户的默认头像、显示名与版本号；点击后展开的菜单 MUST 展示头像、显示名、头衔（存在时）与「退出」入口。

侧栏头像 MUST 与个人页身份区使用同一套派生规则，同一用户的头像在侧栏与个人页 MUST 完全一致；修改昵称后两处 MUST 同步更新。

#### Scenario: 侧栏头像
- **WHEN** 已认证用户查看侧栏底部
- **THEN** 显示该用户的头像与显示名，版本号仍在右侧

#### Scenario: 两处一致
- **WHEN** 用户同时打开个人页并查看侧栏
- **THEN** 两处头像的配色与首字完全一致

#### Scenario: 保存后同步
- **WHEN** 用户在个人页保存新昵称
- **THEN** 侧栏与个人页的显示名同时更新，头像配色不变

### Requirement: 确定性默认头像

未提供自定义头像时，控制台 MUST 使用确定性默认头像；该头像 MUST NOT 依赖网络请求、第三方头像服务或新增素材资源。

默认头像 MUST 由「按用户 `id` 派生的配色」与「显示名首字」组成：CJK 取首字，其他取 1–2 个字母；无法取得字符时回落固定符号。

同一用户在不同会话、不同设备 MUST 得到相同配色与首字；修改昵称 MUST NOT 改变配色。

默认头像在浅色与深色主题下 MUST 保持首字可读（白字与底色的对比度不低于 4.5:1），并 MUST 提供可读的可访问名称。

#### Scenario: 确定性
- **WHEN** 同一用户重新登录并打开「个人」页
- **THEN** 头像是相同的配色与首字

#### Scenario: 昵称不改变配色
- **WHEN** 用户只修改昵称
- **THEN** 头像配色不变，仅首字随新昵称变化

#### Scenario: 中文显示名
- **WHEN** 显示名为「张三」
- **THEN** 头像展示首字「张」

#### Scenario: 拉丁显示名
- **WHEN** 显示名为 `john.doe` 或 `admin`
- **THEN** 头像分别展示 `JD` / `AD`

#### Scenario: 可访问名称
- **WHEN** 使用读屏软件聚焦头像
- **THEN** 读出「<显示名> 的头像」

### Requirement: 首次登录快速开始

控制台 SHALL 提供 `/welcome` 页面，由**概念导语**与**可判定步骤**两部分组成。步骤 MUST 分为两组：「接入」组对所有角色展示，「运维」组仅对 admin 展示并标注可稍后完成。

完成态与进度 MUST 只由「接入」组决定；「运维」组 MUST NOT 阻塞完成态，其进度 MUST 单独展示。

自动进入 MUST 同时满足：用户已认证、当前用户未归档路由数为 `0`、本机不存在该用户的「已跳过 / 已完成」标记。自动进入 MUST 使用 `replace` 导航，且同一次页面会话内 MUST 只强制一次；用户主动离开 `/welcome` 后 MUST NOT 被弹回。

「跳过」入口 MUST 始终可见、单击即生效、MUST NOT 二次确认，并 MUST 持久化到本机且按用户维度隔离；持久化 MUST NOT 写入服务端。

步骤完成状态 MUST 由实时接口数据推导，SHALL NOT 仅依据本机记录；用户删除已创建的路由、上游凭据或 API Key 后，对应步骤 MUST 回到未完成。

步骤 MUST 提供直达入口：创建路由 MUST 复用控制台外壳上的创建对话框；「添加上游账号」MUST 进入该路由详情并自动打开新增凭据对话框；「创建 API Key」MUST 进入 API Keys 页并自动打开创建对话框；自动打开的对话框 MUST 在打开后清除 URL 上的触发参数。

「接入」组全部完成后页面 MUST 展示完成态、写入本机完成标记，并 MUST 提供前往用量页的入口。`/welcome` MUST NOT 阻塞任何其他页面的访问。

#### Scenario: 首次登录自动进入
- **WHEN** 用户登录且当前没有任何未归档路由，本机也无该用户的跳过或完成标记
- **THEN** 控制台自动进入 `/welcome`，地址以 `replace` 方式变更，可回退到登录前页面之外的其他页面

#### Scenario: 已有路由不自动进入
- **WHEN** 用户已有至少一条未归档路由
- **THEN** 登录后停留在 `/routes`，不自动进入 `/welcome`

#### Scenario: 跳过
- **WHEN** 用户在 `/welcome` 点击「跳过」
- **THEN** 立即进入路由总览页并写入本机「已跳过」标记，刷新或重新登录后都不再自动进入

#### Scenario: 主动离开不弹回
- **WHEN** 用户未跳过、从 `/welcome` 点击侧栏其他入口
- **THEN** 停留在目标页面，本次会话内不再被强制带回 `/welcome`

#### Scenario: 完成判定来自实时数据
- **WHEN** 用户创建过 API Key 后又将其删除，然后回到 `/welcome`
- **THEN** 「创建 API Key」步骤重新显示为未完成，进度条相应回退

#### Scenario: 创建路由直达
- **WHEN** 用户在 `/welcome` 点击「创建路由」
- **THEN** 控制台外壳上的创建路由对话框直接打开，无需先跳转页面

#### Scenario: 上游凭据直达
- **WHEN** 用户在 `/welcome` 点击「添加上游账号」
- **THEN** 进入当前用户的一条路由详情页并自动打开「新增 Provider」对话框，URL MUST 不含残留的触发参数

#### Scenario: API Key 直达
- **WHEN** 用户在 `/welcome` 点击「创建 API Key」
- **THEN** 进入 `/api-keys` 并自动打开创建对话框，URL MUST 不含残留的触发参数

#### Scenario: 角色差异
- **WHEN** admin 打开 `/welcome`
- **THEN** 在「接入」组之外额外展示「运维」组（对外访问地址、员工账号、告警 Webhook）

#### Scenario: 普通用户无运维组
- **WHEN** 普通用户打开 `/welcome`
- **THEN** 只展示「接入」组，MUST NOT 展示任何管理员专属步骤

#### Scenario: 接入组完成
- **WHEN** 「接入」组 4 步全部判定为完成
- **THEN** 页面展示完成态与前往用量页的入口，并写入本机完成标记，之后不再自动进入

#### Scenario: 运维组未完成不影响完成态
- **WHEN** admin 的「接入」组已完成但「运维」组仍有未完成步骤
- **THEN** 页面仍展示完成态，且「运维」组继续可见并单独展示其完成数

### Requirement: 帮助中心

控制台 SHALL 提供 `/help` 页面，包含**概念**、**常见任务**、**接入示例**、**故障排查**、**我的环境**、**FAQ** 六个分区。每个分区与条目 MUST 具有稳定锚点，`/help#<anchor>` MUST 直接定位到该分区 / 条目并使其展开。

接入示例 MUST 复用路由详情页相同的示例生成逻辑，SHALL NOT 在页面内维护第二份示例代码；示例中的接入地址 MUST 优先取 `/api/access` 配置的对外地址，未配置时回落当前访问地址，且 MUST 与路由详情页展示的地址一致。

「我的环境」MUST 只读展示：版本、接入地址、当前访问地址与异常凭据汇总。对 admin MUST 额外展示运行模式、监听地址与最近未关闭的故障事件（对应接口仅 admin 可读，非 admin MUST NOT 发起这些请求）。该分区 MUST NOT 提供任何写操作。

页面 MUST 提供检索框，按分区标题与条目关键词做前端过滤；清空检索 MUST 恢复全部分区。

页面全部用户可见文案 MUST 来自 i18n 资源。

#### Scenario: 锚点直达
- **WHEN** 其他页面链接到 `/help#trouble-403`
- **THEN** 页面滚动到该条目并展开所属分区，其余分区不展开

#### Scenario: 目录导航
- **WHEN** 用户点击目录中的某个分区
- **THEN** 页面定位到该分区并把锚点写入 URL，可复制分享

#### Scenario: 示例一致性
- **WHEN** admin 修改了对外访问地址并重新打开帮助页
- **THEN** 帮助页与路由详情页展示的接入地址、示例代码保持一致

#### Scenario: 环境诊断
- **WHEN** 用户打开「我的环境」
- **THEN** 展示版本、接入地址、当前访问地址与异常凭据汇总，且均为只读

#### Scenario: 非 admin 隐藏运维字段
- **WHEN** 普通用户打开「我的环境」
- **THEN** 不展示运行模式、监听地址与故障事件列表，也不发起获取这些信息的请求

#### Scenario: admin 额外诊断
- **WHEN** admin 打开「我的环境」
- **THEN** 额外展示最近未关闭的故障事件列表

#### Scenario: 检索命中
- **WHEN** 用户在帮助页输入某个关键词（如 `403`）
- **THEN** 只保留标题或关键词命中的分区与条目

#### Scenario: 检索清空
- **WHEN** 用户清空检索框
- **THEN** 恢复展示全部分区

#### Scenario: 帮助入口可达
- **WHEN** 已认证用户浏览任意页面
- **THEN** 侧栏「帮助」分组与顶部栏帮助入口都能进入快速开始与帮助中心
