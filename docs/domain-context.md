# Domain Context — 领域术语表

> 项目专属通用语言（ubiquitous language）。只放领域概念，不放通用编程概念。新术语固化时立即更新。

## 术语

| 术语 | 定义 | _Avoid_（禁用别名） |
|---|---|---|
| 网关（gateway） | 本项目本体：自托管的 LLM 接入与账号池化设施 | 中转站、relay、代理站 |
| 限额域（quota domain） | 池化单位：上游额度生效的边界（organization / project 级），key 只是域的凭据 | 账号池、号池 |
| 保真转发（forward unchanged） | 请求体、响应体与流式事件原样透传，仅鉴权头与目标地址由网关改写 | 0 字节改写 |
| 换号 | 首个字节返回之前，将请求切换到**同一路由内**另一 Provider / 限额域重发整轮请求 | 重试、续传 |
| 政策处置 | 上游基于条款对账号做出的封禁 / 停用等处置；api_key 失效（401）按永久退役隔离、异常未确定（403 等）按临时不可用换号，均不无限自动切换 | 封号恢复 |
| 路由（route） | 对消费方暴露、按名寻址的 LLM 服务单位，由一组 Provider 共同提供 | 连接池、pool、服务池 |
| Provider（provider key） | 挂在某条路由下的一条上游接入，含 provider、base_url、协议格式与加密后的 api_key；控制台统一显示为「Provider」，代码与表名沿用 provider_key | 凭据、连接、账号、渠道 |
| 上游 API 根（upstream API root） | Provider `base_url` 的语义：上游服务的 API 根路径，端点路径在其后拼接；与入站家族根无关，也不等于完整请求地址 | base、上游地址、接口地址 |
| 版本段（version segment） | 上游 URL path 中的 API 版本段（`v1` / `v1beta` / `v1alpha`）；anthropic 族的 `base_url` 缺版本段时由网关在转发目标构造中补齐 `/v1`，不回写用户输入 | 版本号、api version |
| 模型自动拉取（model auto-discover） | 创建 Provider 且模型清单为空时，服务端在创建路径内自动执行的一次性模型清单获取（失败不阻断创建）；与手动「获取模型」区分：后者由用户显式触发、可重试 | 自动同步、定时拉取、模型发现任务 |
| 模型别名（model alias） | Provider 上声明的「客户端模型名（`name`）→ 上游模型 id（`upstream`）」映射；两者不同时，网关只改写请求体与响应体的**顶层** `model` | 模型重写、模型路由 |
| 选号（provider selection） | 在一条路由的候选 Provider 中挑出本次尝试的顺序；与「换号」区分：选号决定先试谁，换号决定失败后重试谁 | 负载均衡、调度 |
| provider 预设（provider preset） | 网关内置的常见 provider 目录项，描述协议族、默认 base_url、鉴权方式与类型/说明元数据；只收录三种入站格式可直连的上游 | 适配器、适配层 |
| 预设类型（preset kind） | 预设的计费/接入形态：`api`（按量 API）、`coding_plan`（编程套餐）、`token_plan`（Token 套餐）、`subscription`（订阅制）、`local`（本地推理）、`enterprise`（云厂商托管）、`gateway`（聚合/代理网关）；不是协议族 | 分类、套餐类型（单独使用时含义过宽） |
| 预设说明（preset summary / note） | 预设的中英文说明元数据：`summary` 为一句话用途，`note` 为特殊鉴权或套餐限制；均为 i18n key，不在目录数据里写死语言 | 描述、备注（单独使用时含义过宽） |
| 预设名称（preset name） | 预设的规范英文名 `name` 与中文显示名 `name_zh`；`name` 始终保留为英文回退，`name_zh` 仅用于中文界面展示 | 翻译名、别名 |
| 端点模板（base_url template） | 预设声明的 URL 模板，可含 `{name}` / `${NAME}` 占位符；写入时展开为绝对地址后落库，运行期不再替换 | 默认地址、URL 模板变量 |
| 端点变量（base_url variable） | URL 模板中的占位符取值，存于条目 `options.base_url_vars`；按 `host-label` / `path-segment` / `host-only` / `full-url` / `gcp-location` / `gcp-project` / `azure-api-version` 校验 | URL 参数、替换项 |
| 路径参数（path parameter） | 上游路径模板中的凭据级占位符（`project` / `location` / `deployment` / `api_version`），存于 `options.path_params`，只由 UI 收集、随凭据冻结；`model` / `action` / `endpoint` 属请求级占位符，不是路径参数 | 路径变量、路径模板 |
| 预设条目（preset-bound provider） | `provider` 等于目录 key 的 Provider：端点、API 格式、鉴权方式由预设决定并锁定，仅端点变量可填 | 内置 Provider、系统 Provider |
| 复制为自定义（copy as custom） | 以某预设为起点新建独立条目：`provider` 默认 `<预设key>-custom`（可改），端点与格式自由；改名即解绑预设 | 克隆 Provider、另存为 |
| 转为自定义（convert to custom） | 把已有预设条目就地解绑为自定义条目（破坏性、需确认）；转发行为不变，失去模板变量表单 | 降级为自定义、脱离预设 |
| 上游路径模板（path template） | 预设声明的路径骨架，含凭据级路径参数与请求级 `{model}` / `{action}` / `{endpoint}`；写入时冻结到凭据，运行期据此渲染目标地址。`rawPredict` 动作同时决定正文形状 | 适配器、协议、path_template 手填 |
| 不可探测（unprobeable） | 无法发起健康探针的凭据状态：Service Account、模型清单为空、`api_formats` 为空；不得呈现为健康 | 未知健康、无健康记录 |
| AWS 凭据（aws_sigv4） | provider key 的一种凭据形态：JSON 含 `access_key_id` / `secret_access_key` / 可选 `session_token`，按 AWS Signature V4 对每个转发请求签名；不支持模型清单拉取与健康探测 | AWS key、AccessKey |
| SigV4 签名（sigv4） | 用 AWS 凭据对请求 method/path/query/headers/body 计算 `AWS4-HMAC-SHA256` 签名，scope 为 `<date>/<region>/<service>/aws4_request` | 签名鉴权、AWS 鉴权 |
| API Key（接入凭据） | 客户端调用**网关**所用的凭据，绑定唯一一个路由；网关按哈希校验，明文仅创建时可见一次 | 密钥、token、上游 key |
| 路由归属（route owner） | 路由的私有归属用户；只有 owner 能查看 / 编辑该路由及其 Provider、API Key 与用量 | 路由所属人、租户 |
| 私有路由（private route） | 仅 owner 可见的路由；admin 也不能通过路由管理面访问其他用户的路由 | 全局路由、公共路由 |
| 承接 Provider（serving provider） | 最终成功承接某个逻辑请求的 Provider；token 用量归属于它 | 成功账号、实际渠道 |
| 用量缺失（usage missing） | 请求正常完成、但上游响应中没有可解析的 token usage；统计上单独计数，不做估算 | 未知 token、0 token |
| 选号策略（selection strategy） | 路由级配置：决定同一条路由内多个 Provider 的尝试顺序（`round_robin` / `uptime` / `latency` / `auto`）；只影响选号，不影响转发字节 | 路由策略、负载均衡策略、routing |
| Provider 优先级（provider priority） | Provider 凭据上的整数**档位**（`1..=1000`，默认 1）：数字越大越优先，按降序分档，档内再按选号策略排序；不进分数。「暂时不参与选号」用停用表达，不用档位 | 权重、weight、0 档 |
| 选号指标（live metrics） | 数据面内存中每个 Provider 的首字节时延与失败率 EWMA（30 分钟陈旧判定），不落库 | 健康度、成功率统计 |
| 故障记录（incident） | Provider 一次故障的生命周期：原因码、严重级别、开始 / 最近 / 结束时间、次数；与「当前状态」互补，回池不删除 | 故障日志、告警历史 |
| 故障严重级别（incident severity） | `transient`（限流 / 5xx 抖动，只记录）/ `degraded`（需关注：配额耗尽、403）/ `manual_action`（需人工：401） | 告警等级、优先级 |
| 全局统计（global usage） | 仅 admin 可见的全站只读用量聚合，不提供路由配置访问 | 全站统计、超管统计 |
| 归档（archive） | 重资源的**不可恢复**逻辑删除：立即停止服务 / 禁止登录并移出默认列表；Route 释放名称并销毁 Provider / API Key，但保留 Route 行、历史与身份；与「停用」不同，停用只是可逆的服务开关 | 软删除、disable、注销 |
| 停用（disable） | 可逆的服务 / 登录开关：Route 不参与数据面但仍保留；User 不能登录但保留身份与归属 | 禁用、关闭 |
| 历史归属（historical attribution） | 请求发生时固化的 owner 快照；Route 转移不追溯修改历史用量归属 | 当前归属、实时归属 |
| 资源墓碑（tombstone） | 轻资源物理删除或重资源物理清除后保留的最小身份记录（名称、父级、owner 快照等），不含任何密钥材料；历史查询据此回退标签 | 删除日志、回收站 |
| Offboard | 用户离职流程：把其全部活跃 Route 转移给继任者，再归档用户并撤销会话 | 交接、注销流程 |
| 桌面偏好（desktop preferences） | 桌面壳的两个生命周期开关：开机自启动、关闭窗口时最小化到托盘；存于 `<data_root>/desktop.toml`，壳唯一写、服务只读 | 应用设置、客户端配置 |
| 静默启动（hidden start） | 以 `--hidden` 启动桌面壳：不主动展示任何窗口（含 splash 与控制台），状态只在托盘；失败仍展示错误页 | 后台启动、无界面启动 |
| 桌面自动登录（desktop auto-login） | 桌面壳用服务签发的进程级令牌（`desktop-login` 事件）把控制台自动登录为 admin；进程内可复用、重启轮换。桌面模式**不引导设口令**，口令是可选项（见 ADR 0017） | 免密码、绕过登录 |
| 桌面偏好回执（desktop apply receipt） | 壳执行控制台下发的偏好后写入 `<data_root>/desktop.status.json` 的结果（`apply_id` / `ok` / `error` / `applied`），控制台据此确认或回滚 | 执行结果、ACK |

## 已弃用 / 已删除术语

- `provider_keys.weight`（权重）：**schema 中已不存在**。它从未被数据面读取；语义由「Provider 优先级（provider priority）」取代。
- `provider_keys.sort_order`（排序）：**schema 中已不存在**。它只影响管理面列表顺序，与选号无关；顺序统一由 `priority` 表达。
- `provider_keys.protocol`（协议列）：**schema 中已不存在**。协议族与端点形态分别由 `api_formats` 与 `path_template` 表达（ADR 0011）；旧值域把两者混在一起，无法自解释。
- 目录字段 `default_base_url`：**已改名**为 `base_url_template`（端点模板化，见 ADR 0011）。
- 「偏离预设即自动改名 `<key>-custom`」：**已移除**；改为 `provider` 显式「复制为自定义 / 转为自定义」。

## 待定术语

- （暂无。消费方身份已由「API Key（接入凭据）」承担，原「脚本级身份」待评估事项关闭。）
