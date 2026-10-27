//! 端口占用者识别（尽力而为，三平台）。
//!
//! 用 `listeners` crate（Linux `rustix` / macOS `byteorder` / Windows `windows`，
//! 无 bindgen）。只使用 `pid` 与 `name`——`Process` 带完整路径，打进日志不合适。

/// 占用端口的进程（脱敏后的最小信息）。
#[derive(Debug, Clone)]
pub struct Holder {
    pub pid: u32,
    pub name: String,
}

/// 查找监听指定端口的进程；返回首个（pid 最小）与其余数量。
///
/// 任何失败都返回 `None`：诊断永远不能影响退出行为。
pub fn find(port: u16) -> Option<(Holder, usize)> {
    let mut processes: Vec<_> = listeners::get_processes_by_port(port).ok()?.into_iter().collect();
    if processes.is_empty() {
        return None;
    }
    processes.sort_by_key(|process| process.pid);
    let more = processes.len() - 1;
    let first = processes.into_iter().next()?;
    Some((
        Holder {
            pid: first.pid,
            name: first.name,
        },
        more,
    ))
}
