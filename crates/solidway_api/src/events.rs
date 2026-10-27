//! stdout 事件契约（唯一机器可读输出）。
//!
//! 格式固定为 `solidway: <event> k=v ...`：ASCII、不本地化、**不经 `tracing`**
//! （因此不受 `RUST_LOG` 过滤影响）。消费方应"扫行找前缀"，不能假设是首行。

use std::io::Write;

use crate::net_holder::Holder;

fn emit(line: &str) {
    // 整行一次 `write`：stderr 与 stdout 可能落到同一个日志文件（O_APPEND），
    // 单次写入才能保证行不被别的写入截断。
    let mut payload = String::with_capacity(line.len() + 1);
    payload.push_str(line);
    payload.push('\n');
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(payload.as_bytes());
    let _ = out.flush();
}

/// 值里不能出现空白（否则破坏 `k=v` 的分词）。
fn token(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_whitespace() || c == '"' { '_' } else { c })
        .collect()
}

/// 监听成功：每次 bind 成功后恰好一行。
pub fn listening(addr: &str) {
    emit(&format!("solidway: listening addr={}", token(addr)));
}

/// 监听失败：随后进程应以非零码退出。
pub fn bind_failed(addr: &str, kind: &str, holder: Option<(&Holder, usize)>) {
    let mut line = format!(
        "solidway: bind-failed addr={} kind={}",
        token(addr),
        token(kind)
    );
    if let Some((holder, more)) = holder {
        line.push_str(&format!(" holder_pid={} holder_comm={}", holder.pid, token(&holder.name)));
        if more > 0 {
            line.push_str(&format!(" holder_more={more}"));
        }
    }
    emit(&line);
}

/// 请求托管方重启（仅 desktop 模式输出）。
pub fn restart_requested(reason: &str) {
    emit(&format!("solidway: restart-requested reason={}", token(reason)));
}

/// 请求托管方应用桌面偏好（仅 desktop 模式输出）。
///
/// `id` 由请求方生成并原样回写进 `<data_root>/desktop.status.json`，供控制台等待回执。
pub fn desktop_apply(id: &str, autostart: bool, close_to_tray: bool) {
    emit(&desktop_apply_line(id, autostart, close_to_tray));
}

/// `desktop-apply` 事件行（纯函数，便于断言格式）。
pub fn desktop_apply_line(id: &str, autostart: bool, close_to_tray: bool) -> String {
    format!(
        "solidway: desktop-apply id={} autostart={} close_to_tray={}",
        token(id),
        u8::from(autostart),
        u8::from(close_to_tray)
    )
}

/// admin 引导：一次性令牌与可直接打开的引导页地址。
///
/// 两种运行模式输出一致：desktop 由壳解析（且不写入日志），standalone 由终端直接可见。
/// `url` MUST 已包含 fragment，值中不能出现空白。
pub fn bootstrap(token_value: &str, url: &str) {
    emit(&format!(
        "solidway: bootstrap token={} url={}",
        token(token_value),
        token(url)
    ));
}

/// 桌面自动登录：把 webview 导航到该地址即可换取管理员会话（仅 desktop 模式）。
///
/// 行内含令牌：壳 MUST NOT 写入日志（与 `bootstrap` 同规格）。
pub fn desktop_login(url: &str) {
    emit(&format!("solidway: desktop-login url={}", token(url)));
}

/// 引导令牌已被消费（壳据此清掉引导页地址，后续使用桌面自动登录）。
pub fn bootstrap_consumed() {
    emit("solidway: bootstrap-consumed");
}

/// 把 `io::Error` 映射为稳定的 kind 名称。
pub fn bind_error_kind(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => "address-in-use",
        std::io::ErrorKind::PermissionDenied => "permission-denied",
        std::io::ErrorKind::AddrNotAvailable => "address-not-available",
        _ => "bind-error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_apply_line_is_ascii_and_stable() {
        assert_eq!(
            desktop_apply_line("3f2a1c9d", true, false),
            "solidway: desktop-apply id=3f2a1c9d autostart=1 close_to_tray=0"
        );
        assert!(desktop_apply_line("3f2a1c9d", false, true).is_ascii());
    }
}
