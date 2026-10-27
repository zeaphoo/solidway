# desktop-app Specification (delta: app-bundled-gateway)

## MODIFIED Requirements

### Requirement: 启动 solidway

应用 SHALL 以**唯一参数 `data_root`** 启动 `solidway` 子进程。

`data_root` MUST 取平台应用数据目录，MUST NOT 依赖应用专属环境变量覆盖。

`solidway` 二进制的定位顺序 MUST 为：应用可执行文件同目录 → 应用资源目录 → 开发期工作区构建产物目录（仅调试构建）→ `PATH`。应用资源目录 MUST 取平台打包器给出的资源目录（Linux deb/rpm 为 `/usr/lib/<产品名>`、AppImage 为 `$APPDIR/usr/lib/<产品名>`、macOS 为 `Contents/Resources`、Windows 为安装目录），MUST NOT 由壳自行按字符串拼接推断。

发布构建的候选列表 MUST NOT 包含开发期工作区路径；定位失败时的错误信息 MUST 只列出真实尝试过的位置。

子进程异常退出后 MUST 按退避策略自动重启；MUST NOT 依赖环境变量开关。子进程的 stdout/stderr MUST 追加写入 `<data_root>/solidway.log`。

#### Scenario: 首次启动
- **WHEN** 应用启动且 `data_root` 尚无数据库
- **THEN** 应用创建目录并启动 `solidway`，由其对数据库执行迁移与 admin 引导

#### Scenario: 客户端包自带网关
- **WHEN** 用户只安装了桌面客户端（系统 PATH 中没有 `solidway`）
- **THEN** 应用从应用资源目录找到并启动随包分发的 `solidway`

#### Scenario: 可执行文件同目录优先
- **WHEN** 应用可执行文件同目录与资源目录同时存在 `solidway`
- **THEN** 使用同目录的那一份（兼容手工放置与服务包并存）

#### Scenario: 找不到二进制
- **WHEN** 上述定位顺序均未找到 `solidway`
- **THEN** 应用 MUST 展示可读错误（含已尝试的路径），SHALL NOT 静默失败，且错误中 MUST NOT 出现开发期工作区路径

#### Scenario: 控制台静态托管可用
- **WHEN** 子进程启动完成且 `web/dist` 位于仓库根或应用可执行文件附近
- **THEN** `GET /` 返回控制台页面，而非 404

#### Scenario: 子进程异常退出
- **WHEN** `solidway` 非正常退出且未超过重启上限
- **THEN** 应用按退避策略自动重启子进程
