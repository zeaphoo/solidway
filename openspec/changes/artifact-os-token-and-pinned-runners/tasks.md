# Tasks

## 1. 产物名加 OS 标记

- [x] 1.1 `scripts/package-linux.sh`：deb 在最后一个 `_` 前插入 `linux`，rpm 在最后一个 `.` 前插入 `linux`。验证：本地产出 `solidway_0.5.0~rc.1-1_linux_amd64.deb`、`solidway-0.5.0~rc.1-1.linux.x86_64.rpm`
- [x] 1.2 `scripts/package-linux-app.sh`：AppImage / deb 名称插入 `linux`
- [x] 1.3 `scripts/package-mac.sh`：dmg 名称插入 `macos`
- [x] 1.4 `scripts/package-windows.sh`：setup.exe / msi / portable zip 名称插入 `windows`
- [x] 1.5 四个脚本 `bash -n` 通过

## 2. 固定构建机

- [x] 2.1 `.github/workflows/release.yml`：全部 Ubuntu job 改 `ubuntu-22.04`（verify / linux-service / linux-app / release），Windows job 改 `windows-2022`
- [x] 2.2 `.github/workflows/ci.yml`：Ubuntu matrix 与两个 Ubuntu job 改 `ubuntu-22.04`，Windows job 改 `windows-2022`，macOS matrix 改 `macos-14`（发布侧 mac 矩阵本已固定 `macos-14` / `macos-15-intel`）
- [x] 2.3 两个 workflow YAML 解析通过

## 3. 文档

- [x] 3.1 `packaging/README.md`：产物表、命名约定（`<os>` 取值与理由）、安装示例、三平台固定 runner 表（含 glibc 2.35 基线）与「不用 `-latest`」说明
- [x] 3.2 `CHANGELOG.md`：`[Unreleased]` 记 Changed 条目

## 4. 归档

- [ ] 4.1 下一次发布后归档本 change 并把 delta 合并进 `openspec/specs/release-pipeline/spec.md`
