# release-pipeline Specification (delta: release-notes-from-changelog)

## ADDED Requirements

### Requirement: 发布说明来源

仓库 MUST 以版本化文件 `CHANGELOG.md` 记录每个版本的变更说明，格式遵循 Keep a Changelog：`## [Unreleased]` 段 + 每个版本一个 `## [<version>] - <date>` 段，段内按 Added / Changed / Deprecated / Removed / Fixed / Security 分类；版本号 MUST 遵循语义化版本。

GitHub Release 的正文 MUST 取自 `CHANGELOG.md` 中与该标签版本对应的条目，MUST NOT 依赖 GitHub 自动生成的提交汇总。

抽取 MUST 由可独立执行的脚本提供（`scripts/release-notes.sh`），支持显式传入版本或默认取 `scripts/check-release-version.sh` 的当前版本；条目不存在时 MUST 以非零码退出。

发布工作流 MUST 在任何构建开始前校验该版本条目存在，缺失时 MUST 立即失败并 MUST NOT 产出任何发布物。

#### Scenario: 条目存在则作为正文
- **WHEN** 推送标签 `v0.5.0-rc.1` 且 `CHANGELOG.md` 含 `## [0.5.0-rc.1]` 段
- **THEN** Release 正文为该段内容，且正文不含相邻版本段落与文件末尾链接定义

#### Scenario: 条目缺失阻断发布
- **WHEN** 推送标签 `v0.5.0-rc.1` 但 `CHANGELOG.md` 无该版本条目
- **THEN** 版本校验阶段即失败，不进入构建、不产出 Release

#### Scenario: 本地抽取
- **WHEN** 维护者执行 `./scripts/release-notes.sh 0.5.0-rc.1`
- **THEN** 标准输出为该版本条目正文；版本不存在时以非零码退出并提示缺失的版本号
