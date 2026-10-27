#!/usr/bin/env bash
# 构建 Linux systemd 服务安装包：deb + rpm。
#
# 依赖：cargo、pnpm、cargo-deb、cargo-generate-rpm。
# 可用环境变量：
#   SKIP_WEB_BUILD=1  跳过 pnpm build（要求 web/dist 已存在）
#   DIST_DIR=...      输出目录，默认 <repo>/dist/linux

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT/dist/linux}"

for command in cargo pnpm; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "缺少命令: $command" >&2
        exit 1
    fi
done

if ! cargo deb --version >/dev/null 2>&1; then
    echo "缺少 cargo-deb；请执行: cargo install cargo-deb --locked" >&2
    exit 1
fi
if ! cargo generate-rpm --version >/dev/null 2>&1; then
    echo "缺少 cargo-generate-rpm；请执行: cargo install cargo-generate-rpm --locked" >&2
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

cd "$ROOT"
cargo build --release -p solidway
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

cargo deb -p solidway --no-build --no-dbgsym --output "$DIST_DIR"
cargo generate-rpm -p crates/solidway --output "$DIST_DIR"

echo
echo "生成完成："
find "$DIST_DIR" -maxdepth 1 -type f \( -name '*.deb' -o -name '*.rpm' \) -printf '  %p\n' | sort
