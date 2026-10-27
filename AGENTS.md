# Agent 核心行为准则与记忆体系

本 Agent 采用 **三层认知模型**，确保工程规范、领域语言与业务需求严格解耦，避免上下文漂移与逻辑冲突。

## 一、认知模型（最终定义）

### 1. Memory Bank（工程事实层）

- **定义：**
  Memory Bank 是项目工程规范与技术上下文的唯一存储载体。

- **职责：**
  - 存储编码规范、架构模式、协作流程
  - 定义系统"如何实现"（how）
  - 提供稳定、可演进的工程上下文

- **边界：**
  - 严禁存储业务需求、功能逻辑或系统行为（归属 OpenSpec）
  - 不得与 OpenSpec 内容重叠

- **存储结构：**
  - `@docs/project-brief.md` — 项目定位、阶段目标、已确认方向与边界约束（不写业务行为）
  - `@docs/system-patterns.md` — 当前已采用的工程模式**事实快照**（"我们怎么做"），只记模式是什么，不记为何选它（动机在 `docs/adr/`）
  - `@docs/tech-context.md` — 当前技术栈、依赖、运行时与部署假设的**事实状态**（"我们用什么、跑在哪"），偏静态技术构成而非设计模式
  - `@docs/future-roadmap.md` — 远期规划、待评估想法与未排期需求（"我们未来可能做什么/怎么演进"）
  - `@CONTEXT-MAP.md`（领域术语表入口，指向 `docs/domain-context.md`）
  - `@docs/domain-context.md`（领域术语表正文，通用语言）
  - `@docs/adr/`（架构决策记录，ADR）

  > `system-patterns` 与 `tech-context` 的区别：前者记录**设计层面的工程模式与约定**（如容器隔离、hub/cell 拆分、显式 CLI 入口），回答"系统如何组织"；后者记录**技术层面的栈与运行时事实**（如 uv 管理依赖、Podman 镜像、SQLite 持久化路径、网络信任前提），回答"用什么技术、跑在什么环境"。两者都是事实层，不含决策动机。

- **更新触发（必须执行）：**

  当出现以下情况时，必须更新 Memory Bank 或其领域模型层：
  - 新的代码规范或约定形成 → 更新 `docs/system-patterns.md`
  - 重复工程模式被验证 → 更新 `docs/system-patterns.md`
  - 关键架构或实现路径发生变化 → 更新 `docs/system-patterns.md`
  - 暂时不做，未来可能做的事情 → 更新 `docs/future-roadmap.md`
  - 术语冲突 / 模糊 / 新术语固化 → 立即更新 `docs/domain-context.md`（来自 domain-modeling 纪律，不批量积压）
  - 不可逆、有取舍、未来读者会疑惑"为何这样"的架构决策 → 立即落成 `docs/adr/` 的 ADR
  - `docs/system-patterns.md` 仅保留当前已采用模式的事实快照，决策动机不回写于此
  - 用户执行 `update memory bank`

### 2. 领域模型层（通用语言与决策底座）

由 domain-modeling skill 维护，是 Memory Bank 与 OpenSpec 共享的词汇与决策底座。

- **`CONTEXT-MAP.md` + `docs/domain-context.md`（通用语言表）**
  - 术语正文位于 `docs/domain-context.md`；根目录 `CONTEXT-MAP.md` 是 skill 识别的入口，指向该文件。
  - 定义项目专属术语及其 `_Avoid_` 别名，仅记录"是什么"。
  - 是 Memory Bank 与 OpenSpec 共同引用的词汇基线；两者出现术语冲突时必须回查此处。
  - 边界：只放项目专属领域概念，不放通用编程概念（timeout、error 等）。

- **`docs/adr/`（架构决策记录）**
  - 记录"为何这样选、否决了什么"的不可逆且有取舍的架构决策。
  - 格式遵循 domain-modeling skill 的 ADR-FORMAT，序号 `0001-slug.md` 递增。
  - 边界：仅承载决策动机，不重复记录模式本身（模式事实留在 Memory Bank 的 `docs/system-patterns.md`）。

- **维护纪律（来自 domain-modeling skill，常驻执行）：**
  - 术语冲突 / 模糊 / 新术语固化 → 立即更新 `docs/domain-context.md`，不批量积压。
  - 不可逆、有取舍、未来读者会疑惑"为何这样"的架构决策 → 立即落成 `docs/adr/` 的 ADR。
  - `docs/system-patterns.md` 仅保留当前已采用模式的事实快照，决策动机不回写于此。

### 3. OpenSpec（业务事实层）

- **定义：**
  OpenSpec 是系统行为与业务逻辑的**唯一事实来源（SSOT）**。

- **初始化：**
  新仓库初始化时必须执行 `openspec init`，生成 `openspec/` 目录结构（`config.yaml`、`specs/`、`changes/`）。

- **职责：**
  - 定义系统"做什么"
  - 描述业务规则、行为与约束

- **强约束（未命中熔断）：**

  若在 `openspec/specs/` 中未找到对应定义：

  > **禁止实现功能，禁止推测需求，必须执行：**
  >
  > 1. 创建 Proposal
  > 2. 写入 `openspec/changes/<change-name>/`
  > 3. 经确认后再开发

## 二、初始化执行顺序（新 Session 强制）

每次进入新会话，必须严格执行：

1. **加载 Memory Bank**
   - `@docs/project-brief.md`
   - `@docs/system-patterns.md`
   - `@docs/tech-context.md`

2. **加载领域模型层（词汇基线）**
   - `@CONTEXT-MAP.md` → `@docs/domain-context.md`
   - 涉及术语校验时以 `docs/domain-context.md` 为准

3. **读取 OpenSpec**
   - `openspec/specs/`

> 禁止在未读取 Spec 的情况下定义业务逻辑。

## 三、响应与协作规范

### 1. 语言与格式

- 用中文沟通，用中文思考，用中文写文档，用中文做计划，中文优先
- 中英文混排必须添加空格
- Spec 使用标准术语（`MUST` / `SHALL` / `ADDED` 等）
- 运行日志（`tracing`）文案用英文、结构化字段保持 `key=value`；界面文案与文档保持中文；stdout 事件契约（`solidway: …`）必须 ASCII 且不经 `tracing`
- Commit 遵循 Conventional Commits（如 `feat: xxx`）

### 2. 风格

- 专业、技术导向、简洁
- 优先结构化表达，避免冗余
- 注释语言与代码库保持一致
- 每次变更需附带原则说明（如：符合 KISS）

## 四、核心行为准则

### 1. 权限控制

#### Git（强限制）

- 无明确指令，禁止执行任何写操作：
  - `commit` / `push` / `branch` / `merge`

#### 高风险操作确认

以下操作必须获得明确确认：

- 文件删除 / 批量修改
- 系统配置变更
- 数据库操作
- 调用生产 API
- 依赖变更

**确认格式：**

~~~txt
⚠️ 危险操作！
操作类型：[具体操作]
影响范围：[详细说明]
风险评估：[潜在后果]

请确认是否继续？（回复："是"、"确认"或"继续"方可执行）
~~~

### 2. 开发原则（必须体现）

- **KISS：** 优先最简单方案
- **YAGNI：** 不实现未明确需求
- **DRY：** 消除重复逻辑
- **SOLID：** 保持良好设计

### 3. 执行纪律

- 先读后写
- 基于事实，不做假设
- 持续执行直到问题解决
- 未读取 Spec 前禁止开发

## 五、OpenSpec 结构

~~~txt
openspec/
├── config.yaml          # init 生成（schema: spec-driven），保持默认即可
├── specs/               # 当前系统行为（唯一事实来源），支持嵌套 <area>/<capability>/spec.md
│   └── <domain>/
└── changes/             # 变更提案（唯一入口）
    ├── <change-name>/   # 创建变更时脚手架：proposal.md / design.md(可选) / tasks.md / specs/
    └── archive/         # 已归档变更
~~~

## 六、不可违背原则

- 工程规范只存在于 Memory Bank
- 业务定义只存在于 OpenSpec
- 领域术语只存在于 `docs/domain-context.md`（经 `CONTEXT-MAP.md` 入口），决策动机只存在于 `docs/adr/`（不回写 `system-patterns.md`）
- 未定义需求 → 必须先 Proposal
- 无明确指令 → 不执行 Git 写操作

## 七、本项目特别注意

- Rust 为主的项目，采用 cargo workspace 管理，代码统一放到 `crates/` 目录下
- 前端在 `web/` 目录，技术栈为 pnpm + vite + react + tailwind + shadcn/ui + zustand，使用 JS（不用 TS）
- Rust 代码中使用 sqlx 管理与连接 SQLite，数据库迁移采用 sqlx migration 方式
- Web 服务采用 axum 的最新版本（升级前与主管确认）
- 使用 /grilling 技能时，可以一次提出 3 到 5 个关联的问题，提升沟通效率
- 在 Code Block 输出 Markdown 时，最外层使用 ~~~ 避免嵌套混乱
