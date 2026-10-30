# app-open-console-keeps-detail

## Why

Linux 用户实测：客户端先因为「找不到 solidway」失败，随后点托盘「打开控制台」，错误页却变成了 `尚未获知监听地址（未收到 listening 事件）`——最没信息量的那句把真实原因覆盖掉了，导致排查方向跑偏（以为是打包/事件问题，实际是二进制缺失 + 端口被占）。

根因在 `open_console_now()`：没有任何可导航地址时无条件调用 `failed(...)`，而 `failed()` 会 `set_state(Failed, "启动失败", Some(通用文案))`，把 `inner.detail` 里已经写好的具体原因（未找到二进制 / 端口冲突 / 启动超时）冲掉。另外「子进程仍在握手时点打开控制台」也会被误判为失败。

## What Changes

1. `open_console_now()` 缺地址时按状态分流（纯函数 `open_action`，可单测）：
   - 启动中 → 只展示启动页，**不判失败**；
   - 已失败 / 已停止且已有原因 → 展示状态页，**保留原因**；
   - 既无地址也无原因 → 才标记失败，且提示里带上 `solidway.log` 路径。
2. 通用失败提示补充日志路径，便于一步定位。
3. 单测覆盖四种分流。

不修改：`console_target` 的导航优先级、`bind-failed` 事件解析与端口占用文案（已含 `holder_pid` / `holder_comm`）、健康探测与重启策略。

## 验证要求

- `cargo test --manifest-path app/Cargo.toml` 通过（新增 `open_action_keeps_failure_detail`）。
- 手工：故意让子进程找不到（PATH 与资源目录均无 `solidway`），点托盘「打开控制台」，错误页仍显示「未找到 solidway 可执行文件」而不是通用提示；启动中点击则停在启动页。
