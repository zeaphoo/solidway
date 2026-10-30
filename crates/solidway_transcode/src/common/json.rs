//! JSON Pointer（RFC 6901）机械工具；不含协议语义。
//!
//! 只提供「解析」与「分段反转义」。指针的**拼装**仍由各方向就地
//! `format!("{pointer}/field")` 完成——把字段名搬进 `common` 会违反
//! `common` 不得出现协议字段名的边界。

use serde_json::Value;

/// 反转义单个 pointer token：`~1 -> /`，`~0 -> ~`。
///
/// 按 RFC 6901 §4，必须先替换 `~1` 再替换 `~0`，否则 `~01` 会被错误还原成 `/`。
fn unescape_token(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

/// 解析本地指针 `#/a/b/0`，返回目标值的克隆；任何一环缺失返回 `None`。
///
/// 只接受以 `#/` 开头的本地引用；外部 URL 与纯 `#` 视为不可解析。
pub(crate) fn resolve_local(root: &Value, reference: &str) -> Option<Value> {
    let pointer = reference.strip_prefix("#/")?;
    let mut current = root;
    for token in pointer.split('/') {
        let token = unescape_token(token);
        current = match current {
            Value::Object(object) => object.get(&token)?,
            Value::Array(items) => items.get(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_objects_and_array_indexes() {
        let root = json!({"a": {"b": [10, {"c": "hit"}]}});
        assert_eq!(resolve_local(&root, "#/a/b/1/c"), Some(json!("hit")));
        assert_eq!(resolve_local(&root, "#/a/b/0"), Some(json!(10)));
    }

    #[test]
    fn resolves_escaped_tokens() {
        let root = json!({"a/b": {"c~d": 1}});
        assert_eq!(resolve_local(&root, "#/a~1b/c~0d"), Some(json!(1)));
    }

    #[test]
    fn escapes_in_rfc_order() {
        // `~01` 是 token "~1"，不是 "/"。
        let root = json!({"~1": "literal", "/": "slash"});
        assert_eq!(resolve_local(&root, "#/~01"), Some(json!("literal")));
        assert_eq!(resolve_local(&root, "#/~1"), Some(json!("slash")));
    }

    #[test]
    fn rejects_unresolvable_references() {
        let root = json!({"a": {"b": 1}});
        assert_eq!(resolve_local(&root, "#"), None, "空指针不解析");
        assert_eq!(resolve_local(&root, "a/b"), None, "必须带 #/ 前缀");
        assert_eq!(
            resolve_local(&root, "https://x/y#/a"),
            None,
            "不解析外部 URL"
        );
        assert_eq!(resolve_local(&root, "#/a/missing"), None);
        assert_eq!(resolve_local(&root, "#/a/b/c"), None, "标量不可下探");
        assert_eq!(resolve_local(&root, "#/a/b/-"), None, "数组只接受数字下标");
        assert_eq!(resolve_local(&root, "#/a/b/notanindex"), None);
    }
}
