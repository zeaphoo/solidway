//! 模型名分类与通用阈值；不含协议语义。

/// OpenAI `gpt-N` 中 N >= 5 视为 reasoning 模型（与 sub2api 判定一致）。
pub(crate) fn is_reasoning_model(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    let Some(rest) = model.strip_prefix("gpt-") else {
        return false;
    };
    let digits: String = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    let Ok(major) = digits.parse::<u32>() else {
        return false;
    };
    major >= 5
}

/// Chat / Responses 输出 token 的最小值。
pub(crate) const MIN_OUTPUT_TOKENS: u32 = 128;
