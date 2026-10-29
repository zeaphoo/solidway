# Design

## Context

动机见 proposal.md - Why。约束现状：

- CI 发布的 dmg 不是 `package-mac.sh` 那份，而是 `notarize-mac.sh` 从已钉票 `.app` **重建**的（dmg 校验和独立、需二次公证 + 钉票，`notarize-mac.sh:212-216`）——安装盘布局必须作用于两处 dmg 组装，否则 CI 产物无布局。
- `notarize-mac.sh` 挂载断言（`Contents/MacOS/solidway` 存在 + codesign/stapler/spctl 验收）在新布局下均成立（Applications 链接是新增项），保持不动。
- `scripts/package-mac.sh:22` 现为 `IDENTITY="${MAC_SIGN_IDENTITY:-AKSL Software}"`；`release.yml` 已无 AKSL 回落（CI secrets 必填已生效）。
- AKSL 自签证书及 CA 已由 Charles 删除（2026-10-30）。
- grilling 裁决：Q1=A（共享组装脚本）、Q2=A（ds_store vendor）、Q3=A（版权独立需求）；Q4 的自定义背景方案经 macOS 26 实证不可用（见 D4），改为 Finder 默认白窗。范围追加①②（身份解析、文档重写）。

## Goals / Non-Goals

**Goals:**

- dmg 打开即见安装引导（窗口/图标布局 + Applications 链接），拖入即装。
- 本机与 CI 发布 dmg 布局构造上必然一致（同一组装实现）。
- 本机构建默认路径只产出可公证的 Developer ID 签名包，杜绝默认自签出口。

**Non-Goals:**

- 不改签名/公证/钉票/验收语义（`notarize-mac.sh` 公证链行为零变化）。
- 不动 `release.yml`（CI 依赖 Secrets 不变）。
- 不做自定义卷图标、多语言文案、`.DS_Store` 之外的 Finder 定制（工具栏/侧边栏精修等）。
- 不做自定义窗口背景/标签颜色（macOS 26 已忽略背景；`.DS_Store` 无受支持的标签颜色键）。

## Decisions

### D1 dmg 组装共享：新增 `scripts/dmg-layout.sh`（Q1=A）

单一实现承载「staging（`Solidway.app` + `/Applications` 符号链接 + 生成的 `.DS_Store`）→ `hdiutil` 建盘」。`package-mac.sh` 建盘段与 `notarize-mac.sh` 重建段均调用它；`notarize-mac.sh` 仅换组装来源，签名/公证/钉票/验收语义零变化（「不碰签名/公证链」按语义级理解）。`package-mac.sh` 的挂载断言追加布局断言（链接/`.DS_Store` 存在），`notarize-mac.sh` 断言保持不变。被否：B（staging 目录跨脚本耦合、布局逻辑两处）、C（双写必然分叉）。

### D2 `ds_store` vendor 入库（Q2=A）

MIT 许可小库 vendored 到 `scripts/python/ds_store/`（保留 LICENSE），**连同实际 Python 传递依赖 `mac_alias` 一并 vendor**（各自保留 LICENSE；`ds_store` 1.3.3 已不依赖 biplist），生成器 `scripts/python/gen_dmg_ds_store.py` 按 `sys.path` 就近 import；零安装、离线可用、CI/本机一致。被否：`pip3 install`（网络依赖、macOS 系统 python PEP 668 摩擦、版本漂移）；`dmgbuild` 整包（输入限定 ds_store 库）。

### D3 `.DS_Store` 布局常量集中在生成器

窗口 660×420、图标坐标（app 居左 / Applications 居右）、隐藏无关元素——作为常量写在 `gen_dmg_ds_store.py`；`.DS_Store` 只写窗口 `bwsp`、图标 `Iloc` 与 `icvp.arrangeBy`。生成后可用 ds_store 库读回断言（tasks 验证用）。无头生成，不依赖 Finder/AppleScript。

### D4 自定义背景放弃：macOS 26 只交付干净白窗（2026-10-30 实证）

**实证**（编排者在 macOS 26 挂载 preview dmg 截图/复验）：

- 自定义背景图（`.DS_Store` `icvp.backgroundImageAlias`）被 Finder 忽略，窗口回退系统默认白底；
- 纯色背景（`icvp.backgroundColor*` + `backgroundType=1`）同样被忽略；
- 图标下方的水印底由系统自动生成，不是可由背景图/`icvp` 覆盖的图层；
- `.DS_Store`/`icvp` 无受支持的标签颜色键，Finder 标签固定近黑色。

**决策**：放弃自定义 dmg 背景，交付干净白窗。`dmg-layout.sh` 不写任何背景相关键；
`.DS_Store` 只保留窗口 `bwsp`、图标 `Iloc` 与 `icvp.arrangeBy`；`packaging/dmg/` 素材移除。

**被否/留档**：自定义背景图 + 标签提亮带（曾尝试，macOS 26 不渲染背景，留档待系统恢复支持）；
整体浅色背景（同样被忽略，且与品牌深色底冲突）。构建期合成、含文字引导、自定义卷图标一并排除。

### D5 身份解析语义（范围追加①）

解析顺序：`MAC_SIGN_IDENTITY` 显式（校验存在于钥匙串，保留既有 `find-identity` 精确名检查）→ 未设置时探测 `security find-identity -v -p codesigning` 中名称以 `Developer ID Application` 开头的有效身份：恰一个即用其全名；多个/零个即失败并指引。无内置默认名。显式指定非 Developer ID 身份（如重建的自签身份）仍可用——「自签应急」收敛为显式路径，README 应急段给重建指引。被否：保留 `AKSL Software` 默认（证书已删、默认自签出口有害）、探测不到时回退任意 codesign 身份（静默降级）。

### D6 版权独立需求（Q3=A）

`copyright` 一行配置 +「应用版权信息」ADDED 需求（Get Info / Info.plist 可验收）；不并入 dmg 呈现需求（机制不同：bundle 元数据 vs dmg 布局）。

### D7 delta 落位 `release-pipeline`

MODIFIED「macOS 公证与钉票」+ ADDED×3，基线已在主 specs，无跨 change 归档依赖（对比 mac-notarized-packaging 的顺序约束，本 change 无）。

## Risks / Trade-offs

- [布局视觉效果无自动化断言] → 布局断言只查元素存在性（链接/`.DS_Store`/窗口与图标键）；视觉正确性靠目检清单（tasks 7.1/7.3，本机 + CI 产物各一次）。
- [系统默认白窗与品牌深色视觉差异] → 接受：macOS 26 不渲染自定义 dmg 背景；系统白窗 + 深色 app 图标仍清晰，避免不可维护的背景 hack。
- [`ds_store` 生成的 `.DS_Store` 在不同 macOS 版本 Finder 表现差异] → 布局常量固定、目检验收；`.DS_Store` 仅影响呈现，不影响安装功能（拖入安装始终可用）。
- [Applications 符号链接进入 dmg 影响公证/断言] → 公证提交物是 `.app` 的 zip（链接不进 zip）；挂载断言只查 `Solidway.app/Contents/MacOS/solidway`，新项不干扰。
- [vendored 库的维护成本] → ds_store 文件格式稳定、库小；固定版本并保留 LICENSE；升级属常规依赖变更。
- [共享脚本改动触碰 `notarize-mac.sh`] → 语义级零变化（仅组装来源），公证链行为经 mac-notarized-packaging 多轮评审固化；演练实证回归。
- [身份探测误匹配] → 只认 `Developer ID Application` 前缀的**有效**身份（`-v`）；多/无候选不猜、不降级。

## Migration Plan

1. 合并本 change（无数据迁移、无 Secrets 变更、`release.yml` 不动）。
2. 本机回归：`scripts/package-mac.sh`（自动探测身份）产出布局 dmg，目检 + 挂载断言。
3. `workflow_dispatch` 演练（`publish=false`）产出已公证布局 dmg；Charles 目检 CI 产物（拖入安装可用）。
4. 回滚：revert 合并即可；脚本为增量文件，无持久状态。
