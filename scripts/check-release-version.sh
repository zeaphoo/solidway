#!/usr/bin/env bash
# 发布版本一致性闸门：清单之间必须一致；给了 tag 时还要 v<version> == tag。
#
# 用法：
#   ./scripts/check-release-version.sh                # 只校验清单一致性，打印裸版本
#   ./scripts/check-release-version.sh v0.1.0         # 额外校验 tag
#
# 成功时把裸版本号打到 stdout（CI 作为 job output 使用），失败以非零码退出。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAG="${1:-}"

if [[ -n "$TAG" && "$TAG" != v* ]]; then
    echo "标签必须形如 v<version>，收到：$TAG" >&2
    exit 1
fi

conf_version="$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*$/\1/p' \
    "$ROOT/app/tauri.conf.json" | head -n1)"
if [[ -z "$conf_version" ]]; then
    echo "无法读取 app/tauri.conf.json 的 version" >&2
    exit 1
fi

manifests=(
    "$ROOT/crates/solidway/Cargo.toml"
    "$ROOT/crates/solidway_api/Cargo.toml"
    "$ROOT/crates/solidway_core/Cargo.toml"
    "$ROOT/crates/solidway_egress/Cargo.toml"
    "$ROOT/crates/solidway_transcode/Cargo.toml"
    "$ROOT/app/Cargo.toml"
)

failed=0
for manifest in "${manifests[@]}"; do
    if [[ ! -f "$manifest" ]]; then
        echo "缺少清单：$manifest" >&2
        failed=1
        continue
    fi
    version="$(awk -F'"' '/^version = "/{print $2; exit}' "$manifest")"
    if [[ "$version" != "$conf_version" ]]; then
        echo "版本不一致：app/tauri.conf.json=$conf_version, ${manifest#"$ROOT"/}=${version:-<空>}" >&2
        failed=1
    fi
done

if [[ "$failed" -ne 0 ]]; then
    exit 1
fi

if [[ -n "$TAG" && "$TAG" != "v$conf_version" ]]; then
    echo "标签与版本不一致：tag=$TAG, 清单期望=v$conf_version" >&2
    exit 1
fi

echo "$conf_version"
