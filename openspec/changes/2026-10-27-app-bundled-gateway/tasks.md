# Tasks — app-bundled-gateway

> 对应提案：`openspec/changes/2026-10-27-app-bundled-gateway/`。

## 1. 定位逻辑

- [x] 1.1 `app/src/config.rs`：`locate_solidway(resource_dir)` + 纯函数 `locate_in(exe, resource_dir, dev, path_dirs)`，顺序 = 同目录 → 资源目录 → dev（仅 debug）→ PATH
- [x] 1.2 候选去重（Windows 资源目录 == 同目录）；`seen` 防止重复列同一路径
- [x] 1.3 发布构建不列开发期路径（`cfg!(debug_assertions)`）
- [x] 1.4 `app/src/main.rs`：启动时取 `handle.path().resource_dir()` 传给定位函数
- [x] 1.5 单测 5 条：同目录优先 / 资源目录次之 / dev 仅 debug / PATH 兜底 / 重复目录去重

## 2. Linux 客户端打包

- [x] 2.1 `app/tauri.linux.conf.json`：`bundle.resources` 注入 `solidway`
- [x] 2.2 `scripts/package-linux-app.sh`：构建网关 → 临时放入 `app/solidway` → tauri build → 清理
- [x] 2.3 deb 包名重写为 `solidway-app`（`dpkg-deb -R/-b`，`--root-owner-group`），文件名 `solidway-app_<ver>_<arch>.deb`
- [x] 2.4 AppImage 保持 bundler 命名，内含 `usr/lib/Solidway/solidway`
- [x] 2.5 本机验证：deb `Package: solidway-app`；解包到临时前缀运行，应用成功拉起自带网关（系统无 `/usr/bin/solidway`）

## 3. 文档与规格

- [x] 3.1 ADR 0018 决策 5 改为「三平台各自自带网关，落点统一由资源目录承接」
- [x] 3.2 `packaging/README.md`：Linux 客户端自包含；服务包可选；包名 `solidway-app`
- [x] 3.3 `docs/tech-context.md` 发布段落同步
- [x] 3.4 `docs/system-patterns.md` 记录「网关二进制落点 + 资源目录查找」约定
- [x] 3.5 更新仍打开的 `release-pipeline-v-tags`（产物集：Linux 客户端自带网关、包名改名）
