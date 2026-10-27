# ADR 0011 — macOS 桌面交付口径：纯桌面进程模型 + 自签名不公证

## 状态

已接受（2026-10-22，Charles 与负责人拍板 D1/D2/D3，本 ADR 留痕动机与取舍）

## 背景

mac 打包闭环（`mac-desktop-packaging`）需要先回答两个未来读者必会疑惑的问题：

1. **进程模型**：Linux 有 systemd 服务包（deb/rpm，常驻服务），mac 要不要对等地提供 launchd 服务包？要不要支持「app 检测已有 `solidway` 服务则连接」？
2. **签名与公证**：Apple 生态的常规发布是 Developer ID 签名 + 公证（notarization）。为什么本项目自签「AKSL Software」且不做公证？ad-hoc 签名为什么被禁止？

约束事实：Keychain 已有「AKSL Software」身份（含其 CA）；AKSL 系 TCC（BarRunner / Stoker / MarmotHole 同款）共识身份；mac 分发对象以内部与受控渠道为主。

## 决策

1. **mac 侧纯桌面 app**：随开随用、退出即停（`solidway` 子进程由 `app/` 托管，退出即停）。**不做 launchd 服务包**，**不补**「app 检测已有服务则连接」模式；`app/` 的 supervisor / 托盘 / 健康探测行为零变更。
2. **发布物自签「AKSL Software」+ hardened runtime，禁 ad-hoc**：`.app` 内全部 Mach-O 与 `.app` 本体均以 AKSL 身份签名（`codesign --options runtime`），签名不可用即失败，不提供跳过开关、不回退 ad-hoc。
3. **不做 Apple 公证**：接受的 Gatekeeper 体验下限——经 quarantine（浏览器下载 / AirDrop）的分发对象会看到「无法验证开发者」，可放行使用；`spctl -a -vv` 预期不通过属正常，留痕说明。对外分发真发生时**另立事项**做 Developer ID + 公证。
4. **CI artifact 同为分发物**：CI（`mac-packages` job）经 secrets 注入 p12 凭据、临时钥匙串导入 AKSL 身份后打包上传；secrets 缺失时 fail fast，MUST NOT 产出未签名 / ad-hoc artifact。

## 被否方案

- **launchd 服务包（对齐 Linux systemd）**：mac 使用形态是「随开随用」的桌面工具，服务化引入开机自启、权限与卸载语义，超出当前需求；且已有 Linux 服务形态覆盖常驻场景。
- **「连接已有服务」模式**：桌面壳职责是「定位并启动 `solidway`」，连接外部实例会引入双端口 / 双数据目录的归属问题，YAGNI。
- **ad-hoc 签名（`codesign -s -`）**：quarantine 下呈「已损坏」级体验（比自签更差），且违反 AKSL 系 TCC 共识的身份口径。
- **Apple 公证（Developer ID + notarization）**：需要付费开发者账号、凭据轮换与公证流水线，当前分发对象为内部 / 受控渠道，成本收益不成立；对外分发出现时另立。
- **CI 不签名（产物仅供冒烟）**：CI artifact 属本轮分发物（D3），未签名产物在 quarantine 下体验劣于自签口径，与「禁 ad-hoc」精神冲突；故 CI 必须签名（secrets 前置）。

## 后果

### 正面

- mac 用户获得可分发的 `.app` / `.dmg`（本机一键 + CI artifact），quarantine 下为「可放行」级体验；
- 进程模型简单（随开随用、退出即停），与 `app/` 既有职责边界一致，零运行时变更；
- 签名口径与 TCC 其他产品（AKSL 系）一致，身份管理集中。

### 负面 / 风险

- 未公证：quarantine 对象需手动放行（README 说明 + `xattr` 兜底），`spctl` 不通过会出现在安全审计视野；
- 自签身份依赖 Keychain 中 AKSL 证书有效期，过期需轮换（CI secrets 亦随轮换更新）；
- mac 无常驻服务形态：需要常驻的部署场景只能用 Linux 服务包（或用户自行包装 launchd）；
- CI job 依赖 secrets 配置，未配置即红（fail fast 是有意为之，防未签名产物流出）。

## 修订记录

### 2026-10-22 — 方案 B：CI 不构建 mac 分发物

**裁定**（Charles gate，2026-10-22）：**撤销决策 4**（「CI artifact 同为分发物 / `mac-packages` job」）。GitHub CI **不构建、不上传** mac 分发物；本机 `scripts/package-mac.sh` 成为**唯一构建路径**。决策 1~3 不变（纯桌面进程模型、AKSL 自签 + hardened runtime、禁 ad-hoc、不做公证）。

**理由**：

1. **私钥不上云**：CI 构建需要把 AKSL 签名私钥（p12 + 口令）放进 GitHub Secrets 并导入 runner 临时钥匙串；签名私钥不应离开本机 Keychain。
2. **成本**：私有仓库的 macOS runner 计费倍率约为 Linux 的 10 倍；mac 分发物产出频率低，常驻 CI 构建不划算。

**落地改动**：删除 `.github/workflows/ci.yml` 的 `mac-packages` job（含 secrets 检查 / 临时 keychain / 签名 / 上传步骤）；`packaging/README.md` mac 节删除 CI secrets 清单，改为「CI 不产出 mac 包，构建仅本机」并保留本机构建/签名可勾对清单；`docs/tech-context.md` 同步为「本机打包，CI 不构建」。

### 2026-10-26 — 按预留路径恢复 CI 打包（tag 触发 + secrets）

**裁定**：接受 ADR 0018，恢复决策 4 的 CI 构建，但改为**只由 `v*` 标签触发**（不再随 push 常驻构建），并把签名凭据放进 GitHub Secrets（`MAC_CERT_P12_BASE64` / `MAC_CERT_PASSWORD`，可选 `MAC_SIGN_IDENTITY`）。签名口径、post-copy 顺序、禁 ad-hoc、不公证全部不变；`scripts/package-mac.sh` 未做任何修改。

**落地**：`.github/workflows/release.yml` 的 `mac` job（matrix `macos-13` x86_64 / `macos-14` arm64）：secrets 缺失即失败 → 临时 keychain 导入 → 本机同一脚本 → `codesign --verify --deep --strict` 自检 → 只上传 `.dmg` → `always()` 删除 keychain。详见 `openspec/changes/2026-10-26-release-pipeline-v-tags/`。

**仍未改变**：不做 Apple 公证，`spctl` 预期不通过；quarantine 放行说明仍在 `packaging/README.md`。

**恢复路径**（负责人决定后）：CI 改为 **tag 触发**（避免每次 push 构建），配置 `MAC_CERT_P12_BASE64` / `MAC_CERT_PASSWORD`（p12 含 `AKSL Software` 及其 CA 完整链），恢复「临时 keychain 导入 → `scripts/package-mac.sh` → `codesign --verify --deep --strict` 自检 → 只上传 `.dmg`」；本机脚本无需改动。
