# app-bundled-gateway

## Why

Linux 用户装完客户端启动就报错：

```txt
未找到 solidway 可执行文件。已尝试：
  - /usr/bin/solidway
  - /usr/bin/solidway.exe
  - /usr/bin/../../../target/debug/solidway
  - /usr/bin/../../../target/release/solidway
  - PATH 各目录下的 solidway
```

根因是此前把「Linux 客户端不内嵌网关、依赖服务包提供 `/usr/bin/solidway`」当作取舍（ADR 0018 决策 5、`packaging/README` 的安装顺序说明）。工程上省了几 MB，用户体验上等于交付了半个产品：只装客户端必然坏；AppImage 这种「下载即用」形态更是直接失效。README 不能替代安装器行为。

另外顺手发现两个必须一起修的问题：

1. 客户端 deb 的 `Package:` 是 `solidway`，与服务包**同名** —— 两者无法共存（dpkg 会当成同一个包的升级，把服务包的 systemd unit 与 `/usr/bin/solidway` 删掉）。
2. 定位失败时的候选列表里混着 `../../target/...` 这类开发期路径，正式包用户看到的是噪音。

## What Changes

1. 客户端包**自带** `solidway` 二进制：Linux 用 `bundle.resources`（落点 `/usr/lib/<产品名>/`，AppImage 为 `$APPDIR/usr/lib/<产品名>/`）；Windows 已有（安装目录）；mac 保持 post-copy 进 `Contents/MacOS`。
2. 桌面壳的定位顺序改为：**可执行文件同目录 → 应用资源目录（`resource_dir()`）→ 开发期仓库 target（仅 debug）→ PATH**。资源目录由 Tauri 给出，三平台语义一致，不再靠字符串猜路径。
3. 发布构建里不再列出开发期候选；失败信息只包含真实尝试过的位置。
4. Linux 客户端 deb 的包名改为 **`solidway-app`**（与 `mainBinaryName` 一致），避免与服务包 `solidway` 冲突；文件名同步为 `solidway-app_<ver>_<arch>.deb`。
5. 服务包降级为**可选**：需要 systemd 常驻时照旧安装；只想要桌面端的用户单装客户端即可，两者同时安装时客户端优先用自带的资源副本。

不修改：`solidway` 自身的运行行为、服务包（`scripts/package-linux.sh`）内容、Windows/mac 产物的既有布局、`data_root` 解析。

## 范围外

- 把网关二进制改成 Tauri `externalBin`（sidecar）命名带 target triple 的方案：会打断开发期构建（archive 里已记录该取舍）。
- 自动下载/自更新网关二进制（客户端已自带，无需网络）。
- Windows/mac 的代码签名（
  ADR 0018 已挂账）。

## 验证要求

- `cargo test --manifest-path app/Cargo.toml`：新增定位顺序单测（同目录优先、资源目录次之、dev 仅 debug、PATH 兜底、重复目录去重）全绿。
- `SKIP_WEB_BUILD=1 ./scripts/package-linux-app.sh`：产物 `dist/linux-app/solidway-app_<ver>_<arch>.deb`（`dpkg-deb -f … Package` = `solidway-app`）与 AppImage 内均含 `usr/lib/Solidway/solidway`。
- 把 deb 解包到临时前缀直接运行（系统无 `/usr/bin/solidway`）：应用能启动自带的网关（`<data_root>/solidway.log` 出现 `listening` 或 `bind-failed`，说明子进程确实被拉起）。
