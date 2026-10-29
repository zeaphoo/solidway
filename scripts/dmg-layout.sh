#!/usr/bin/env bash
# 组装 macOS dmg：Solidway.app + /Applications 链接 + Finder 窗口/图标布局。
#
# 用法：dmg-layout.sh <app-path> <output-dmg> <volume-name>
#
# 由 scripts/package-mac.sh 与 scripts/notarize-mac.sh 共用，保证本机与 CI
# 重建出的 dmg 布局一致。签名/公证/钉票不在本脚本职责内。
#
# 流程：
#   1. staging：复制 .app、创建 /Applications 链接、在 staging 内生成 .DS_Store
#   2. hdiutil 建压缩 dmg（-fs HFS+）
#   3. 只读挂载复核 app/链接/.DS_Store
#
# macOS 26 起 Finder 忽略 dmg 自定义背景（图片与纯色均被忽略），因此这里不写
# backgroundImageAlias/backgroundColor；窗口使用 Finder 默认呈现。
#
# 依赖：hdiutil、ditto、python3、ln、mktemp（macOS 自带）。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="Solidway"
APP_BUNDLE="$APP_NAME.app"
GENERATOR="$ROOT/scripts/python/gen_dmg_ds_store.py"

fail() {
    echo "$*" >&2
    exit 1
}

if [[ "$(uname -s)" != "Darwin" ]]; then
    fail "dmg-layout.sh 仅支持 macOS"
fi

if [[ "$#" -ne 3 ]]; then
    fail "用法: dmg-layout.sh <app-path> <output-dmg> <volume-name>"
fi

APP_PATH="$1"
DMG_PATH="$2"
VOL_NAME="$3"

if [[ ! -d "$APP_PATH" ]]; then
    fail "缺少 app：${APP_PATH}"
fi
if [[ "$(basename "$APP_PATH")" != "$APP_BUNDLE" ]]; then
    fail "app 名称必须为 ${APP_BUNDLE}：${APP_PATH}"
fi
if [[ ! -f "$GENERATOR" ]]; then
    fail "缺少 .DS_Store 生成器：${GENERATOR}"
fi

for command in hdiutil ditto python3 ln mktemp; do
    if ! command -v "$command" >/dev/null 2>&1; then
        fail "缺少命令: ${command}"
    fi
done

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/solidway-dmg-layout.XXXXXX")"
STAGING="$WORK_DIR/staging"
MOUNT_DIR="$WORK_DIR/mnt"
MOUNTED=""

cleanup() {
    if [[ -n "${MOUNTED}" ]]; then
        hdiutil detach "$MOUNTED" >/dev/null 2>&1 || true
        MOUNTED=""
    fi
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

mkdir -p "$STAGING" "$MOUNT_DIR" "$(dirname "$DMG_PATH")"

ditto "$APP_PATH" "$STAGING/$APP_BUNDLE"
ln -s /Applications "$STAGING/Applications"

echo "生成 Window/Icon 布局…"
python3 "$GENERATOR" "$STAGING"

echo "创建 dmg：${DMG_PATH}"
rm -f "$DMG_PATH"
hdiutil create -volname "$VOL_NAME" -srcfolder "$STAGING" -fs HFS+ -ov -format UDZO "$DMG_PATH" >/dev/null

echo "只读挂载复核布局…"
hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT_DIR" "$DMG_PATH" >/dev/null
MOUNTED="$MOUNT_DIR"
[[ -d "$MOUNT_DIR/$APP_BUNDLE" ]] || fail "dmg 内缺少 ${APP_BUNDLE}"
[[ -L "$MOUNT_DIR/Applications" ]] || fail "dmg 内缺少 Applications 符号链接"
[[ "$(readlink "$MOUNT_DIR/Applications")" == "/Applications" ]] || fail "Applications 链接目标不是 /Applications"
[[ -f "$MOUNT_DIR/.DS_Store" ]] || fail "dmg 内缺少 .DS_Store"
hdiutil detach "$MOUNT_DIR" >/dev/null
MOUNTED=""

echo "dmg 生成完成：${DMG_PATH}"
