#!/usr/bin/env bash
# 构建 Linux 桌面客户端（Tauri）：deb + AppImage。
#
# 依赖：cargo、pnpm、node、cargo-tauri、Tauri 2 的 Linux 系统依赖
#       （webkit2gtk-4.1 / libayatana-appindicator3 / librsvg2 / patchelf 等）。
# 可用环境变量：
#   SKIP_WEB_BUILD=1  跳过 pnpm build（要求 web/dist 已存在）
#   DIST_DIR=...      输出目录，默认 <repo>/dist/linux-app
#
# 客户端**自带** solidway 二进制（bundle.resources，落点 /usr/lib/<产品名>/），
# 桌面壳按「可执行文件同目录 → 资源目录 → 开发期 target/<profile> → PATH」查找，
# 因此单装客户端即可运行；已装 service 包时仍会优先用自带的资源副本。
#
# Tauri 的 deb 配置没有「包名」字段（Package 由 productName 推导，会是 solidway，
# 与服务包同名），因此这里对产物做一次 control 重写，把包名改成 solidway-app。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT/dist/linux-app}"

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

# 版本一致性：tag/发布闸门之外的第二道保证（与 package-mac.sh 同口径）。
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
SOLIDWAY_BIN="$ROOT_TARGET/release/solidway"
if [[ ! -f "$SOLIDWAY_BIN" ]]; then
    echo "未找到 $SOLIDWAY_BIN" >&2
    exit 1
fi

# 资源注入：把网关二进制放到 app/ 供 bundle.resources 收集（构建后删除）。
cp -f "$SOLIDWAY_BIN" "$ROOT/app/solidway"
cleanup() {
    rm -f "$ROOT/app/solidway" "$ROOT/app/.bundle-resources.json"
}
trap cleanup EXIT

# 资源注入用 --config 临时文件（不落 app/tauri.*.conf.json）：普通 cargo build/test
# 不该要求 app/solidway 存在，只有打包时才声明这个 resource。
INJECT_CONFIG="$ROOT/app/.bundle-resources.json"
# resource 源路径必须是绝对路径：bundler 不按 tauri.conf.json 所在目录解析相对路径。
node -e 'const fs=require("fs");fs.writeFileSync(process.argv[2], JSON.stringify({bundle:{resources:{[process.argv[1]]:"solidway"}}}))' \
    "$ROOT/app/solidway" "$INJECT_CONFIG"

# linuxdeploy 自身是 AppImage：无 FUSE / 受限容器里挂载会失败（failed to run linuxdeploy），
# 强制自解压运行，避免本地与 CI 环境差异。
export APPIMAGE_EXTRACT_AND_RUN=1

BUNDLE_DIR="$(target_dir "$ROOT/app")/release/bundle"
# linuxdeploy 会复用 bundle/appimage 下的 AppDir；上次中断留下的坏 AppDir 会让
# 下一次构建报 `failed to run linuxdeploy`，因此每次构建前先清掉这两个目录。
rm -rf "$BUNDLE_DIR/appimage" "$BUNDLE_DIR/appimage_deb"
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

(cd "$ROOT/app" && cargo tauri build --config "$INJECT_CONFIG" --bundles deb,appimage)
rm -f "$INJECT_CONFIG"

shopt -s nullglob
debs=("$BUNDLE_DIR"/deb/*.deb)
appimages=("$BUNDLE_DIR"/appimage/*.AppImage)
shopt -u nullglob

if [[ ${#debs[@]} -eq 0 || ${#appimages[@]} -eq 0 ]]; then
    echo "Tauri bundler 未产出预期的 deb / AppImage（bundle 目录：${BUNDLE_DIR}）" >&2
    exit 1
fi

# AppImage：与 deb 统一成客户端命名前缀 solidway-app_（bundler 默认用 productName）。
# 架构名与 deb 对齐（dpkg 用 amd64/arm64，而不是 uname 的 x86_64/aarch64）。
case "$(uname -m)" in
    x86_64) LINUX_ARCH="amd64" ;;
    aarch64 | arm64) LINUX_ARCH="arm64" ;;
    *) LINUX_ARCH="$(uname -m)" ;;
esac
for file in "${appimages[@]}"; do
    cp -f "$file" "$DIST_DIR/solidway-app_${version}_${LINUX_ARCH}.AppImage"
done

# deb：重写包名（solidway → solidway-app），避免与服务包 `solidway` 同名互相顶掉。
if ! command -v dpkg-deb >/dev/null 2>&1; then
    echo "缺少 dpkg-deb（Debian 系打包工具），无法重写 deb 包名" >&2
    exit 1
fi
for file in "${debs[@]}"; do
    version="$(dpkg-deb -f "$file" Version)"
    arch="$(dpkg-deb -f "$file" Architecture)"
    work="$(mktemp -d)"
    dpkg-deb -R "$file" "$work"
    sed -i 's/^Package: .*/Package: solidway-app/' "$work/DEBIAN/control"
    out="$DIST_DIR/solidway-app_${version}_${arch}.deb"
    dpkg-deb --root-owner-group -b "$work" "$out" >/dev/null
    rm -rf "$work"

    # 包名必须与服务包 `solidway` 区分：同名会被 dpkg 当成同一个包升级，
    # 从而删掉 service 的 unit 与 /usr/bin/solidway。
    pkg="$(dpkg-deb -f "$out" Package)"
    if [[ "$pkg" != "solidway-app" ]]; then
        echo "deb 包名应为 solidway-app（与服务包 solidway 区分），实际：$pkg" >&2
        exit 1
    fi

    # bundler 的中间产物按 productName 命名（Solidway_<ver>_<arch>.deb，Package: solidway），
    # 已被上面的重打包取代；删除以免被误当作发布物分发。
    rm -f "$file"
done

echo
echo "生成完成："
for file in "$DIST_DIR"/*.deb; do
    echo "  $file  (Package: $(dpkg-deb -f "$file" Package))"
done
for file in "$DIST_DIR"/*.AppImage; do
    echo "  $file"
done
