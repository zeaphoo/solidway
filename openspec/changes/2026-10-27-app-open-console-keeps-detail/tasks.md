# Tasks — app-open-console-keeps-detail

- [x] 1.1 `app/src/supervisor.rs`：新增纯函数 `open_action(has_target, state, has_detail)` 与 `OpenAction` 四态
- [x] 1.2 `open_console_now()` 缺地址时按判定分流：启动中 → 启动页；有原因 → 状态页（保留 detail）；无原因 → 失败
- [x] 1.3 通用失败提示补充 `<data_root>/solidway.log` 路径
- [x] 1.4 单测 `open_action_keeps_failure_detail`（17/17 通过）
