# Tasks

## 1. 变更记录文件

- [x] 1.1 新增 `CHANGELOG.md`（Keep a Changelog 1.1.0 格式）：`## [Unreleased]` + `## [0.5.0-rc.1] - 2026-10-30`，按 Added / Fixed 分类，末尾带 compare 链接定义。验证：`./scripts/release-notes.sh 0.5.0-rc.1` 输出 22 行正文

## 2. 抽取脚本

- [x] 2.1 新增 `scripts/release-notes.sh`：`## [<version>]` 起到下一个 `## [` 或末尾链接定义止；无参默认取清单版本；缺条目非零退出。验证：命中 / 缺版本（`9.9.9` 退出 1）/ 默认取版本 三种路径实测

## 3. 流水线接入

- [x] 3.1 `release.yml` verify job 增「Check CHANGELOG has this version」（失败即不构建）；release job 改用 `scripts/release-notes.sh` 产出 `release-notes.md` 并 `body_path`，移除 `generate_release_notes`。验证：`python3 -c "import yaml; yaml.safe_load(...)"` 通过

## 4. 文档

- [x] 4.1 `packaging/README.md` 发布流程补「写 CHANGELOG 条目」步骤与预发布版本写法；README 中英「延伸阅读」加 CHANGELOG 链接
- [ ] 4.2 归档本 change 并把 delta 合并进 `openspec/specs/release-pipeline/spec.md`（随下一次正式发布执行）
