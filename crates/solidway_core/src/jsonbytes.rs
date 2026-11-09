//! 请求体的定点字节改写工具（不引入 JSON DOM，保持其余字节原样）。
//!
//! 与 `engine` 里的模型别名改写共用同一套扫描原语；这里补充兼容适配档需要的
//! 「按 key 定位完整成员」与「按 key 改写字符串值」两类操作。

use crate::engine::{find_string_end, skip_ws, top_level_value_span};

/// 顶层 `key` 成员（含 key、冒号与完整值）的区间 `[start, end)`；`start` 指向 key 的左引号。
pub(crate) fn top_level_member_span(raw: &[u8], key: &str) -> Option<(usize, usize)> {
    let mut index = 0;
    let mut depth = 0usize;
    let mut expecting_key = false;

    while index < raw.len() {
        match raw[index] {
            b'"' => {
                let key_quote = index;
                let string_end = find_string_end(raw, key_quote)?;
                if depth == 1 && expecting_key {
                    let after_key = skip_ws(raw, string_end + 1);
                    if raw.get(after_key) == Some(&b':') {
                        if &raw[key_quote + 1..string_end] == key.as_bytes() {
                            let (_, value_end) = top_level_value_span(raw, key)?;
                            return Some((key_quote, value_end));
                        }
                        expecting_key = false;
                    }
                }
                index = string_end + 1;
            }
            b'{' => {
                depth += 1;
                expecting_key = depth == 1;
                index += 1;
            }
            b'[' => {
                depth += 1;
                expecting_key = false;
                index += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                expecting_key = false;
                index += 1;
            }
            b',' => {
                expecting_key = depth == 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

/// 顶层是否声明了 `key`。
pub(crate) fn has_top_level_member(raw: &[u8], key: &str) -> bool {
    top_level_value_span(raw, key).is_some()
}

/// 删除顶层成员（含分隔逗号），其余字节保持原样；没有可删成员时返回 `None`。
pub(crate) fn remove_top_level_members(raw: &[u8], keys: &[&str]) -> Option<Vec<u8>> {
    let mut out = raw.to_vec();
    let mut changed = false;
    for key in keys {
        while let Some(span) = top_level_member_span(&out, key) {
            out = splice_member_out(&out, span);
            changed = true;
        }
    }
    changed.then_some(out)
}

/// 去掉一个成员区间，同时处理它前/后的分隔逗号，避免留下 `{,}` 或悬空逗号。
fn splice_member_out(raw: &[u8], (start, end): (usize, usize)) -> Vec<u8> {
    let mut start = start;
    let mut end = end;
    // 优先吃掉成员前面的逗号；成员是第一个时改吃后面的。
    let mut before = start;
    while before > 0 && raw[before - 1].is_ascii_whitespace() {
        before -= 1;
    }
    if before > 0 && raw[before - 1] == b',' {
        start = before - 1;
        while start > 0 && raw[start - 1].is_ascii_whitespace() {
            start -= 1;
        }
    } else {
        let mut after = end;
        while after < raw.len() && raw[after].is_ascii_whitespace() {
            after += 1;
        }
        if after < raw.len() && raw[after] == b',' {
            end = after + 1;
        }
    }
    let mut out = Vec::with_capacity(raw.len().saturating_sub(end.saturating_sub(start)));
    out.extend_from_slice(&raw[..start]);
    out.extend_from_slice(&raw[end..]);
    out
}

/// 把所有「对象成员的 key 为 `key`、值为字符串 `from`」的值改写成 `to`，其余字节原样。
///
/// 扫描按字符串边界跳过：正文（例如消息内容里的 `"role"` 字样）不会被误改。
pub(crate) fn rewrite_string_values(
    raw: &[u8],
    key: &str,
    from: &str,
    to: &str,
) -> Option<Vec<u8>> {
    let mut out: Option<Vec<u8>> = None;
    let mut copied = 0usize;
    let mut pending_key: Option<(usize, usize)> = None;
    let mut index = 0;

    while index < raw.len() {
        if raw[index] != b'"' {
            index += 1;
            continue;
        }
        let string_end = find_string_end(raw, index)?;
        let after = skip_ws(raw, string_end + 1);
        if raw.get(after) == Some(&b':') {
            pending_key = Some((index + 1, string_end));
        } else {
            let is_target = pending_key.is_some_and(|(start, end)| {
                &raw[start..end] == key.as_bytes() && &raw[index + 1..string_end] == from.as_bytes()
            });
            if is_target {
                let buffer = out.get_or_insert_with(|| Vec::with_capacity(raw.len()));
                buffer.extend_from_slice(&raw[copied..index + 1]);
                buffer.extend_from_slice(to.as_bytes());
                copied = string_end;
            }
            pending_key = None;
        }
        index = string_end + 1;
    }

    out.map(|mut buffer| {
        buffer.extend_from_slice(&raw[copied..]);
        buffer
    })
}

/// 把 `messages[*].role` 中值为 `from` 的角色改为 `to`。
pub(crate) fn rewrite_message_roles(raw: &[u8], from: &str, to: &str) -> Option<Vec<u8>> {
    rewrite_string_values(raw, "role", from, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_top_level_member_with_surrounding_comma() {
        let raw = br#"{"model":"m","store":false,"messages":[]}"#;
        let out = remove_top_level_members(raw, &["store"]).expect("rewrite");
        assert_eq!(out, br#"{"model":"m","messages":[]}"#);
    }

    #[test]
    fn removes_first_and_last_members() {
        assert_eq!(
            remove_top_level_members(br#"{"a":1,"b":2}"#, &["a"]).unwrap(),
            br#"{"b":2}"#
        );
        assert_eq!(
            remove_top_level_members(br#"{"a":1,"b":2}"#, &["b"]).unwrap(),
            br#"{"a":1}"#
        );
        assert_eq!(
            remove_top_level_members(br#"{"a":1}"#, &["a"]).unwrap(),
            b"{}"
        );
    }

    #[test]
    fn removing_missing_member_is_noop() {
        assert!(remove_top_level_members(br#"{"a":1}"#, &["store"]).is_none());
    }

    #[test]
    fn removal_handles_nested_objects() {
        let raw = br#"{"thinking":{"type":"enabled","budget":1},"model":"m"}"#;
        let out = remove_top_level_members(raw, &["thinking"]).unwrap();
        assert_eq!(out, br#"{"model":"m"}"#);
    }

    #[test]
    fn rewrites_only_role_values() {
        let raw = br#"{"messages":[{"role":"developer","content":"developer"},{"role":"user","content":"hi"}]}"#;
        let out = rewrite_message_roles(raw, "developer", "system").unwrap();
        assert_eq!(
            out,
            br#"{"messages":[{"role":"system","content":"developer"},{"role":"user","content":"hi"}]}"#
        );
    }

    #[test]
    fn role_rewrite_skips_escaped_content() {
        let raw = br#"{"messages":[{"role":"user","content":"he said \"role\":\"developer\""}]}"#;
        assert!(rewrite_message_roles(raw, "developer", "system").is_none());
    }

    #[test]
    fn role_rewrite_is_idempotent() {
        let raw = br#"{"messages":[{"role":"developer","content":"x"}]}"#;
        let once = rewrite_message_roles(raw, "developer", "system").unwrap();
        assert!(rewrite_message_roles(&once, "developer", "system").is_none());
    }
}
