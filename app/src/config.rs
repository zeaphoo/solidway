//! 桌面壳配置：`data_root`、二进制定位、健康探测。
//!
//! 端口**不再**从配置文件读取——由子进程的 `solidway: listening addr=...`
//! 事件告知（见 `supervisor`）。

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// 桌面壳运行参数。
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// solidway 的 data_root。
    pub data_root: PathBuf,
    /// 子进程失败时是否自动重启（当前固定开启）。
    pub restart_on_failure: bool,
    /// 静默启动（登录自启传入 `--hidden`）：不主动弹窗，失败仍可见。
    pub hidden: bool,
}

impl AppConfig {
    pub fn load(app: &AppHandle) -> Result<Self, String> {
        let data_root = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("无法解析应用数据目录：{error}"))?;
        std::fs::create_dir_all(&data_root)
            .map_err(|error| format!("创建 data_root 失败 {}：{error}", data_root.display()))?;

        // 不使用配置文件或应用专属环境变量：异常退出后固定按退避策略重启。
        Ok(Self {
            data_root,
            restart_on_failure: true,
            hidden: hidden_requested(),
        })
    }
}

/// 只认精确的 `--hidden` token（`--hidden=true` 不算）。
fn hidden_requested() -> bool {
    // `args_os` 避免非 UTF-8 参数 panic。
    std::env::args_os().any(|arg| arg == "--hidden")
}

/// 定位 `solidway` 可执行文件。
///
/// 顺序：应用可执行文件同目录 → 应用资源目录 → 开发期工作区构建产物 → `PATH`。
///
/// 资源目录来自 Tauri（`app.path().resource_dir()`），是打包器放置 `bundle.resources`
/// 的位置：Linux deb/rpm 为 `/usr/lib/<产品名>`、AppImage 为 `$APPDIR/usr/lib/<产品名>`、
/// macOS 为 `Contents/Resources`、Windows 为安装目录（与可执行文件同目录，会被去重）。
/// 客户端包因此可以自带网关二进制，不再要求用户先装服务包。
pub fn locate_solidway(resource_dir: Option<&Path>) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().ok();
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    locate_in(
        exe.as_deref(),
        resource_dir,
        cfg!(debug_assertions),
        &path_dirs,
    )
}

/// 定位实现（纯路径拼装 + 存在性检查，便于单测）。
///
/// `dev` 为真时才尝试开发期路径（`app/target/<profile>` → 仓库根 `target/<profile>`）；
/// 发布构建里不再出现这些候选，报错信息只列出真实尝试过的位置。
fn locate_in(
    exe: Option<&Path>,
    resource_dir: Option<&Path>,
    dev: bool,
    path_dirs: &[PathBuf],
) -> Result<PathBuf, String> {
    let names = executable_names();
    let mut tried: Vec<String> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();

    if let Some(dir) = exe.and_then(Path::parent) {
        if let Some(found) = find_in(dir, &names, &mut tried, &mut seen) {
            return Ok(found);
        }
    }

    if let Some(dir) = resource_dir {
        if let Some(found) = find_in(dir, &names, &mut tried, &mut seen) {
            return Ok(found);
        }
    }

    if dev {
        if let Some(dir) = exe.and_then(Path::parent) {
            // dev：app/target/<profile>/solidway-app → 仓库根 target/<profile>/solidway
            for profile in ["debug", "release"] {
                let candidate_dir = dir.join("..").join("..").join("..").join("target").join(profile);
                if let Some(found) = find_in(&candidate_dir, &names, &mut tried, &mut seen) {
                    return Ok(found);
                }
            }
        }
    }

    for dir in path_dirs {
        if let Some(found) = find_in(dir, &names, &mut tried, &mut seen) {
            return Ok(found);
        }
    }
    tried.push("PATH 各目录下的 solidway".to_string());

    Err(format!(
        "未找到 solidway 可执行文件。已尝试：\n  - {}",
        tried.join("\n  - ")
    ))
}

/// 在单个目录里按候选名查找；`seen` 用于去重（Windows 的资源目录就是 exe 目录）。
fn find_in(
    dir: &Path,
    names: &[&'static str],
    tried: &mut Vec<String>,
    seen: &mut Vec<PathBuf>,
) -> Option<PathBuf> {
    for name in names {
        let candidate = dir.join(name);
        if seen.iter().any(|item| item == &candidate) {
            continue;
        }
        if candidate.is_file() {
            return Some(resolved(candidate));
        }
        seen.push(candidate.clone());
        tried.push(candidate.display().to_string());
    }
    None
}

/// 规范化路径（消掉 `..` 段），失败时原样返回。
fn resolved(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn executable_names() -> [&'static str; 2] {
    if cfg!(windows) {
        ["solidway.exe", "solidway"]
    } else {
        ["solidway", "solidway.exe"]
    }
}

/// 后台健康探测：本地回环 `GET /api/health`，仅判断状态行是否为 2xx。
///
/// 自实现极小 HTTP 客户端（只做一次 GET），避免为本地探测引入 async / HTTP 依赖。
pub fn health_ok(addr: &str) -> bool {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;

    let Ok(addr) = addr.parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(600)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(600)));
    let request = format!(
        "GET /api/health HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        addr
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut buffer = [0u8; 64];
    let Ok(read) = stream.read(&mut buffer) else {
        return false;
    };
    let head = String::from_utf8_lossy(&buffer[..read]);
    head.starts_with("HTTP/1.1 2") || head.starts_with("HTTP/1.0 2")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 建一个唯一临时目录（测试结束由调用方删除）。
    fn temp_tree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "solidway-locate-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(path, b"").expect("write file");
    }

    #[test]
    fn exe_dir_wins() {
        let root = temp_tree("exe");
        let exe_dir = root.join("bin");
        let exe = exe_dir.join("solidway-app");
        touch(&exe);
        touch(&exe_dir.join("solidway"));

        let found = locate_in(Some(&exe), None, false, &[]).expect("应找到同目录二进制");
        assert_eq!(found, fs::canonicalize(exe_dir.join("solidway")).unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resource_dir_is_checked_before_path() {
        let root = temp_tree("resource");
        let exe_dir = root.join("usr/bin");
        let exe = exe_dir.join("solidway-app");
        touch(&exe);
        // deb 布局：/usr/bin/solidway-app + /usr/lib/Solidway/solidway
        let resource = root.join("usr/lib/Solidway");
        touch(&resource.join("solidway"));
        let path_dir = root.join("path-dir");
        touch(&path_dir.join("solidway"));

        let found = locate_in(Some(&exe), Some(&resource), false, &[path_dir]).expect("应找到资源目录");
        assert_eq!(found, fs::canonicalize(resource.join("solidway")).unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dev_target_only_in_debug() {
        let root = temp_tree("dev");
        let exe = root.join("app/target/debug/solidway-app");
        touch(&exe);
        let repo_bin = root.join("target/release/solidway");
        touch(&repo_bin);

        let found = locate_in(Some(&exe), None, true, &[]).expect("debug 应回落到仓库 target");
        assert_eq!(found, fs::canonicalize(repo_bin).unwrap());

        let error = locate_in(Some(&exe), None, false, &[]).expect_err("release 不应查开发期路径");
        // exe 自身位于 app/target/debug，报错里出现它是正常的；要排除的是仓库根 target/。
        let repo_candidate = format!("{}/target/", root.display());
        assert!(
            !error.contains(&repo_candidate),
            "release 报错不应出现开发期路径：{error}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn path_is_last_resort() {
        let root = temp_tree("path");
        let exe = root.join("bin/solidway-app");
        touch(&exe);
        let path_dir = root.join("path-dir");
        touch(&path_dir.join("solidway"));

        let found = locate_in(Some(&exe), None, false, &[path_dir]).expect("应回落到 PATH");
        assert_eq!(found, fs::canonicalize(root.join("path-dir/solidway")).unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_reports_tried_dirs_once() {
        let root = temp_tree("missing");
        let exe_dir = root.join("bin");
        let exe = exe_dir.join("solidway-app");
        touch(&exe);

        // 资源目录与 exe 目录相同（Windows 语义）：同一目录只应出现一轮候选
        // （solidway / solidway.exe 两条），而不是两轮。
        let error = locate_in(Some(&exe), Some(&exe_dir), false, &[]).expect_err("应失败");
        let listed = error
            .lines()
            .filter(|line| line.contains(&exe_dir.display().to_string()))
            .count();
        assert_eq!(listed, 2, "重复目录应去重：{error}");
        let _ = fs::remove_dir_all(&root);
    }
}
