<div align="right">

**English** · [中文](CONTRIBUTING.zh.md)

</div>

# Contributing

Thanks for taking a look. This repository is spec-driven: behaviour is agreed in `openspec/` before it is implemented.

## Before you start

- Open an issue for anything larger than a bug fix, so the behaviour can be discussed before code exists.
- Read [`AGENTS.md`](AGENTS.md) for the project's working rules and [`docs/system-patterns.md`](docs/system-patterns.md) for the engineering conventions already in use.

## Behaviour changes go through OpenSpec

```bash
# 1) Propose: creates openspec/changes/<date>-<slug>/
openspec propose <slug>

# 2) Fill in proposal / design / tasks and the spec delta, then implement

# 3) Verify the change set
openspec validate --all

# 4) Archive once merged
openspec archive <change>
```

`openspec/specs/` is the source of truth for what the system does. If a change is not described there, it needs a proposal first — including "small" behaviour tweaks.

## Required checks

Run these before opening a pull request; CI runs the same set:

```bash
cargo test --workspace          # Rust
cargo clippy --workspace -- -D warnings
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
pnpm --dir web test             # pure-function tests
openspec validate --all
```

Front-end code is linted with `pnpm --dir web lint`.

## Code conventions

- Comments and documentation are in Chinese; identifiers and log fields are English (`key=value`).
- Log lines use `tracing`; user-visible strings in the console come from i18n resources, never hard-coded.
- Keep modules under ~500 lines; prefer a new module over a growing one.
- New dependencies need a reason in the pull request: what it replaces and why the standard library or an existing crate is not enough.
- Do not commit secrets, `*.db` files, `master.key`, build output or local data directories — `.gitignore` covers them.

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `chore:`, `ci:`, `refactor:`, `test:`. Scope is optional (`feat(app): …`).
- One logical change per pull request. Include the verification you ran in the description.
- If a change touches specified behaviour, link the OpenSpec change directory.

## Reporting bugs

Include:

- version (footer of the console, or `solidway --help` output if the binary does not start),
- platform and how it was installed (package, archive, desktop app),
- what you expected, what happened, and the relevant part of `solidway.log` (redact tokens and credentials).
