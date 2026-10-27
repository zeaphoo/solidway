//! 端点模板：占位符识别、展开（expand）、模式匹配（match）与变量取值校验。
//!
//! 模板同时支持 `{name}` 与 `${NAME}` 两种写法；变量名即花括号内原文。
//! 展开与匹配互为逆运算，被"写入校验 / 变量反推 / Google 端点判定"共用。

use std::collections::BTreeMap;

use crate::error::{ApiError, Result};

/// 从模板中按出现顺序取出占位符名字（同一名称只保留一次）。
pub fn placeholders(template: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let bytes = template.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' {
            if let Some(end) = template[index + 1..].find('}') {
                let name = &template[index + 1..index + 1 + end];
                if !name.is_empty() && !name.contains('{') {
                    if !names.iter().any(|existing| existing == name) {
                        names.push(name.to_string());
                    }
                    index = index + 1 + end + 1;
                    continue;
                }
            }
        }
        index += 1;
    }
    names
}

/// 用变量值渲染模板；缺失或空值报 `missing_base_url_var`。
pub fn expand(template: &str, vars: &BTreeMap<String, String>) -> Result<String> {
    let mut rendered = template.to_string();
    for name in placeholders(template) {
        let value = vars
            .get(&name)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ApiError::coded(
                    "missing_base_url_var",
                    format!("缺少端点变量 {name}"),
                )
            })?;
        // 先替换 `${NAME}`，避免随后替换 `{NAME}` 时留下 `$` 前缀。
        rendered = rendered.replace(&format!("${{{name}}}"), value);
        rendered = rendered.replace(&format!("{{{name}}}"), value);
    }
    Ok(rendered)
}

/// 变量取值的种类约束；未知种类一律拒绝，避免静默放行。
pub fn validate_value(kind: &str, value: &str) -> std::result::Result<(), &'static str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("不能为空");
    }
    match kind {
        "host-label" | "gcp-location" | "gcp-project" => {
            let ok = trimmed
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
                && !trimmed.starts_with('-')
                && !trimmed.ends_with('-');
            if ok {
                Ok(())
            } else {
                Err("只允许字母、数字与连字符")
            }
        }
        "path-segment" => {
            if trimmed.contains(['/', '?', '#', ' ']) {
                Err("不能包含 / ? # 或空格")
            } else {
                Ok(())
            }
        }
        "host-only" => {
            if trimmed.contains(['/', '?', '#', ' ']) {
                return Err("只填主机名，不能包含路径");
            }
            if trimmed.starts_with("://") || trimmed.contains("://") {
                return Err("只填主机名，不要带 scheme");
            }
            Ok(())
        }
        "azure-api-version" => {
            let ok = !trimmed.is_empty()
                && trimmed
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
            if ok {
                Ok(())
            } else {
                Err("只允许字母、数字与连字符（如 2024-10-21）")
            }
        }
        "full-url" => {
            let Some((scheme, _)) = trimmed.split_once("://") else {
                return Err("需为完整地址（含 http:// 或 https://）");
            };
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
                Ok(())
            } else {
                Err("仅支持 http / https")
            }
        }
        _ => Err("未知的变量类型"),
    }
}

/// 去掉 URL 末尾斜杠。
pub fn trim_trailing_slash(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// 取 URL 的 host（不含 port 之后的路径）。
pub fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    rest.split(['/', '?', '#']).next().unwrap_or_default()
}

/// 模板匹配：`pattern` 中的占位符匹配单段（不跨 `.` `/` `:`），整串锚定。
///
/// 命中返回变量取值表，供"反向绑定"等场景复用。
pub fn match_template(pattern: &str, value: &str) -> Option<BTreeMap<String, String>> {
    let pattern = pattern.to_ascii_lowercase();
    let value = value.to_ascii_lowercase();
    let mut vars = BTreeMap::new();
    if match_from(&pattern, &value, &mut vars) {
        Some(vars)
    } else {
        None
    }
}

fn is_segment_char(ch: char) -> bool {
    !matches!(ch, '.' | '/' | ':' | '?' | '#' | ' ')
}

fn match_from(pattern: &str, value: &str, vars: &mut BTreeMap<String, String>) -> bool {
    // 取出第一个占位符；没有则剩余部分必须完全相等。
    let Some(open) = pattern.find('{') else {
        return pattern == value;
    };
    let Some(close) = pattern[open..].find('}').map(|offset| open + offset) else {
        return pattern == value;
    };
    let literal = &pattern[..open];
    let name = &pattern[open + 1..close];
    let rest = &pattern[close + 1..];

    let Some(literal_rest) = value.strip_prefix(literal) else {
        return false;
    };

    // 变量段长度：优先让剩余字面量后缀匹配（贪心到最后一个可匹配位置）。
    for take in (0..=literal_rest.len()).rev() {
        let candidate = &literal_rest[..take];
        if !candidate.chars().all(is_segment_char) {
            continue;
        }
        let remainder = &literal_rest[take..];
        let mut nested = vars.clone();
        nested.insert(name.to_string(), candidate.to_string());
        if match_from(rest, remainder, &mut nested) {
            *vars = nested;
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn placeholders_support_both_styles() {
        assert_eq!(placeholders("https://{location}-aiplatform.googleapis.com"), ["location"]);
        assert_eq!(
            placeholders("https://api.x.com/a/${ACCOUNT_ID}/b"),
            ["ACCOUNT_ID"]
        );
        assert!(placeholders("https://api.openai.com/v1").is_empty());
    }

    #[test]
    fn expand_replaces_all_occurrences() {
        let rendered = expand(
            "https://{resource}.openai.azure.com",
            &vars(&[("resource", "myres")]),
        )
        .expect("expand");
        assert_eq!(rendered, "https://myres.openai.azure.com");
    }

    #[test]
    fn expand_rejects_missing_and_blank() {
        for input in [vars(&[]), vars(&[("resource", "   ")])] {
            let error = expand("https://{resource}.x.com", &input).unwrap_err();
            assert!(matches!(error, ApiError::Coded { code: "missing_base_url_var", .. }));
        }
    }

    #[test]
    fn validate_kinds() {
        assert!(validate_value("host-label", "my-resource").is_ok());
        assert!(validate_value("host-label", "my.resource").is_err());
        assert!(validate_value("path-segment", "abc123").is_ok());
        assert!(validate_value("path-segment", "a/b").is_err());
        assert!(validate_value("host-only", "dbc-1.cloud.databricks.com").is_ok());
        assert!(validate_value("host-only", "dbc-1.cloud.databricks.com/ai").is_err());
        assert!(validate_value("full-url", "https://gw.example.com").is_ok());
        assert!(validate_value("full-url", "gw.example.com").is_err());
        assert!(validate_value("gcp-location", "us-east5").is_ok());
        assert!(validate_value("gcp-project", "my-project").is_ok());
        assert!(validate_value("azure-api-version", "2024-10-21").is_ok());
        assert!(validate_value("azure-api-version", "2024/10/21").is_err());
    }

    #[test]
    fn match_host_pattern() {
        assert!(match_template("{prefix}.googleapis.com", "us-east5-aiplatform.googleapis.com").is_some());
        assert!(match_template("{prefix}.googleapis.com", "generativelanguage.googleapis.com").is_some());
        assert!(match_template("{prefix}.googleapis.com", "api.example.com").is_none());
        assert!(match_template("{prefix}.googleapis.com", "a.b.googleapis.com").is_none());
    }

    #[test]
    fn match_is_inverse_of_expand() {
        let pattern = "https://{location}-aiplatform.googleapis.com";
        let expanded = expand(pattern, &vars(&[("location", "europe-west1")])).unwrap();
        let parsed = match_template(pattern, &expanded).expect("match");
        assert_eq!(parsed.get("location").map(String::as_str), Some("europe-west1"));
    }

    #[test]
    fn host_of_strips_scheme_and_path() {
        assert_eq!(host_of("https://gw.example.com/v1"), "gw.example.com");
        assert_eq!(host_of("gw.example.com"), "gw.example.com");
    }
}
