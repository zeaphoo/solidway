<div align="right">

[English](CONTRIBUTING.md) · **中文**

</div>

# 参与贡献

欢迎关注这个项目。本仓库是规格驱动的：行为先在 `openspec/` 里达成一致，再写实现。

## 开始之前

- 超过 bug fix 规模的改动，请先开 issue，让行为在动手前先讨论清楚。
- 动手前先读 [`AGENTS.md`](AGENTS.md)（项目工作规则）与 [`docs/system-patterns.md`](docs/system-patterns.md)（已在使用的工程约定）。

## 行为变更走 OpenSpec

```bash
# 1) 提案：创建 openspec/changes/<date>-<slug>/
openspec propose <slug>

# 2) 补齐 proposal / design / tasks 与 spec delta，然后实现

# 3) 校验变更集
openspec validate --all

# 4) 合并后归档
openspec archive <change>
```

`openspec/specs/` 是「系统做什么」的唯一事实来源。规格里没写的改动（包括看起来很小的行为调整）都需要先有提案。

## 必须通过的检查

提 PR 前跑这些；CI 跑的是同一组：

```bash
cargo test --workspace          # Rust
cargo clippy --workspace -- -D warnings
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
pnpm --dir web test             # 纯函数测试
openspec validate --all
```

前端代码用 `pnpm --dir web lint` 检查。

## 代码约定

- 注释与文档用中文；标识符与日志字段用英文（`key=value`）。
- 日志走 `tracing`；控制台里用户可见的文案一律来自 i18n 资源，禁止硬编码。
- 单文件控制在约 500 行以内；宁可拆模块，也不要让一个文件持续膨胀。
- 新增依赖需要在 PR 里说明理由：替代了什么、为什么标准库或现有 crate 不够。
- 不要提交密钥、`*.db`、`master.key`、构建产物或本地数据目录——`.gitignore` 已覆盖这些。

## 提交与 PR

- 使用 [Conventional Commits](https://www.conventionalcommits.org/)：`feat:`、`fix:`、`docs:`、`chore:`、`ci:`、`refactor:`、`test:`；scope 可选（如 `feat(app): …`）。
- 一个 PR 只做一件逻辑上的事；描述里写明你跑过的验证。
- 若改动涉及规格中的行为，请附上对应的 OpenSpec change 目录。

## 报障

请提供：

- 版本（控制台页脚；若二进制起不来，给 `solidway --help` 的输出）；
- 平台与安装方式（安装包 / 归档 / 桌面应用）；
- 期望行为、实际行为，以及 `solidway.log` 的相关片段（记得抹掉令牌与凭据）。
