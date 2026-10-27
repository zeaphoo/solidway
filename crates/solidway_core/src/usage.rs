//! 上游 `usage` 归一化与 token 统计口径。
//!
//! 只做**读取**，不修改响应；解析不到时返回 `None`，由调用方记录为用量缺失。

use serde_json::Value;

/// 归一化后的 token 用量。
///
/// - `input_tokens`：总 prompt token，包含缓存命中与缓存写入部分；
/// - `output_tokens`：总 completion token，包含 reasoning 部分；
/// - `cached_tokens` / `cache_write_tokens` / `reasoning_tokens` 都是子集，
///   不参与 `input + output` 的求和。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
}

impl TokenUsage {
    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cached_tokens == 0
            && self.cache_write_tokens == 0
            && self.reasoning_tokens == 0
    }

    /// 合并多次出现的 usage（例如 Anthropic 的 `message_start` + `message_delta`）。
    ///
    /// 各字段取较大值：上游通常返回累计值，取 max 可避免重复累加，
    /// 同时保留分事件出现时已解析到的部分。
    pub fn merge(&mut self, other: TokenUsage) {
        self.input_tokens = self.input_tokens.max(other.input_tokens);
        self.output_tokens = self.output_tokens.max(other.output_tokens);
        self.cached_tokens = self.cached_tokens.max(other.cached_tokens);
        self.cache_write_tokens = self.cache_write_tokens.max(other.cache_write_tokens);
        self.reasoning_tokens = self.reasoning_tokens.max(other.reasoning_tokens);
    }
}

fn number(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}

fn detail_number(value: &Value, details_key: &str, field: &str) -> Option<u64> {
    number(
        value
            .get(details_key)
            .and_then(|details| details.get(field)),
    )
}

/// 从一段 JSON 中提取用量；支持 OpenAI / Responses / Anthropic 常见位置。
pub fn extract_usage(value: &Value) -> Option<TokenUsage> {
    let mut usage = TokenUsage::default();
    let mut found = false;

    // OpenAI Chat / Anthropic message_delta：顶层 usage。
    if let Some(raw) = value.get("usage") {
        found |= merge_object(raw, &mut usage);
    }
    // OpenAI Responses：response.usage。
    if let Some(raw) = value.get("response").and_then(|item| item.get("usage")) {
        found |= merge_object(raw, &mut usage);
    }
    // Anthropic message_start：message.usage。
    if let Some(raw) = value.get("message").and_then(|item| item.get("usage")) {
        found |= merge_object(raw, &mut usage);
    }
    (found && !usage.is_empty()).then_some(usage)
}

/// 从完整 JSON 响应体中提取用量。
pub fn extract_usage_from_json(value: &Value) -> Option<TokenUsage> {
    extract_usage(value)
}

/// 从 SSE 的 `data:` 负载中提取用量。非 JSON 或没有 usage 时返回 `None`。
pub fn extract_usage_from_sse_data(data: &str) -> Option<TokenUsage> {
    let value: Value = serde_json::from_str(data).ok()?;
    extract_usage(&value)
}

fn merge_object(raw: &Value, target: &mut TokenUsage) -> bool {
    let Some(object) = raw.as_object() else {
        return false;
    };

    let mut found = false;

    // 输入 token：优先 input_tokens（OpenAI Responses / Anthropic），否则 prompt_tokens（OpenAI Chat）。
    let input = number(object.get("input_tokens"))
        .or_else(|| number(object.get("prompt_tokens")));
    let cache_read = number(object.get("cache_read_input_tokens"))
        .or_else(|| detail_number(raw, "input_tokens_details", "cached_tokens"))
        .or_else(|| detail_number(raw, "prompt_tokens_details", "cached_tokens"));
    let cache_write = number(object.get("cache_creation_input_tokens"));

    if let Some(input) = input {
        // Anthropic 的 input_tokens 不含 cache read / creation；OpenAI 的 prompt_tokens
        // 已包含 cached，因此只在 Anthropic 的 input_tokens 字段存在时相加。
        let anthropic_cache = if object.get("input_tokens").is_some() {
            number(object.get("cache_read_input_tokens"))
                .unwrap_or(0)
                .saturating_add(cache_write.unwrap_or(0))
        } else {
            0
        };
        target.input_tokens = target
            .input_tokens
            .max(input.saturating_add(anthropic_cache));
        found = true;
    }
    if let Some(cached) = cache_read {
        target.cached_tokens = target.cached_tokens.max(cached);
        found = true;
    }
    if let Some(write) = cache_write {
        target.cache_write_tokens = target.cache_write_tokens.max(write);
        found = true;
    }

    // 输出 token：output_tokens / completion_tokens。
    let output = number(object.get("output_tokens"))
        .or_else(|| number(object.get("completion_tokens")));
    if let Some(output) = output {
        target.output_tokens = target.output_tokens.max(output);
        found = true;
    }

    let reasoning = detail_number(raw, "output_tokens_details", "reasoning_tokens")
        .or_else(|| detail_number(raw, "completion_tokens_details", "reasoning_tokens"));
    if let Some(reasoning) = reasoning {
        target.reasoning_tokens = target.reasoning_tokens.max(reasoning);
        found = true;
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openai_chat_usage_includes_cached_and_reasoning_subsets() {
        let value = json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 40,
                "prompt_tokens_details": { "cached_tokens": 30 },
                "completion_tokens_details": { "reasoning_tokens": 10 }
            }
        });
        let usage = extract_usage(&value).expect("usage");
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 40);
        assert_eq!(usage.cached_tokens, 30);
        assert_eq!(usage.reasoning_tokens, 10);
        assert_eq!(usage.cache_write_tokens, 0);
    }

    #[test]
    fn responses_usage_is_nested_under_response() {
        let value = json!({
            "type": "response.completed",
            "response": {
                "usage": {
                    "input_tokens": 80,
                    "output_tokens": 20,
                    "input_tokens_details": { "cached_tokens": 5 },
                    "output_tokens_details": { "reasoning_tokens": 3 }
                }
            }
        });
        let usage = extract_usage(&value).expect("usage");
        assert_eq!(usage.input_tokens, 80);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.cached_tokens, 5);
        assert_eq!(usage.reasoning_tokens, 3);
    }

    #[test]
    fn anthropic_input_and_output_events_merge() {
        let mut usage = extract_usage(&json!({
            "type": "message_start",
            "message": {
                "usage": {
                    "input_tokens": 100,
                    "cache_read_input_tokens": 20,
                    "cache_creation_input_tokens": 10
                }
            }
        }))
        .expect("input usage");
        assert_eq!(usage.input_tokens, 130);
        assert_eq!(usage.cached_tokens, 20);
        assert_eq!(usage.cache_write_tokens, 10);

        let output = extract_usage(&json!({
            "type": "message_delta",
            "usage": { "output_tokens": 50 }
        }))
        .expect("output usage");
        usage.merge(output);
        assert_eq!(usage.input_tokens, 130);
        assert_eq!(usage.output_tokens, 50);
    }

    #[test]
    fn prompt_tokens_with_cache_read_is_not_double_counted() {
        let value = json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "cache_read_input_tokens": 30
            }
        });
        let usage = extract_usage(&value).expect("usage");
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.cached_tokens, 30);
    }

    #[test]
    fn json_body_usage_falls_back_to_single_object() {
        let value = json!({
            "usage": { "prompt_tokens": 7, "completion_tokens": 3 }
        });
        let usage = extract_usage_from_json(&value).expect("usage");
        assert_eq!(usage.input_tokens, 7);
        assert_eq!(usage.output_tokens, 3);
    }

    #[test]
    fn missing_usage_returns_none() {
        assert!(extract_usage(&json!({ "id": "x" })).is_none());
    }
}
