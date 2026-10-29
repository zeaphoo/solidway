#!/usr/bin/env bash
# 构建 Windows 桌面客户端：NSIS / MSI 安装包 + 便携 zip（两个 exe 同目录）。
#
# 依赖：cargo、pnpm、node、cargo-tauri；Windows 上可用 Git Bash 运行。
# 可用环境变量：
#   SKIP_WEB_BUILD=1     跳过 pnpm build（要求 web/dist 已存在）
#   DIST_DIR=...         输出目录，默认 <repo>/dist/windows
#   TAURI_BUNDLES=...    传给 tauri 的 bundle 列表，默认 nsis,msi
#                        （没有 WiX 下载权限的环境可设 TAURI_BUNDLES=nsis）
#
# 与 Linux 不同：Windows 没有服务包形态，`solidway.exe` 必须与应用主程序同目录
# （app/src/config.rs 的「可执行文件同目录」查找）。应用主程序名为 `SolidwayApp.exe`
# （tauri.conf.json 的 mainBinaryName）——**不能**叫 `Solidway.exe`：Windows 文件名
# 大小写不敏感，会与 `solidway.exe` 撞成同一个文件。安装包通过打包期
# `--config` 注入 bundle.resources（不落 app/tauri.*.conf.json）；便携 zip 直接放两个 exe。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT/dist/windows}"
BUNDLES="${TAURI_BUNDLES:-nsis,msi}"

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) ;;
    *)
        echo "package-windows.sh 仅在 Windows（Git Bash / MSYS）上运行" >&2
        exit 1
        ;;
esac

for command in cargo pnpm node; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "缺少命令: $command" >&2
        exit 1
    fi
done

if ! cargo tauri --version >/dev/null 2>&1; then
    echo "缺少 tauri-cli；请执行: cargo install tauri-cli --locked" >&2
    exit 1
fi

# 版本一致性：发布闸门之外的第二道保证。
version="$("$ROOT/scripts/check-release-version.sh")"

if [[ "${SKIP_WEB_BUILD:-0}" != "1" ]]; then
    if [[ ! -d "$ROOT/web/node_modules" ]]; then
        pnpm --dir "$ROOT/web" install --frozen-lockfile
    fi
    pnpm --dir "$ROOT/web" build
fi
if [[ ! -f "$ROOT/web/dist/index.html" ]]; then
    echo "缺少 web/dist/index.html；先执行 pnpm --dir web build" >&2
    exit 1
fi

target_dir() {
    # 兼容调用方自定义 CARGO_TARGET_DIR（与 package-mac.sh 同做法）。
    (cd "$1" && cargo metadata --no-deps --format-version 1 |
        node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')
}

cd "$ROOT"
cargo build --release -p solidway
ROOT_TARGET="$(target_dir "$ROOT")"
SOLIDWAY_EXE="$ROOT_TARGET/release/solidway.exe"
if [[ ! -f "$SOLIDWAY_EXE" ]]; then
    echo "未找到 $SOLIDWAY_EXE" >&2
    exit 1
fi

# 安装包注入：把 solidway.exe 放到 app/ 供 bundle.resources 收集（构建后删除）。
cp -f "$SOLIDWAY_EXE" "$ROOT/app/solidway.exe"
cleanup() {
    rm -f "$ROOT/app/solidway.exe" "$ROOT/app/.bundle-resources.json"
}
trap cleanup EXIT

# 资源注入用 --config 临时文件（与 Linux 脚本一致）：把 solidway.exe 打进安装目录。
INJECT_CONFIG="$ROOT/app/.bundle-resources.json"
# 绝对路径 + Windows 形式（bundler 不解析相对路径，也不认 MSYS 的 /d/... 路径）。
node -e 'const fs=require("fs");fs.writeFileSync(process.argv[2], JSON.stringify({bundle:{resources:{[process.argv[1]]:"solidway.exe"}}}))' \
    "$(cygpath -w "$ROOT/app/solidway.exe")" "$(cygpath -w "$INJECT_CONFIG")"
(cd "$ROOT/app" && cargo tauri build --config "$(cygpath -w "$INJECT_CONFIG")" --bundles "$BUNDLES")
rm -f "$INJECT_CONFIG"
APP_TARGET="$(target_dir "$ROOT/app")"
BUNDLE_DIR="$APP_TARGET/release/bundle"

APP_EXE="$APP_TARGET/release/SolidwayApp.exe"
if [[ ! -f "$APP_EXE" ]]; then
    echo "未找到应用可执行文件 $APP_EXE" >&2
    exit 1
fi

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/portable"

# 便携包：SolidwayApp.exe + solidway.exe 同目录（免安装、免管理员）。
# 两个名字必须在**大小写不敏感**的文件系统上也不同，否则便携包会少一个 exe。
cp -f "$APP_EXE" "$DIST_DIR/portable/SolidwayApp.exe"
cp -f "$SOLIDWAY_EXE" "$DIST_DIR/portable/solidway.exe"
cat > "$DIST_DIR/portable/README.txt" <<TXT
Solidway 便携版 ${version} (x64)          Solidway portable ${version} (x64)
================================================================================

启动 / Run
  Double-click SolidwayApp.exe —— 免安装、免管理员权限，不写系统目录。
  应用会在本机启动网关并打开控制台，托盘图标常驻。
  Double-click SolidwayApp.exe. No installer or admin rights needed; the app
  starts the gateway locally and opens the console. It stays in the tray.

目录结构 / Layout
  SolidwayApp.exe   桌面应用 / desktop app
  solidway.exe      网关 / gateway —— 必须与 SolidwayApp.exe 同目录
  README.txt        本说明 / this file

首次启动 / First run
  桌面模式免登录，自动以管理员进入控制台。
  管理员口令是可选项，需要时在「个人」页设置（用于浏览器登录或共享给团队）。
  Desktop mode signs in automatically as admin. A password is optional and can
  be set under Profile — it is only needed for browser or shared access.

数据目录 / Data directory
  %APPDATA%\xyz.solidway.desktop
  solidway.db        用户、路由、Provider 凭据（加密）、设置
  usage_detail.db    逐条用量明细
  master.key         加密主密钥 —— 必须与 solidway.db 一起备份
  solidway.log       日志
  没有 master.key，数据库里的 Provider 凭据无法解密。
  Back up solidway.db and master.key together; without the key, stored provider
  credentials cannot be decrypted.

局域网访问 / LAN access
  默认只监听 127.0.0.1:6565。要让其他机器使用，在控制台「系统 → 网络」修改绑定地址，
  并在对外暴露前开启 HTTPS（「系统 → HTTPS」）。
  Listens on 127.0.0.1:6565 by default. For other machines, change the bind
  address under System → Network and enable HTTPS before exposing it.

退出 / Quit
  托盘菜单「退出」会同时停止网关；只关闭窗口则网关继续在托盘运行。
  Tray → Quit stops the gateway. Closing the window keeps it running.

WebView2
  Windows 10 1803+ 通常已随 Edge 安装。若提示缺少运行时，安装 Evergreen
  Bootstrapper 后重试：https://developer.microsoft.com/microsoft-edge/webview2/
  Usually present on Windows 10 1803+. If missing, install the Evergreen
  Bootstrapper from the URL above and try again.

未签名 / Unsigned build
  本包未做代码签名，SmartScreen 可能提示「未知发布者」：选择「更多信息 → 仍要运行」。
  Not code-signed: SmartScreen may warn about an unknown publisher. Choose
  "More info" → "Run anyway".

卸载 / Uninstall
  删除本目录即可；如需清除数据，再删除 %APPDATA%\xyz.solidway.desktop。
  Delete this folder. To remove data as well, delete %APPDATA%\xyz.solidway.desktop.

许可证 / License
  AGPL-3.0-only — https://github.com/zeaphoo/solidway/blob/main/LICENSE
TXT

ZIP="$DIST_DIR/solidway-app_${version}_x64_portable.zip"
PORTABLE_WIN="$(cygpath -w "$DIST_DIR/portable")"
ZIP_WIN="$(cygpath -w "$ZIP")"
powershell.exe -NoProfile -NonInteractive -Command \
    "Compress-Archive -Path '$PORTABLE_WIN\\*' -DestinationPath '$ZIP_WIN' -Force" >/dev/null

# 安装包：Tauri bundler 产出 nsis/*.exe 与 msi/*.msi。
shopt -s nullglob
installers=("$BUNDLE_DIR"/nsis/*.exe "$BUNDLE_DIR"/msi/*.msi)
shopt -u nullglob
if [[ ${#installers[@]} -eq 0 ]]; then
    echo "Tauri bundler 未产出安装包（bundle 目录：${BUNDLE_DIR}，bundles=${BUNDLES}）" >&2
    exit 1
fi
# 客户端产物统一前缀 solidway-app_（bundler 默认用 productName 命名）。
for file in "${installers[@]}"; do
    case "$file" in
        *.exe) out="solidway-app_${version}_x64-setup.exe" ;;
        *.msi)
            # WiX 按 wix.language 每个语言出一个 MSI（Solidway_<ver>_x64_en-US.msi），
            # 保留语言后缀，避免多语言产物互相覆盖。
            locale="${file##*_}"
            out="solidway-app_${version}_x64_${locale}"
            ;;
        *)     out="$(basename "$file")" ;;
    esac
    cp -f "$file" "$DIST_DIR/$out"
done

# 便携包断言：两个 exe 都必须真的存在（大小写不敏感的文件系统上名字撞车会丢一个）。
for name in SolidwayApp.exe solidway.exe; do
    if [[ ! -f "$DIST_DIR/portable/$name" ]]; then
        echo "便携包缺少 ${name}（检查两个 exe 的文件名是否只差大小写）" >&2
        exit 1
    fi
done

# 安装包断言：NSIS 产物里必须同时含应用主程序与网关。
# NSIS 是 7-Zip 能解析的格式；找不到 7z 时只告警（CI 的 windows runner 自带）。
SEVEN_ZIP=""
if command -v 7z >/dev/null 2>&1; then
    SEVEN_ZIP="7z"
elif [[ -x "/c/Program Files/7-Zip/7z.exe" ]]; then
    SEVEN_ZIP="/c/Program Files/7-Zip/7z.exe"
fi

for file in "$DIST_DIR"/*-setup.exe; do
    [[ -f "$file" ]] || continue
    if [[ -z "$SEVEN_ZIP" ]]; then
        echo "WARN: 未找到 7-Zip，跳过 $file 的内容断言" >&2
        continue
    fi
    listing="$("$SEVEN_ZIP" l "$file" 2>/dev/null || true)"
    for name in SolidwayApp.exe solidway.exe; do
        if ! grep -qi "$name" <<<"$listing"; then
            echo "安装包 ${file##*/} 内缺少 $name" >&2
            exit 1
        fi
    done
    echo "安装包内容断言通过：${file##*/} 含 SolidwayApp.exe + solidway.exe"
done

echo
echo "生成完成："
find "$DIST_DIR" -maxdepth 1 -type f -printf '  %p\n' | sort
echo "  $ZIP"
