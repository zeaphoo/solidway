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
#   4. 由已钉票 .app 重建 dmg（dmg 有独立校验和，不能复用 .app 的公证票据）
#   5. dmg 单独提交 notarytool，等待 Accepted；失败即带 notarytool log 退出
#   6. 钉票 dmg 并 stapler validate；挂载断言 + spctl 验收挂载内 .app

# 注意：GNU bash 3.2 在 set -u 展开错误时若存在 EXIT trap，可能以 0 退出
# （trap 内 $? 已为 0）。因此 release.yml 另设独立 "Verify notarized dmg"
# 闸门，不依赖本脚本的退出语义；本脚本只负责公证、钉票与自查。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT/dist/mac}"
APP_NAME="Solidway"
APP="$DIST_DIR/$APP_NAME.app"
TIMEOUT="${NOTARY_TIMEOUT:-30m}"
ACCEPTED_STATUS="accepted"
ACCEPTED_SOURCE="source=Notarized Developer ID"

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
    fail "缺少已签名 app：${APP}（先执行 scripts/package-mac.sh）"
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

# 提交一个待公证产物（app zip 或 dmg）并等待 Accepted；失败带 notarytool log 后退出。
notarize_artifact() {
    local artifact="$1"
    local label="$2"
    local submit_json submission_id info_json status

    echo "提交${label}公证…"
    if ! submit_json="$(xcrun notarytool submit "$artifact" "${notary_args[@]}" --output-format json)"; then
        fail "公证提交失败：${label}（${artifact}）"
    fi
    submission_id="$(printf '%s\n' "$submit_json" | plutil -extract id raw -o - - 2>/dev/null || true)"
    if [[ -z "$submission_id" ]]; then
        echo "$submit_json" >&2
        fail "notarytool submit 未返回 submission id（${label}）"
    fi

    echo "等待公证结果（submission=${submission_id}，超时预算 ${TIMEOUT}）…"
    if ! xcrun notarytool wait "$submission_id" "${notary_args[@]}" --timeout "$TIMEOUT"; then
        xcrun notarytool log "$submission_id" "${notary_args[@]}" || true
        fail "公证等待失败或超时（submission=${submission_id}，${label}）"
    fi

    if ! info_json="$(xcrun notarytool info "$submission_id" "${notary_args[@]}" --output-format json)"; then
        xcrun notarytool log "$submission_id" "${notary_args[@]}" || true
        fail "公证状态查询失败（submission=${submission_id}，${label}）"
    fi
    status="$(printf '%s\n' "$info_json" | plutil -extract status raw -o - - 2>/dev/null || true)"
    if [[ "$status" != "Accepted" ]]; then
        xcrun notarytool log "$submission_id" "${notary_args[@]}" || true
        fail "公证未通过：status=${status:-unknown}（submission=${submission_id}，${label}）"
    fi
    echo "公证通过：${label} status=${status}"
}

ZIP="$WORK_DIR/$APP_NAME.zip"
echo "打包公证提交物：$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

notarize_artifact "$ZIP" "app zip"

# 先钉票 .app，再由已钉票 .app 重建 dmg：双钉票，离线首开依赖本地票据。
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"

ARCH="$(uname -m)"
DMG="$DIST_DIR/solidway-app_${version}_${ARCH}.dmg"
rm -f "$DMG"
echo "由已钉票 .app 重建 dmg：$DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$APP" -ov -format UDZO "$DMG" >/dev/null
# 重建后的 dmg 有独立校验和，必须单独提交公证后再钉票；
# 否则 stapler staple 会报 Error 65（CloudKit Record not found）。
notarize_artifact "$DMG" "dmg"
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"

hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT_DIR" "$DMG" >/dev/null
if [[ ! -f "$MOUNT_DIR/$APP_NAME.app/Contents/MacOS/solidway" ]]; then
    fail "dmg 内缺少 $APP_NAME.app/Contents/MacOS/solidway"
fi
codesign --verify --deep --strict "$MOUNT_DIR/$APP_NAME.app"
xcrun stapler validate "$MOUNT_DIR/$APP_NAME.app"

# spctl 验收：dmg 本体没有 Mach-O 签名可评（会报 no usable signature），
# 只验收挂载后的 dmg 内 .app。
accepted() {
    # spctl 输出为多行：`<path>: accepted` 与 `source=Notarized Developer ID` 分行，
    # 不能按连续字符串匹配。
    grep -qF "$ACCEPTED_STATUS" <<<"$1" && grep -qF "$ACCEPTED_SOURCE" <<<"$1"
}

app_output="$(spctl -a -vv "$MOUNT_DIR/$APP_NAME.app" 2>&1 || true)"
if ! accepted "$app_output"; then
    echo "$app_output" >&2
    fail "spctl 验收失败：dmg 内 .app 输出不含 accepted / source=Notarized Developer ID"
fi
printf '%s\n' "$app_output" | grep -F "$ACCEPTED_SOURCE"

hdiutil detach "$MOUNT_DIR" >/dev/null
rmdir "$MOUNT_DIR" 2>/dev/null || true

echo
echo "公证与验收完成："
echo "  $APP"
echo "  $DMG"
