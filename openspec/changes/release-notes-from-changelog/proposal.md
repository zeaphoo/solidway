# Proposal: release-notes-from-changelog

## Why

发布说明此前由 `softprops/action-gh-release` 的 `generate_release_notes: true` 生成：仓库里没有任何可审阅、可版本化的变更记录，Release 正文取决于 GitHub 对 commit / PR 的自动汇总。这带来三个问题：

1. 变更说明**不在仓库里**，无法随代码 review，也无法在离线环境查看；
2. 首次发布（本仓库第一个 tag）时自动汇总会把全部历史提交混在一起，无法表达「这个版本对用户意味着什么」；
3. 预发布/正式版之间的说明没有统一格式，维护者需要每次凭记忆手工撰写。

## What Changes

- 新增 `CHANGELOG.md`，格式遵循 [Keep a Changelog 1.1.0](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 semver：`## [Unreleased]` + 每个版本一个 `## [<version>] - <date>` 段，段内按 Added / Changed / Deprecated / Removed / Fixed / Security 分类。
- 新增 `scripts/release-notes.sh`：从 `CHANGELOG.md` 抽取指定版本条目（到下一个 `## [` 或文件末尾链接定义为止）；缺该版本条目时非零退出。不传版本参数时取 `scripts/check-release-version.sh` 输出的当前版本。
- 发布工作流接入：
  - `verify` job 增加「Check CHANGELOG has this version」步骤：版本条目缺失即失败，**不进入构建**；
  - `release` job 用 `release-notes.sh` 生成 `release-notes.md` 并以 `body_path` 作为 Release 正文，移除 `generate_release_notes`。
- 文档：`packaging/README.md` 发布流程加入「写 CHANGELOG 条目」步骤；README 中英「延伸阅读」补 CHANGELOG 链接。

## Capabilities

### New Capabilities

（无）

### Modified Capabilities

- `release-pipeline`：ADDED「发布说明来源」——Release 正文 MUST 来自 `CHANGELOG.md` 的对应版本条目，缺失时发布闸门 MUST 在任何构建开始前失败。

## Impact

- 新增文件：`CHANGELOG.md`、`scripts/release-notes.sh`。
- CI：`.github/workflows/release.yml`（verify 增一步、release 改 `body_path`）。
- 文档：`packaging/README.md`、`README.md`、`README.zh.md`。
- 维护约定：**版本提交必须同时补 CHANGELOG 条目**，否则 tag 发布在 verify 阶段直接失败（快速失败，不浪费构建时间）。
