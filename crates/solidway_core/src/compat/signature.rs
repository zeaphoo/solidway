//! 上游 400 错误的兼容签名识别。
//!
//! 适配档是预检；签名表是兜底：命中已知的「表示层不兼容」错误时，数据面按需
//! 应用最小规则并重试同一次 provider 请求一次，同时把结论记入
//! [`super::Learned`]，后续请求无需再踩一次。

use super::Rule;

/// 从错误响应体识别需要的改写规则；空表示「不是已知的表示层不兼容」。
///
/// 只认 400：429 / 5xx 属于限流或上游故障，交给健康分类处理。
pub fn diagnose(status: u16, body: &[u8]) -> Vec<Rule> {
    if status != 400 {
        return Vec::new();
    }
    // 只看前 4KB：兼容错误信息都很短，避免为错误路径解析大响应体。
    let limit = body.len().min(4096);
    let text = String::from_utf8_lossy(&body[..limit]).to_ascii_lowercase();
    let mut rules = Vec::new();

    if text.contains("developer is not one of")
        || text.contains("'developer' is not one of")
        || (text.contains("role") && text.contains("developer") && text.contains("not one of"))
    {
        rules.push(Rule::DeveloperToSystem);
    }
    if text.contains("system message must be at the beginning")
        || text.contains("system message must be at the start")
    {
        rules.push(Rule::DowngradeMidSystem);
    }
    if text.contains("'stream' and 'stream_options' must be set together") {
        rules.push(Rule::DropStreamOptions);
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_dashscope_role_error() {
        // 阿里 DashScope / Token Plan 的真实错误体。
        let body = br#"{"error":{"code":"invalid_parameter_error","param":null,"message":"developer is not one of ['system', 'assistant', 'user', 'tool', 'function']","type":"invalid_request_error"}}"#;
        assert_eq!(diagnose(400, body), vec![Rule::DeveloperToSystem]);
    }

    #[test]
    fn recognizes_mid_conversation_system_error() {
        let body = br#"{"error":{"message":"System message must be at the beginning."}}"#;
        assert_eq!(diagnose(400, body), vec![Rule::DowngradeMidSystem]);
    }

    #[test]
    fn recognizes_stream_options_pairing_error() {
        let body = br#"{"error":{"message":"'stream' and 'stream_options' must be set together as explicitly required."}}"#;
        assert_eq!(diagnose(400, body), vec![Rule::DropStreamOptions]);
    }

    #[test]
    fn ignores_non_400_and_unknown_bodies() {
        let body = br#"{"error":{"message":"developer is not one of ['system']"}}"#;
        assert!(diagnose(429, body).is_empty());
        assert!(diagnose(400, br#"{"error":{"message":"bad model"}}"#).is_empty());
        assert!(diagnose(400, b"").is_empty());
    }
}
