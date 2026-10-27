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
# 与 Linux 不同：Windows 没有服务包形态，`solidway.exe` 必须与 `Solidway.exe`
# 同目录（app/src/config.rs 的「可执行文件同目录」查找）。安装包通过打包期
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

APP_EXE="$APP_TARGET/release/solidway-app.exe"
if [[ ! -f "$APP_EXE" ]]; then
    echo "未找到应用可执行文件 $APP_EXE" >&2
    exit 1
fi

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/portable"

# 便携包：Solidway.exe + solidway.exe 同目录（免安装、免管理员）。
cp -f "$APP_EXE" "$DIST_DIR/portable/Solidway.exe"
cp -f "$SOLIDWAY_EXE" "$DIST_DIR/portable/solidway.exe"
cat > "$DIST_DIR/portable/README.txt" <<'TXT'
Solidway 便携版
===============

直接运行 Solidway.exe 即可（会在 %APPDATA%\com.solidway.app 建数据目录）。

- solidway.exe 必须与 Solidway.exe 放在同一目录：桌面壳按「应用可执行文件同目录」
  查找网关二进制，找不到会在界面报错。
- 需要 WebView2 运行时（Windows 10 1803+ 通常已随 Edge 安装）。
- 本包未做代码签名，SmartScreen 可能提示「未知发布者」，选择「仍要运行」即可。
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
    echo "Tauri bundler 未产出安装包（bundle 目录：$BUNDLE_DIR，bundles=$BUNDLES）" >&2
    exit 1
fi
# 客户端产物统一前缀 solidway-app_（bundler 默认用 productName 命名）。
for file in "${installers[@]}"; do
    case "$file" in
        *.exe) out="solidway-app_${version}_x64-setup.exe" ;;
        *.msi) out="solidway-app_${version}_x64.msi" ;;
        *)     out="$(basename "$file")" ;;
    esac
    cp -f "$file" "$DIST_DIR/$out"
done

echo
echo "生成完成："
find "$DIST_DIR" -maxdepth 1 -type f -printf '  %p\n' | sort
echo "  $ZIP"
