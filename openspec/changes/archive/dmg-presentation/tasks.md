# Tasks

## 1. 自定义背景移除（2026-10-30 方向变更，design D4）

- [x] 1.1 放弃自定义 dmg 背景：删除 `packaging/dmg/background.*`（macOS 26 Finder 忽略 `.DS_Store` 背景图/纯色，交付 Finder 默认白窗）。验证：`packaging/dmg/` 不存在；dmg 不含 `.background` 目录

## 2. `.DS_Store` 无头生成（design D2/D3）

- [x] 2.1 vendor `ds_store` 库入 `scripts/python/ds_store/`（保留 LICENSE/NOTICE 与出处版本说明）；**连同实际 Python 传递依赖 `mac_alias` 一并 vendor**（纯 Python、MIT，各自保留 LICENSE），保证离线零安装可 import。验证：LICENSE 全部在位；`python3 -c "import sys; sys.path.insert(0,'scripts/python'); import ds_store"` 成功（缺传递依赖会在此失败）
- [x] 2.2 新增 `scripts/python/gen_dmg_ds_store.py`：对 staging 目录生成 `.DS_Store`（窗口 660×420、Solidway.app 居左 / Applications 居右、隐藏无关元素；`icvp` 只写 `arrangeBy`，不写背景键）。验证：python3 生成成功，且用 ds_store 读回断言窗口/两个图标位置条目、无背景键

## 3. 共享 dmg 组装（design D1）

- [x] 3.1 新增 `scripts/dmg-layout.sh`：staging（`Solidway.app` + `/Applications` 符号链接 + 生成的 `.DS_Store`）→ `hdiutil` 建盘（命名沿用 `solidway-app_<version>_<arch>.dmg`）。验证：`bash -n` 通过；产出 dmg 挂载可见 app/链接/`.DS_Store`
- [x] 3.2 `scripts/package-mac.sh` 接入：建盘段改调 `dmg-layout.sh`；挂载断言追加布局断言（`/Applications` 链接、`.DS_Store` 存在）。验证：本机全链跑通，新旧挂载断言全过
- [x] 3.3 `scripts/notarize-mac.sh` 接入：dmg 重建行改调 `dmg-layout.sh`，其余零改动。验证：`bash -n` 通过；公证/钉票/验收/挂载断言语句与改动前 diff 为零（仅组装来源变化）

## 4. 本机构建身份解析（范围追加①，design D5）

- [x] 4.1 `scripts/package-mac.sh` 身份解析：移除 `AKSL Software` 默认值；`MAC_SIGN_IDENTITY` 显式优先（校验存在于钥匙串）→ 未设置时自动探测唯一 `Developer ID Application` 有效身份 → 多/无候选失败并指引显式设置 `MAC_SIGN_IDENTITY`；同步更新脚本头注释。验证：三条路径本机实测（唯一自动探测 / 显式覆盖 / 失败信息含指引）；`grep -n AKSL scripts/package-mac.sh` 无命中

## 5. 应用版权信息（design D6）

- [x] 5.1 `app/tauri.conf.json` 的 `copyright` 改为 `Copyright © 2026 solidway.xyz`。验证：构建后 `Info.plist` 与 Finder「显示简介」显示该字符串

## 6. 文档重写（范围追加②）

- [x] 6.1 `packaging/README.md` 本机构建节重写：Developer ID 本机构建姿势（自动探测 + 显式覆盖）、自签应急路径随 AKSL 证书退役的说明（如需自签按应急段重建指引）、CI 依赖 Secrets 不变、不再维护背景素材。验证：README 无「默认自签可用」口径残留（`grep -n 自签` 仅出现在退役说明/应急重建语境）

## 7. 回归与演练

- [x] 7.1 本机 `scripts/package-mac.sh` 全链回归（自动探测身份路径）+ 挂载断言 + dmg 目检（白窗布局、Applications 链接、拖入安装可用）。验证：目检清单全过（2026-10-30 编排者 macOS 26 实机截图复验：白窗 + 布局 + 标签可读；真实 Developer ID 签名链由 run 40939077699 双臂闭环）
- [x] 7.2 `openspec validate --strict` 通过；`actionlint` 0 告警（若 workflow 有动；预期 `release.yml` 零改动，确认无动即可）。验证：命令输出符合预期
- [x] 7.3 `workflow_dispatch` 演练（`publish=false`）产出已公证布局 dmg。验证：`stapler validate` / `spctl` 验收过；Charles 目检 CI 产物布局与拖入安装可用（run 40939077699：x86_64 / arm64 双臂全绿，公证/钉票/挂载验收通过）

## 8. 决策留痕（随提案已落地）

- [x] 8.1 `docs/adr/0011-mac-desktop-packaging-and-signing.md` 追加「2026-10-30 — 本机默认自签身份退役」修订记录——提案阶段已落地（范围追加①的决策）。验证：修订记录与 design D5 一致

## Workflow follow-up

- 归档本 change（本 change 无跨 change 归档顺序依赖，`release-pipeline` 已在主 specs）；归档后 `openspec validate --all --strict` 确认合并结果。
