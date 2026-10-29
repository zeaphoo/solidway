#!/usr/bin/env bash
# 构建 macOS 桌面应用：Solidway.app + solidway-app_<version>_<arch>.dmg。
#
# 依赖：cargo、pnpm、tauri-cli、codesign、hdiutil、security。
# 可用环境变量：
#   SKIP_WEB_BUILD=1     跳过 pnpm build（要求 web/dist 已存在）
#   DIST_DIR=...         输出目录，默认 <repo>/dist/mac
#   MAC_SIGN_IDENTITY=... 签名身份，默认 "AKSL Software"
#
# 顺序（不可调换，否则 dmg 会覆盖 post-copy / 签名结果）：
#   1. 根 workspace 构建外置 solidway 二进制
#   2. app/ 独立项目经 tauri-cli 产出 .app
#   3. post-copy solidway 进 Contents/MacOS/
#   4. 内层 Mach-O 先签、.app 外层后签
#   5. 从已签名的 .app 用 hdiutil 生成 dmg（禁用 --bundles dmg）
#   6. 挂载 dmg 断言内含 solidway 且签名校验通过

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT/dist/mac}"
IDENTITY="${MAC_SIGN_IDENTITY:-AKSL Software}"
APP_NAME="Solidway"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "package-mac.sh 仅支持 macOS" >&2
    exit 1
fi

for command in cargo pnpm codesign hdiutil security node file; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "缺少命令: $command" >&2
        exit 1
    fi
done

if ! cargo tauri --version >/dev/null 2>&1; then
    echo "缺少 tauri-cli；请执行: cargo install tauri-cli --locked" >&2
    exit 1
fi

# 版本一致性（D10）：tauri.conf.json 与全部 crates 必须一致。
conf_version="$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*$/\1/p' "$ROOT/app/tauri.conf.json" | head -n1)"
if [[ -z "$conf_version" ]]; then
    echo "无法读取 app/tauri.conf.json 的 version" >&2
    exit 1
fi
for manifest in \
    "$ROOT/crates/solidway/Cargo.toml" \
    "$ROOT/crates/solidway_api/Cargo.toml" \
    "$ROOT/crates/solidway_core/Cargo.toml" \
    "$ROOT/app/Cargo.toml"; do
    version="$(awk -F'"' '/^version = "/{print $2; exit}' "$manifest")"
    if [[ "$version" != "$conf_version" ]]; then
        echo "版本不一致：app/tauri.conf.json=$conf_version, $manifest=$version" >&2
        exit 1
    fi
done

# 签名身份必须存在（D6：禁 ad-hoc，不存在即失败）。
if ! security find-identity -v -p codesigning 2>/dev/null | grep -qF "\"$IDENTITY\""; then
    echo "找不到代码签名身份：$IDENTITY（禁 ad-hoc；请先导入含完整链的 p12）" >&2
    exit 1
fi

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
    # 兼容调用方自定义 CARGO_TARGET_DIR。
    (cd "$1" && cargo metadata --no-deps --format-version 1 |
        node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')
}

cd "$ROOT"
cargo build --release -p solidway
ROOT_TARGET="$(target_dir "$ROOT")"

(cd "$ROOT/app" && cargo tauri build --bundles app)
APP_TARGET="$(target_dir "$ROOT/app")"
APP_SRC="$APP_TARGET/release/bundle/macos/$APP_NAME.app"
if [[ ! -d "$APP_SRC" ]]; then
    echo "Tauri bundler 未产出 $APP_SRC" >&2
    exit 1
fi

# post-copy（D9）：solidway 必须与 app 可执行文件同目录。
# 主程序名含大写（SolidwayApp），网关是小写 solidway —— 在大小写不敏感的 APFS 上
# 也不能是只差大小写的名字，这里在复制后显式断言两个文件都存在。
cp -f "$ROOT_TARGET/release/solidway" "$APP_SRC/Contents/MacOS/solidway"
chmod +x "$APP_SRC/Contents/MacOS/solidway"
for name in SolidwayApp solidway; do
    if [[ ! -f "$APP_SRC/Contents/MacOS/$name" ]]; then
        echo "app 包内缺少 Contents/MacOS/$name（检查主程序名是否与 solidway 只差大小写）" >&2
        exit 1
    fi
    if [[ -L "$APP_SRC/Contents/MacOS/$name" ]]; then
        echo "app 包内 Contents/MacOS/$name 是符号链接；签名循环只处理常规文件，拒绝静默跳过" >&2
        exit 1
    fi
done

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"
APP="$DIST_DIR/$APP_NAME.app"
ditto "$APP_SRC" "$APP"

# 内层 Mach-O 先签，再签 .app 本体（D6；禁 ad-hoc）。
#
# 先快照候选再签：签名会改写目录内容（codesign 写临时文件后 rename），
# 边 find 边签可能在部分 runner 上漏掉后续条目；漏签又会被下面的
# signed_inner 汇总掩盖，直到外层签名才以 "In subcomponent" 报错。
# Contents/MacOS 下只应有可执行 Mach-O；这里逐个签、逐个 codesign --verify，
# 失败立即带原始输出退出（不再用 file 的结果做过滤）。
inner_candidates=()
while IFS= read -r -d '' candidate; do
    inner_candidates+=("$candidate")
done < <(find "$APP/Contents/MacOS" -maxdepth 1 -type f -print0)
if [[ "${#inner_candidates[@]}" -eq 0 ]]; then
    echo "$APP/Contents/MacOS 下没有文件" >&2
    exit 1
fi

signed_inner=0
for candidate in "${inner_candidates[@]}"; do
    echo "内层签名：$candidate（file: $(file -b "$candidate" 2>&1)）"
    if ! sign_output="$(codesign --force --options runtime -s "$IDENTITY" "$candidate" 2>&1 </dev/null)"; then
        echo "内层签名失败：$candidate" >&2
        echo "$sign_output" >&2
        exit 1
    fi
    if [[ -n "$sign_output" ]]; then
        echo "$sign_output"
    fi
    if ! verify_output="$(codesign --verify --strict "$candidate" 2>&1 </dev/null)"; then
        echo "内层签名校验失败：$candidate" >&2
        echo "$verify_output" >&2
        codesign -dv --verbose=2 "$candidate" 2>&1 >&2 || true
        exit 1
    fi
    signed_inner=$((signed_inner + 1))
done
if [[ "$signed_inner" -ne "${#inner_candidates[@]}" ]]; then
    echo "内层签名数量不符：$signed_inner/${#inner_candidates[@]}（不应发生）" >&2
    exit 1
fi
echo "内层签名并校验通过：$signed_inner/${#inner_candidates[@]} 个文件"

if ! sign_output="$(codesign --force --options runtime -s "$IDENTITY" "$APP" 2>&1 </dev/null)"; then
    echo "外层签名失败：$APP" >&2
    echo "$sign_output" >&2
    for candidate in "${inner_candidates[@]}"; do
        echo "内层签名状态：$candidate" >&2
        codesign --verify --strict "$candidate" >&2 2>&1 || true
    done
    exit 1
fi
if [[ -n "$sign_output" ]]; then
    echo "$sign_output"
fi
if ! verify_output="$(codesign --verify --deep --strict "$APP" 2>&1 </dev/null)"; then
    echo "签名校验失败：$APP" >&2
    echo "$verify_output" >&2
    for candidate in "${inner_candidates[@]}"; do
        echo "内层签名状态：$candidate" >&2
        codesign -dv --verbose=2 "$candidate" 2>&1 >&2 || true
    done
    exit 1
fi

ARCH="$(uname -m)"
DMG="$DIST_DIR/solidway-app_${conf_version}_${ARCH}.dmg"
hdiutil create -volname "$APP_NAME" -srcfolder "$APP" -ov -format UDZO "$DMG" >/dev/null

# dmg 内容断言（R1）：必须是 post-copy + 签名后的最终态。
MOUNT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/solidway-dmg.XXXXXX")"
cleanup_dmg() {
    hdiutil detach "$MOUNT_DIR" >/dev/null 2>&1 || true
    rmdir "$MOUNT_DIR" 2>/dev/null || true
}
trap cleanup_dmg EXIT
hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT_DIR" "$DMG" >/dev/null
if [[ ! -f "$MOUNT_DIR/$APP_NAME.app/Contents/MacOS/solidway" ]]; then
    echo "dmg 内缺少 $APP_NAME.app/Contents/MacOS/solidway" >&2
    exit 1
fi
codesign --verify --deep --strict "$MOUNT_DIR/$APP_NAME.app"
hdiutil detach "$MOUNT_DIR" >/dev/null
rmdir "$MOUNT_DIR"
trap - EXIT

if [[ ! -d "$APP" || ! -f "$DMG" ]]; then
    echo "产物缺失：$APP / $DMG" >&2
    exit 1
fi

echo
echo "生成完成："
echo "  $APP"
echo "  $DMG"
codesign -dv --verbose=2 "$APP" 2>&1 | grep -E 'Authority|flags=' || true
