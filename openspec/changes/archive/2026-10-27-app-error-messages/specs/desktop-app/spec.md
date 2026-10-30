# desktop-app Specification (delta: app-error-messages)

## ADDED Requirements

### Requirement: 启动失败的呈递

应用在启动失败时 MUST 按失败分类向用户呈现三段内容：**结论标题**、**可执行的建议**、**日志末尾**；MUST NOT 只呈现技术错误串。

分类与建议 MUST 至少覆盖：

- 找不到 `solidway` 可执行文件 → 提示「安装可能不完整」，并说明应重新下载/安装；
- 找到但无法执行（缺权限、架构或依赖不符）→ 提示需重新安装或改用匹配的安装包；
- 端口被占用 → 展示占用者（comm，pid 可读时一并展示），并提示关闭占用程序后重试；占用者为另一个 `solidway` 时 MUST 说明可能是上次未正常退出；
- 端口无权限（`permission-denied`）→ 提示改用 1024 以上端口或调整运行权限；
- 子进程异常退出 / 握手超时 → 展示失败原因与日志末尾。

日志末尾 MUST 取 `<data_root>/solidway.log` 的最后若干行，去除 ANSI 控制序列并限制长度；日志不存在时 MUST 省略该段，MUST NOT 显示为空白的代码块。

应用 MUST NOT 因端口冲突自动结束其他进程，MUST NOT 自动更换监听端口。

#### Scenario: 安装不完整
- **WHEN** 应用在可执行文件同目录、应用资源目录、开发期目录与 `PATH` 中都找不到 `solidway`
- **THEN** 错误页标题为「安装可能不完整」，建议中包含重新下载/安装的指引，并保留已尝试路径供技术排查

#### Scenario: 程序不可执行
- **WHEN** 找到 `solidway` 但系统以权限不足或其他非「不存在」原因拒绝启动
- **THEN** 错误页提示需重新安装或检查权限/架构，不误导用户去重新下载

#### Scenario: 端口被占用
- **WHEN** 服务端发出 `bind-failed` 且 `kind` 非 `permission-denied`
- **THEN** 错误页说明端口已被占用、展示占用者信息、建议关闭占用程序后重试，且附日志末尾

#### Scenario: 端口无权限
- **WHEN** `bind-failed` 的 `kind` 为 `permission-denied`
- **THEN** 错误页提示改用 1024 以上的端口或调整运行权限

#### Scenario: 异常退出带日志
- **WHEN** 子进程异常退出或握手超时
- **THEN** 错误页展示退出原因，并内嵌日志最后 20 行（已去除 ANSI 颜色码）

#### Scenario: 无日志时不显示空块
- **WHEN** `<data_root>/solidway.log` 不存在或为空
- **THEN** 错误页不渲染日志区域，只显示标题与建议
