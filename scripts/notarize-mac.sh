#!/usr/bin/env bash
# 对 scripts/package-mac.sh 产出的 dist/mac 做 Apple 公证、双钉票与 Gatekeeper 验收。
#
# 依赖：xcrun（notarytool / stapler）、ditto、hdiutil、spctl、codesign、plutil。
# 必需环境变量（App Store Connect API key）：
#   MAC_NOTARY_API_KEY_ID          API key id
#   MAC_NOTARY_API_ISSUER_ID       issuer id
#   MAC_NOTARY_API_KEY_P8_BASE64   .p8 私钥（base64）
#   （或 MAC_NOTARY_API_KEY_P8 指向已解码的 .p8 文件；两者都给时以路径为准）
# 可选环境变量：
#   DIST_DIR=...        输入/输出目录，默认 <repo>/dist/mac
#   NOTARY_TIMEOUT=30m  notarytool wait 的超时预算（超时即失败）
#
# 顺序（不可调换）：
#   1. 校验 .app 为 Developer ID 签名 + hardened runtime（公证前提）
#   2. zip 提交 notarytool，按 submission id 等待通过（超时即失败）
#   3. 钉票 .app 并 stapler validate
#   4. 由已钉票 .app 重建 dmg，钉票 dmg
#   5. 挂载断言 dmg 内含 solidway，spctl 验收 accepted source=Notarized Developer ID

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT/dist/mac}"
APP_NAME="Solidway"
APP="$DIST_DIR/$APP_NAME.app"
TIMEOUT="${NOTARY_TIMEOUT:-30m}"
ACCEPTED_MARKER="accepted source=Notarized Developer ID"

fail() {
    echo "$*" >&2
    exit 1
}

if [[ "$(uname -s)" != "Darwin" ]]; then
    fail "notarize-mac.sh 仅支持 macOS"
fi

for command in xcrun ditto hdiutil spctl codesign plutil; do
    if ! command -v "$command" >/dev/null 2>&1; then
        fail "缺少命令: $command"
    fi
done

# 凭据在动任何东西之前校验，避免白跑构建/上传。
missing=()
if [[ -z "${MAC_NOTARY_API_KEY_ID:-}" ]]; then
    missing+=("MAC_NOTARY_API_KEY_ID")
fi
if [[ -z "${MAC_NOTARY_API_ISSUER_ID:-}" ]]; then
    missing+=("MAC_NOTARY_API_ISSUER_ID")
fi
if [[ -z "${MAC_NOTARY_API_KEY_P8:-}" && -z "${MAC_NOTARY_API_KEY_P8_BASE64:-}" ]]; then
    missing+=("MAC_NOTARY_API_KEY_P8_BASE64")
fi
if (( ${#missing[@]} > 0 )); then
    echo "缺少公证凭据：${missing[*]}" >&2
    echo "需要 App Store Connect API key：MAC_NOTARY_API_KEY_ID / MAC_NOTARY_API_ISSUER_ID / MAC_NOTARY_API_KEY_P8_BASE64（或 MAC_NOTARY_API_KEY_P8 指向已解码的 .p8 文件）。" >&2
    echo "CI 请确认仓库 secrets 已配置（清单见 packaging/README.md）；本机请先 export 对应环境变量。" >&2
    exit 1
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/solidway-notarize.XXXXXX")"
MOUNT_DIR="$WORK_DIR/mnt"
mkdir -p "$MOUNT_DIR"

cleanup() {
    if [[ -d "$MOUNT_DIR" ]]; then
        hdiutil detach "$MOUNT_DIR" >/dev/null 2>&1 || true
        rmdir "$MOUNT_DIR" 2>/dev/null || true
    fi
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

KEY_FILE="${MAC_NOTARY_API_KEY_P8:-}"
if [[ -z "$KEY_FILE" ]]; then
    KEY_FILE="$WORK_DIR/notary-key.p8"
    (umask 077; printf '%s' "$MAC_NOTARY_API_KEY_P8_BASE64" | base64 --decode > "$KEY_FILE")
fi
if [[ ! -s "$KEY_FILE" ]]; then
    fail "公证密钥不存在或为空：$KEY_FILE"
fi
if ! grep -q 'BEGIN PRIVATE KEY' "$KEY_FILE"; then
    fail "公证密钥格式不正确（期望 .p8 PEM；检查 MAC_NOTARY_API_KEY_P8_BASE64 是否为完整 base64）"
fi

if [[ ! -d "$APP" ]]; then
    fail "缺少已签名 app：$APP（先执行 scripts/package-mac.sh）"
fi

version="$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*$/\1/p' "$ROOT/app/tauri.conf.json" | head -n1)"
if [[ -z "$version" ]]; then
    fail "无法读取 app/tauri.conf.json 的 version"
fi

# 公证前提：Developer ID 签名 + hardened runtime；本机自签应急产物不能公证。
codesign --verify --deep --strict "$APP"
signature_info="$(codesign -dv --verbose=2 "$APP" 2>&1)"
if ! grep -q 'Authority=Developer ID Application' <<<"$signature_info"; then
    echo "$signature_info" >&2
    fail "公证要求 Developer ID Application 签名；当前 .app 不是该身份（本机自签产物不能公证）"
fi
if ! grep -q 'flags=.*runtime' <<<"$signature_info"; then
    fail "公证要求 hardened runtime（codesign --options runtime）"
fi
echo "$signature_info" | grep -E 'Authority=|flags='

notary_args=(--key "$KEY_FILE" --key-id "$MAC_NOTARY_API_KEY_ID" --issuer "$MAC_NOTARY_API_ISSUER_ID")

ZIP="$WORK_DIR/$APP_NAME.zip"
echo "打包公证提交物：$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

echo "提交公证…"
submit_json="$(xcrun notarytool submit "$ZIP" "${notary_args[@]}" --output-format json)"
submission_id="$(printf '%s\n' "$submit_json" | plutil -extract id raw -o - - 2>/dev/null || true)"
if [[ -z "$submission_id" ]]; then
    echo "$submit_json" >&2
    fail "notarytool submit 未返回 submission id"
fi

echo "等待公证结果（submission=$submission_id，超时预算 $TIMEOUT）…"
if ! xcrun notarytool wait "$submission_id" "${notary_args[@]}" --timeout "$TIMEOUT"; then
    xcrun notarytool log "$submission_id" "${notary_args[@]}" || true
    fail "公证等待失败或超时（submission=$submission_id）"
fi

info_json="$(xcrun notarytool info "$submission_id" "${notary_args[@]}" --output-format json)"
status="$(printf '%s\n' "$info_json" | plutil -extract status raw -o - - 2>/dev/null || true)"
if [[ "$status" != "Accepted" ]]; then
    xcrun notarytool log "$submission_id" "${notary_args[@]}" || true
    fail "公证未通过：status=${status:-unknown}（submission=$submission_id）"
fi
echo "公证通过：status=$status"

# 先钉票 .app，再由已钉票 .app 重建 dmg：双钉票，离线首开依赖本地票据。
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"

ARCH="$(uname -m)"
DMG="$DIST_DIR/solidway-app_${version}_${ARCH}.dmg"
rm -f "$DMG"
echo "由已钉票 .app 重建 dmg：$DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$APP" -ov -format UDZO "$DMG" >/dev/null
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"

hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT_DIR" "$DMG" >/dev/null
if [[ ! -f "$MOUNT_DIR/$APP_NAME.app/Contents/MacOS/solidway" ]]; then
    fail "dmg 内缺少 $APP_NAME.app/Contents/MacOS/solidway"
fi
codesign --verify --deep --strict "$MOUNT_DIR/$APP_NAME.app"
xcrun stapler validate "$MOUNT_DIR/$APP_NAME.app"

# spctl 验收：最终 dmg（按平台差异尝试 open/install 形态）+ 挂载内 .app。
accepted() {
    grep -qF "$ACCEPTED_MARKER" <<<"$1"
}

dmg_output=""
for args in "-a -vv" "-a -vv -t install" "-a -vv -t open --context context:primary-signature"; do
    # shellcheck disable=SC2086 # 需要按空格拆分 spctl 参数
    candidate="$(spctl $args "$DMG" 2>&1 || true)"
    if accepted "$candidate"; then
        dmg_output="$candidate"
        break
    fi
    if [[ -n "$candidate" ]]; then
        dmg_output="$candidate"
    fi
done
if ! accepted "$dmg_output"; then
    echo "$dmg_output" >&2
    fail "spctl 验收失败：dmg 输出不含 \"$ACCEPTED_MARKER\""
fi
echo "$dmg_output" | grep -F "$ACCEPTED_MARKER"

app_output="$(spctl -a -vv "$MOUNT_DIR/$APP_NAME.app" 2>&1 || true)"
if ! accepted "$app_output"; then
    echo "$app_output" >&2
    fail "spctl 验收失败：挂载内 .app 输出不含 \"$ACCEPTED_MARKER\""
fi
echo "$app_output" | grep -F "$ACCEPTED_MARKER"

hdiutil detach "$MOUNT_DIR" >/dev/null
rmdir "$MOUNT_DIR" 2>/dev/null || true

echo
echo "公证与验收完成："
echo "  $APP"
echo "  $DMG"
