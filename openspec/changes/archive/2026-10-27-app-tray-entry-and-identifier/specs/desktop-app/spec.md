# desktop-app Specification (delta: app-tray-entry-and-identifier)

## ADDED Requirements

### Requirement: 打开控制台始终可用

托盘「打开控制台」MUST 始终可用，MUST NOT 因控制台窗口是否存在而改变启用状态，MUST NOT 依赖平台托盘的动态启用能力。

点击「打开控制台」MUST 复用统一路径并按下述顺序分流：

- 控制台窗口存在 → MUST 显示并聚焦，MUST NOT 重新导航或刷新页面；
- 窗口不存在且已有可导航地址 → MUST 重建控制台窗口；
- 窗口不存在且尚无地址 → MUST 按「打开控制台不覆盖失败原因」分流（启动页 / 状态页 / 失败提示）。

#### Scenario: 窗口不存在时仍可点击
- **WHEN** 控制台窗口不存在（尚未创建或已关闭），用户点击托盘「打开控制台」
- **THEN** 应用重建控制台窗口（有地址时直接进入），菜单项全程保持可用

#### Scenario: 窗口存在时只前置
- **WHEN** 控制台窗口已存在，用户点击托盘「打开控制台」
- **THEN** 窗口显示并聚焦，页面不重新加载

#### Scenario: 未就绪时展示状态页
- **WHEN** 子进程尚未发出 `listening` 事件，用户点击「打开控制台」
- **THEN** 按既定分流展示启动页或已有失败原因（不标记为新的失败）

#### Scenario: 不依赖托盘动态启用
- **WHEN** 应用运行在任何桌面平台（含 Linux AppIndicator 后端）
- **THEN** 「打开控制台」菜单项始终可点，行为不依赖 `MenuItem::set_enabled` 的传播

### Requirement: 托盘图标的点击行为

在 Windows 上，**左键单击**托盘图标 MUST 打开控制台（复用「打开控制台」同一路径）；左键单击 MUST NOT 弹出菜单，右键 MUST 弹出菜单；双击 MAY 同样打开（该路径幂等：窗口已存在则只前置）。

macOS 与 Linux MUST NOT 因本需求改变左键行为：macOS 保持左键弹出菜单；Linux 托盘后端（AppIndicator / ksni）不派发点击事件，打开控制台 MUST 依赖菜单项。

#### Scenario: Windows 单击打开控制台
- **WHEN** 用户在 Windows 上左键单击托盘图标
- **THEN** 控制台窗口被打开（已存在则前置并聚焦），页面不重新加载，且不弹出菜单

#### Scenario: Windows 右键菜单
- **WHEN** 用户在 Windows 上右键单击托盘图标
- **THEN** 弹出托盘菜单

#### Scenario: Windows 双击不产生额外副作用
- **WHEN** 用户快速双击托盘图标
- **THEN** 控制台窗口仍只有一个，前台可见，不重复创建窗口

#### Scenario: macOS 保持菜单习惯
- **WHEN** 用户在 macOS 上左键单击托盘图标
- **THEN** 弹出菜单（与改动前一致）

#### Scenario: Linux 只走菜单
- **WHEN** 用户在 Linux 上点击托盘图标
- **THEN** 弹出菜单；打开控制台通过菜单项完成（后端不派发点击事件）

### Requirement: 托管子进程不显示控制台窗口

Windows 上应用派生子进程时 MUST 使用不创建控制台窗口的派生方式（如 `CREATE_NO_WINDOW`）；MUST NOT 通过把网关改成 GUI 子系统来隐藏窗口——网关独立运行（命令行 / 服务）时 MUST 保持控制台子系统，stdout 事件对调用方可见。

#### Scenario: 桌面启动无黑窗
- **WHEN** 用户从桌面应用启动 Solidway（含开机自启）
- **THEN** 不出现任何控制台窗口，只有托盘图标与（按需的）控制台界面

#### Scenario: 独立运行仍有输出
- **WHEN** 用户在命令行直接运行 `solidway.exe <data_root> --mode standalone`
- **THEN** `solidway: listening …`、`bootstrap …` 等事件照常打印在终端

## MODIFIED Requirements

### Requirement: Dock 图标存在性

Dock 图标 SHALL 跟随控制台窗口存在性显隐（macOS activation policy）：控制台窗口存在时以 `regular` 形态运行（Dock 图标可见）；不存在时（关进托盘、`--hidden` 启动未开窗）以 `accessory` 形态运行（Dock 图标隐藏），应用与托盘继续运行。

判据为控制台主窗口存在性：仅控制台主窗口计入，splash、notice 等瞬态窗口不计入；托盘「打开控制台」的可用性不参与该判据（该入口恒可用）。MUST NOT 采用恒隐藏形态（`LSUIElement`）：窗口存在时 Dock 图标 MUST 可见。

显隐切换 MUST 在窗口创建与销毁两个时机执行；隐藏 Dock 图标 MUST NOT 影响托盘图标与已展示窗口的可见性。其他平台 MUST NOT 因本需求产生任何行为变化。

#### Scenario: 关窗进托盘后 Dock 图标消失
- **WHEN** 偏好「关闭窗口时最小化到托盘」开启，用户关闭控制台窗口
- **THEN** Dock 图标消失，应用与 `solidway` 子进程继续运行，托盘图标仍在

#### Scenario: 窗口重建后 Dock 图标出现
- **WHEN** 通过托盘「以管理员自动登录」或「Dock 重开控制台」兜底路径重建控制台窗口
- **THEN** Dock 图标恢复可见

#### Scenario: 窗口存在时 Dock 图标可见
- **WHEN** 控制台窗口存在（含最小化）
- **THEN** Dock 图标可见，点击后窗口显示并聚焦

#### Scenario: --hidden 启动不显示 Dock 图标
- **WHEN** 以 `--hidden` 启动且尚未创建控制台窗口
- **THEN** Dock 图标不显示；首次创建控制台窗口后恢复可见

#### Scenario: 瞬态窗口不改变 Dock 图标
- **WHEN** 仅 splash、notice 等瞬态窗口可见（启动中、失败提示等）
- **THEN** Dock 图标保持隐藏

## REMOVED Requirements

### Requirement: 打开控制台菜单项可用性

**Reason**: 置灰在 Linux 托盘后端不可靠，且把主入口锁死；改为「始终可用 + 点击分流」（见「打开控制台始终可用」），Dock 图标判据保留在「Dock 图标存在性」。
