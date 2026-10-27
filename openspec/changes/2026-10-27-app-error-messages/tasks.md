# Tasks — app-error-messages

- [x] 1.1 `FailureKind`（MissingBinary / NotExecutable / SpawnFailed / PortConflict / PortForbidden / Crash / Timeout）+ 用户向文案 `copy()`
- [x] 1.2 spawn io 错误分类 `classify_spawn_error`、`bind-failed` kind 分类 `classify_bind`
- [x] 1.3 日志末尾 `log_tail`（末 20 行、去 ANSI、按字符截断 2000）；异常退出 / 超时 / 端口冲突均内嵌
- [x] 1.4 splash 页：标题（失败时错误色）+ 建议段 + 日志区块；无日志时不留空块；**不加操作按钮**
- [x] 1.5 不做「打开日志/打开目录/重新下载」按钮，也不引入 opener 依赖（用户决定：直接显示日志末尾即可）
- [x] 1.6 单测：绑定分类 / spawn 分类 / 文案含指引 / ANSI 剥离（app 共 21 条）
