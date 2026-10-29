# Design

## Context

动机见 proposal.md - Why。约束现状：

- `.github/workflows/release.yml` mac job（`macos-13` x86_64 / `macos-14` arm64）已有：secrets 缺失即失败 → 临时 keychain 导入（`MAC_CERT_P12_BASE64` / `MAC_CERT_PASSWORD` / `MAC_SIGN_IDENTITY`，默认 `AKSL Software`）→ `scripts/package-mac.sh` → `codesign --verify --deep --strict` 自检 → 只上传 dmg → `always()` 删 keychain。无公证环节。
- `scripts/package-mac.sh` 是本机五步链（workspace 构建 `solidway` → tauri 出 `.app` → post-copy 进 `Contents/MacOS/` → 内层 Mach-O 先签、`.app` 外层后签 → `hdiutil` 出 dmg + 挂载断言），无公证/钉票步骤。
- 桌面壳 `data_root` 由 Tauri `app_data_dir()`（按 `identifier` 派生）决定（`app/src/config.rs`）；`identifier` 迁移会改三平台数据目录名。
- `release-pipeline` 能力由 in-flight change `2026-10-26-release-pipeline-v-tags`（30/30 已完工、**尚未归档**）建立，其 delta 的「发布产物集」「macOS 构建的签名前提」即本 change 要改的两块。
- grilling 裁决（Charles，2026-10-29）：Q1=B、Q2=B、Q3=A、Q4=A、Q5=A。

## Goals / Non-Goals

**Goals:**

- release CI 的 mac 产物达到「下载后双击直接打开、无 Gatekeeper 拦截」（含离线）。
- 公证/钉票/验收链路本机与 CI 共用一套脚本，行为不分叉。
- 凭据面收敛为一份 Secrets 清单（名称定稿，供 Charles 配置）。

**Non-Goals:**

- 不改 Linux/Windows 构建任务结构、产物命名与签名策略（Windows 仍未签名）。
- 不改 `scripts/package-mac.sh` 本机链（自签应急路径保留原样）。
- 不做公证缓存/票据复用、不做 universal2、不做自动更新。
- 不做 identifier 数据目录迁移工具（v0.1.0 未发布，无历史用户）。

## Decisions

### D1 公证凭据：App Store Connect API key（Q1=B）

Secrets 定稿（连同签名共 6 项，供 Charles 在 GitHub 仓 Settings 配置）：

| Secret | 用途 | 状态 |
|---|---|---|
| `MAC_CERT_P12_BASE64` | Developer ID Application 证书 p12（base64，含完整链） | 沿用名，内容换新证书 |
| `MAC_CERT_PASSWORD` | p12 口令 | 沿用名，内容换新证书 |
| `MAC_SIGN_IDENTITY` | `Developer ID Application: <姓名> (<TeamID>)` 全名 | **改必填**，去掉 `AKSL Software` 默认 |
| `MAC_NOTARY_API_KEY_ID` | App Store Connect API key id | 新增 |
| `MAC_NOTARY_API_ISSUER_ID` | issuer id | 新增 |
| `MAC_NOTARY_API_KEY_P8_BASE64` | `.p8` 私钥（base64） | 新增 |

`notarytool` 以 `--key <临时文件> --key-id … --issuer-id …` 认证；`.p8` 解码到 `$RUNNER_TEMP` 临时文件，`always()` 清理。被否：Apple ID + App 专用密码（绑定个人账号、与 2FA 环境耦合；API key 可独立轮换、与人解耦）。

### D2 公证提交物与钉票口径：`.app` 提交、`.app`/dmg 双钉票（Q2=B）

`scripts/notarize-mac.sh` 流程（输入为 `package-mac.sh` 产出的 `dist/mac/`）：

1. `ditto -c -k --keepParent` 打包 `Solidway.app` 为 zip；
2. 提交 zip 并等待 Accepted；轮询预算实现形态（R1 实现提示）：`notarytool submit` 拿 submission id → `notarytool wait <id> --timeout 30m`（或外层 `timeout` 命令包裹），超时即失败——`--timeout` 属 `wait` 子命令，`submit --wait` 本身无该选项；
3. `xcrun stapler staple` 钉票 `.app`，`stapler validate` 断言；
4. 由已钉票 `.app` `hdiutil` 重建 `solidway-app_<version>_<arch>.dmg`（命名沿用约定），对 dmg 钉票；
5. 挂载 dmg 断言内含 `solidway`、`stapler validate` 通过；
6. `spctl -a -vv` 验收（对 dmg，必要时 `--type install`；挂载内 `.app` 一并验证），输出 MUST 含 `accepted source=Notarized Developer ID`。

entitlements 口径（R1 实现提示）：保持现状手工 `codesign --options runtime`、**不引入 entitlements 文件**——公证只强制 hardened runtime（已满足），不改变运行时约束；潜在运行时问题由 tasks 4.3 手工验收（双击直开）兜底。

被否：只提交/钉票 dmg（`.app` 内无票，首开依赖 Apple 在线校验，离线被拦）。

### D3 公证脚本化：新增独立 `scripts/notarize-mac.sh`，`package-mac.sh` 零改动

CI 与本机共用；本机 `package-mac.sh` 自签路径保持可用作应急。被否：CI 步骤内联（本机无法复用，容易与 CI 分叉）。

### D4 规格增量落 `release-pipeline`：MODIFIED + ADDED（Q3=A）

对「发布产物集」「macOS 构建的签名前提」做 `MODIFIED`（完整块改写），公证四要求 `ADDED`。实测 `openspec validate --strict` 通过（0 failed），但报 INFO：因 `release-pipeline` 主规格尚未归档存在，**归档时**会拒绝 `MODIFIED`。处置：**归档顺序依赖**——先归档 `2026-10-26-release-pipeline-v-tags`（其 delta 并入 main specs）再归档本 change；该依赖写入 tasks 收尾任务与 ready 标记，评审知悉。被否：全 `ADDED` 自包含（与基线 delta 的同名要求在归档合并时冲突，语义更差）；落 `platform-support`（发布行为错域）。

### D5 `MAC_SIGN_IDENTITY` 必填化

个人账号身份名含个人姓名，不可硬编码默认值；`release.yml` 的 env 默认值 `AKSL Software` 移除，缺失与其他 secrets 同一失败路径（配置指引文案列出全部 6 个 secrets）。`scripts/package-mac.sh` 的 `MAC_SIGN_IDENTITY` 环境变量机制不变（默认仍 `AKSL Software`，本机应急路径），CI 经 env 显式传入。

### D6 identifier 迁移无规格增量

`xyz.solidway` → `xyz.solidway.desktop` 是构建配置（`app/tauri.conf.json` 一行）+ 文档/文案同步（`docs/getting-started.md` / `docs/getting-started.zh.md` / `packaging/README.md` 数据目录表、`scripts/package-windows.sh` 卸载说明）。桌面壳行为（`data_root` 取平台应用数据目录）不变，`desktop-app` 现有规格无 identifier 命名约束，故不产生 spec delta。连带：三平台数据目录名变为 `xyz.solidway.desktop`；v0.1.0 未发布、开发期目录改名可接受，`packaging/README.md` 留一行手工迁移说明。

### D7 自签回落取消（输入需求 3 固化）

CI 侧 secrets 缺失 = 明确失败 + 配置指引，不降级、不跳过；公证失败 = 发布失败。应急构建走本机 `package-mac.sh`（不在 CI 打通）。

## Risks / Trade-offs

- [归档顺序依赖（D4）] → tasks 收尾任务显式核对 `2026-10-26-release-pipeline-v-tags` 已归档；ready 标记向评审声明；若归档期顺序反转，先归档方需按 OpenSpec 合并规则调整（已在 design 记录退路）。
- [Apple 公证服务不可用 / 排队超时] → `notarytool --wait` 加轮询预算（建议 30 分钟），超时即失败；失败不降级（D7），可重跑 job。
- [`.p8` / p12 泄露] → 只以 base64 进 Secrets，workflow 只读、临时文件用后即删（`always()`）；API key 可在 App Store Connect 即时吊销，泄露半径小于个人口令。
- [`macos-13` 被 GitHub 下线] → 已知风险（ADR 0018 已记），届时换 Intel 镜像。
- [identifier 改名使开发机旧数据目录"丢失"] → 未发布无用户；README 留手工迁移一行（改名目录即可）。
- [`spctl` 对 dmg 的评估形态差异] → 验收以输出 `accepted source=Notarized Developer ID` 为准，dmg 与挂载内 `.app` 双验证（必要时 `--type install`），演练先行实证。

## Migration Plan

1. Charles 在 GitHub 仓 Settings 配置 6 个 Secrets（清单见 D1）。
2. 合并本 change 后 `workflow_dispatch` 演练（`publish=false`，`with_mac=true`）实跑：产出已公证 dmg，`spctl` 验收通过、`stapler validate` 票据存在。
3. Charles 下载演练产物手工验「双击直接打开无 Gatekeeper 拦截」。
4. 通过后按既有流程发 `v*` 标签正式发布。
5. 回滚：revert 合并即可（无数据迁移；identifier 未发布前可反向改回）。

## 评审留痕（R1，2026-10-29，通过）

评审报告 `tmp/review-spec-mac-notarized-packaging-r1.md` 的 3 条非阻塞提示**全部接受**，落痕位置如下（spec 正文无修改）：

1. notarytool 轮询预算实现形态（`wait <id> --timeout`，`submit --wait` 无 `--timeout`）→ 落 D2 步骤 2 与 tasks 2.1。
2. entitlements 口径（不引入 entitlements 文件，公证只强制 hardened runtime）→ 落 D2「entitlements 口径」段。
3. tasks 1.1 grep 验证会命中归档 change 与 ADR 0011 修订记录的历史字样（有意保留）→ 落 tasks 1.1 验证措辞。
