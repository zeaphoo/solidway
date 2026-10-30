//! 计费字段的机械算术：`Option<u64>` 饱和加减；不含协议语义。
//!
//! 缺失（`None`）与 0 在**运算**上等价（按 0 参与），但**输出**时是否保留
//! `Some(0)` 由调用方用 [`positive`] 决定，本模块不做隐式归一。

/// `total - parts...`：逐项饱和相减，`None` 计作 0，结果不为负。
///
/// 对应「输入 tokens 扣除缓存命中 / 缓存写入」这类派生字段：
/// `saturating_deduct(prompt_tokens, &[cached, cache_creation])`。
pub(crate) fn saturating_deduct(total: Option<u64>, parts: &[Option<u64>]) -> u64 {
    parts
        .iter()
        .flatten()
        .fold(total.unwrap_or(0), |acc, part| acc.saturating_sub(*part))
}

/// 逐项饱和相加：`None` 计作 0，结果溢出时饱和到 [`u64::MAX`]。
pub(crate) fn saturating_sum(parts: &[Option<u64>]) -> u64 {
    parts
        .iter()
        .flatten()
        .fold(0u64, |acc, part| acc.saturating_add(*part))
}

/// 0 视为缺失：`0 -> None`，其余 `n -> Some(n)`。
///
/// 用于「值为 0 时不下发该字段」的线上形状。
pub(crate) fn positive(value: u64) -> Option<u64> {
    (value > 0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduct_saturates_at_zero() {
        assert_eq!(saturating_deduct(Some(100), &[Some(30), Some(20)]), 50);
        assert_eq!(saturating_deduct(Some(10), &[Some(30)]), 0);
        assert_eq!(
            saturating_deduct(Some(10), &[Some(8), Some(8)]),
            0,
            "多步扣减不得下溢"
        );
    }

    #[test]
    fn deduct_treats_none_as_zero() {
        assert_eq!(saturating_deduct(None, &[]), 0);
        assert_eq!(saturating_deduct(None, &[None, Some(5)]), 0);
        assert_eq!(saturating_deduct(Some(42), &[None]), 42);
        assert_eq!(saturating_deduct(Some(42), &[None, Some(2)]), 40);
    }

    #[test]
    fn sum_is_saturating() {
        assert_eq!(saturating_sum(&[]), 0);
        assert_eq!(saturating_sum(&[Some(1), None, Some(2)]), 3);
        assert_eq!(saturating_sum(&[Some(u64::MAX), Some(1)]), u64::MAX);
    }

    #[test]
    fn positive_folds_zero_to_none() {
        assert_eq!(positive(0), None);
        assert_eq!(positive(1), Some(1));
        assert_eq!(positive(u64::MAX), Some(u64::MAX));
    }
}
