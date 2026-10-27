//! 入站端点与 API 家族。
//!
//! 网关不做格式转换：入站 path 决定家族，家族决定家族根（上游地址拼接）、`model` 来源、
//! 自造错误体外壳、探针端点与会话字段。

use axum::http::Method;

use crate::config::ApiFormat;

/// 入站 API 家族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `/v1/chat/completions`、`/v1/responses`。
    OpenAi,
    /// `/v1/messages`、`/v1/messages/count_tokens`。
    Anthropic,
}

impl Family {
    /// 由 path 是否命中**已建模端点**判断家族；未建模路径回退到 OpenAI 外壳。
    ///
    /// 自造错误体在鉴权阶段就要成形（鉴权先于端点分类），因此这里必须是纯 path 判断。
    pub fn from_path(path: &str) -> Self {
        match Endpoint::from_path(path) {
            Some(Endpoint::Forward(inbound)) => inbound.family(),
            _ => Family::OpenAi,
        }
    }

    /// 家族根：拼接上游地址时从入站 path 剥离。
    pub fn root(self) -> &'static str {
        "/v1"
    }

    /// `base_url` 缺版本段时补的版本段；OpenAI 兼容族不补（`base_url` 是完整 API 根）。
    fn missing_version_segment(self) -> Option<&'static str> {
        match self {
            Family::OpenAi => None,
            Family::Anthropic => Some("/v1"),
        }
    }
}

/// 取 URL 的 path 部分（无 host / 无 `/` 时返回空串）。
fn url_path(url: &str) -> &str {
    let after_scheme = match url.find("://") {
        Some(idx) => &url[idx + 3..],
        None => url,
    };
    match after_scheme.find('/') {
        Some(idx) => &after_scheme[idx..],
        None => "",
    }
}

/// `base_url` path 中是否存在名为 `v1` / `v1beta` / `v1alpha` 的 path 段。
///
/// 段名精确比对：`v1beta1`、`v1alpha2` 等相邻形态不算命中。
fn has_version_segment(base_url: &str) -> bool {
    url_path(base_url)
        .split('/')
        .any(|segment| matches!(segment, "v1" | "v1beta" | "v1alpha"))
}

/// 客户端的入站端点（家族内已建模的具体端点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inbound {
    ChatCompletions,
    Responses,
    Messages,
    MessagesCountTokens,
}

impl Inbound {
    pub fn family(self) -> Family {
        match self {
            Inbound::ChatCompletions | Inbound::Responses => Family::OpenAi,
            Inbound::Messages | Inbound::MessagesCountTokens => Family::Anthropic,
        }
    }

    pub fn api_format(self) -> ApiFormat {
        match self {
            Inbound::ChatCompletions => ApiFormat::ChatCompletions,
            Inbound::Responses => ApiFormat::Responses,
            Inbound::Messages | Inbound::MessagesCountTokens => ApiFormat::AnthropicMessages,
        }
    }

    /// 是否为生成端点。非生成端点（`count_tokens`）计入请求数，
    /// 但不计 token、也不计入「用量缺失」。
    pub fn is_generation(self) -> bool {
        !matches!(self, Inbound::MessagesCountTokens)
    }
}

/// 入站 path 分类：转发端点 / 模型清单端点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    Forward(Inbound),
    Models,
}

impl Endpoint {
    /// 精确匹配；Gemini 路径已移除，`/v1beta/*` 与 `/upload/*` 均不再建模。
    pub fn from_path(path: &str) -> Option<Self> {
        match path {
            "/v1/models" => Some(Endpoint::Models),
            "/v1/chat/completions" => Some(Endpoint::Forward(Inbound::ChatCompletions)),
            "/v1/responses" => Some(Endpoint::Forward(Inbound::Responses)),
            "/v1/messages" => Some(Endpoint::Forward(Inbound::Messages)),
            "/v1/messages/count_tokens" => Some(Endpoint::Forward(Inbound::MessagesCountTokens)),
            _ => None,
        }
    }

    /// 该端点是否接受给定方法。
    pub fn allows_method(&self, method: &Method) -> bool {
        match self {
            Endpoint::Forward(_) => method == Method::POST,
            Endpoint::Models => method == Method::GET,
        }
    }

    /// 405 响应使用的 `Allow` 头。
    pub fn allow_header(&self) -> &'static str {
        match self {
            Endpoint::Forward(_) => "POST",
            Endpoint::Models => "GET",
        }
    }
}

/// 按家族补齐上游 `base_url` 的版本段（去尾斜杠；已含版本段则原样）。
///
/// `anthropic_messages` 族在 path 不含版本段时补 `/v1`；OpenAI 兼容族不补（任意兼容前缀语义）。
/// 只作用于请求构造，不回写存储值。转发目标构造与 discover 地址构造共用。
pub fn normalize_base_url(base_url: &str, family: Family) -> String {
    let base = base_url.trim_end_matches('/');
    match family.missing_version_segment() {
        Some(version) if !has_version_segment(base) => format!("{base}{version}"),
        _ => base.to_string(),
    }
}

/// 拼接上游地址：`base_url`（经版本段归一化）+ 入站 path 去掉家族根 + 清理后的 query。
///
/// 家族根只剥离一次；归一化见 [`normalize_base_url`]。
pub fn upstream_url(base_url: &str, path: &str, query: Option<&str>, family: Family) -> String {
    let base = normalize_base_url(base_url, family);
    let suffix = path.strip_prefix(family.root()).unwrap_or(path);
    let mut url = format!("{base}{suffix}");
    if let Some(query) = query
        && !query.is_empty()
    {
        url.push('?');
        url.push_str(query);
    }
    url
}

/// 剔除凭据类 query 参数（`key` / `api_key`），其余参数保持原始顺序与字面量。
///
/// 即使 Gemini 端点已移除，客户端仍可能把网关 Key 放进 query；原样转发会泄漏给上游。
pub fn clean_query(query: &str) -> Option<String> {
    let mut kept: Vec<&str> = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let name = pair.split('=').next().unwrap_or(pair);
        if name.eq_ignore_ascii_case("key") || name.eq_ignore_ascii_case("api_key") {
            continue;
        }
        kept.push(pair);
    }
    (!kept.is_empty()).then(|| kept.join("&"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_is_decided_by_path_literal() {
        assert_eq!(Family::from_path("/v1/chat/completions"), Family::OpenAi);
        assert_eq!(Family::from_path("/v1/files"), Family::OpenAi);
        assert_eq!(Family::from_path("/v1/messages"), Family::Anthropic);
        assert_eq!(
            Family::from_path("/v1/messages/count_tokens"),
            Family::Anthropic
        );
        // Gemini 路径不再建模，回退 OpenAI 外壳。
        assert_eq!(
            Family::from_path("/v1beta/models/gemini-2.5-pro:generateContent"),
            Family::OpenAi
        );
        assert_eq!(Family::from_path("/v1beta/cachedContents"), Family::OpenAi);
        assert_eq!(Family::from_path("/upload/v1beta/files"), Family::OpenAi);
    }

    #[test]
    fn endpoints_are_exact() {
        assert_eq!(
            Endpoint::from_path("/v1/chat/completions"),
            Some(Endpoint::Forward(Inbound::ChatCompletions))
        );
        assert_eq!(
            Endpoint::from_path("/v1/responses"),
            Some(Endpoint::Forward(Inbound::Responses))
        );
        assert_eq!(
            Endpoint::from_path("/v1/messages"),
            Some(Endpoint::Forward(Inbound::Messages))
        );
        assert_eq!(
            Endpoint::from_path("/v1/messages/count_tokens"),
            Some(Endpoint::Forward(Inbound::MessagesCountTokens))
        );
        assert_eq!(Endpoint::from_path("/v1/models"), Some(Endpoint::Models));
        assert_eq!(Endpoint::from_path("/v1/models/x"), None);
        assert_eq!(Endpoint::from_path("/pools/main/v1/chat/completions"), None);
        assert_eq!(Endpoint::from_path("/v1/files"), None);
        assert_eq!(Endpoint::from_path("/v1/messages/other"), None);
    }

    #[test]
    fn gemini_paths_are_not_modelled() {
        assert_eq!(
            Endpoint::from_path("/v1beta/models/gemini-2.5-pro:generateContent"),
            None
        );
        assert_eq!(
            Endpoint::from_path("/v1beta/models/gemini-2.5-pro:streamGenerateContent"),
            None
        );
        assert_eq!(
            Endpoint::from_path("/v1beta/models/gemini-2.5-pro:countTokens"),
            None
        );
        assert_eq!(Endpoint::from_path("/v1beta/models"), None);
        assert_eq!(Endpoint::from_path("/v1beta/cachedContents"), None);
        assert_eq!(Endpoint::from_path("/v1beta/cachedContents/c1"), None);
        assert_eq!(Endpoint::from_path("/v1beta/files"), None);
        assert_eq!(Endpoint::from_path("/v1beta/files/f1"), None);
        assert_eq!(Endpoint::from_path("/upload/v1beta/files"), None);
    }

    #[test]
    fn methods_are_method_specific() {
        let chat = Endpoint::from_path("/v1/chat/completions").expect("chat");
        assert!(chat.allows_method(&Method::POST));
        assert!(!chat.allows_method(&Method::GET));
        let models = Endpoint::from_path("/v1/models").expect("models");
        assert!(models.allows_method(&Method::GET));
        assert!(!models.allows_method(&Method::POST));
    }

    #[test]
    fn upstream_url_strips_family_root_once() {
        assert_eq!(
            upstream_url(
                "https://api.openai.com/v1",
                "/v1/chat/completions",
                None,
                Family::OpenAi
            ),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            upstream_url(
                "https://x.example.com/compatible-mode/v1/",
                "/v1/responses",
                Some("a=1"),
                Family::OpenAi
            ),
            "https://x.example.com/compatible-mode/v1/responses?a=1"
        );
        assert_eq!(
            upstream_url(
                "https://api.anthropic.com/v1",
                "/v1/messages",
                None,
                Family::Anthropic
            ),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            upstream_url(
                "https://proxy.example.com/anthropic/v1",
                "/v1/messages/count_tokens",
                None,
                Family::Anthropic
            ),
            "https://proxy.example.com/anthropic/v1/messages/count_tokens"
        );
    }

    #[test]
    fn upstream_url_keeps_openai_compatible_prefix() {
        assert_eq!(
            upstream_url(
                "https://open.bigmodel.cn/api/paas/v4",
                "/v1/chat/completions",
                None,
                Family::OpenAi
            ),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn upstream_url_fills_anthropic_version_segment() {
        // 厂商公布的 anthropic base 惯例不含版本段（百炼 / 智谱 / DeepSeek / Kimi）。
        assert_eq!(
            upstream_url(
                "https://api.example.com/apps/anthropic",
                "/v1/messages",
                None,
                Family::Anthropic
            ),
            "https://api.example.com/apps/anthropic/v1/messages"
        );
        // count_tokens 同样补齐，query 保留。
        assert_eq!(
            upstream_url(
                "https://api.deepseek.com/anthropic/",
                "/v1/messages/count_tokens",
                Some("beta=true"),
                Family::Anthropic
            ),
            "https://api.deepseek.com/anthropic/v1/messages/count_tokens?beta=true"
        );
    }

    #[test]
    fn upstream_url_keeps_anthropic_version_segment_when_present() {
        assert_eq!(
            upstream_url(
                "https://api.anthropic.com/v1",
                "/v1/messages",
                None,
                Family::Anthropic
            ),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            upstream_url(
                "https://proxy.example.com/anthropic/v1/",
                "/v1/messages",
                None,
                Family::Anthropic
            ),
            "https://proxy.example.com/anthropic/v1/messages"
        );
        // 前缀含版本段的代理：宁可不补，避免 `/v1/v1`。
        assert_eq!(
            upstream_url(
                "https://proxy.example.com/v1/anthropic",
                "/v1/messages",
                None,
                Family::Anthropic
            ),
            "https://proxy.example.com/v1/anthropic/messages"
        );
    }

    #[test]
    fn upstream_url_never_fills_openai_version_segment() {
        assert_eq!(
            upstream_url(
                "https://api.deepseek.com",
                "/v1/chat/completions",
                None,
                Family::OpenAi
            ),
            "https://api.deepseek.com/chat/completions"
        );
        assert_eq!(
            upstream_url(
                "https://api.deepinfra.com/v1/openai",
                "/v1/chat/completions",
                None,
                Family::OpenAi
            ),
            "https://api.deepinfra.com/v1/openai/chat/completions"
        );
        assert_eq!(
            upstream_url(
                "https://open.bigmodel.cn/api/paas/v4",
                "/v1/responses",
                None,
                Family::OpenAi
            ),
            "https://open.bigmodel.cn/api/paas/v4/responses"
        );
    }

    #[test]
    fn normalize_base_url_applies_version_segment_per_family() {
        assert_eq!(
            normalize_base_url("https://api.example.com/apps/anthropic", Family::Anthropic),
            "https://api.example.com/apps/anthropic/v1"
        );
        // 已含版本段（含 v1alpha）原样；OpenAI 族不补。
        assert_eq!(
            normalize_base_url("https://x.example.com/v1alpha", Family::Anthropic),
            "https://x.example.com/v1alpha"
        );
        assert_eq!(
            normalize_base_url("https://api.deepseek.com/", Family::OpenAi),
            "https://api.deepseek.com"
        );
    }

    #[test]
    fn version_segment_matches_exact_path_segment() {
        assert!(has_version_segment("https://x.example.com/v1"));
        assert!(has_version_segment("https://x.example.com/a/v1beta/b"));
        assert!(has_version_segment("https://x.example.com/v1alpha"));
        assert!(!has_version_segment("https://x.example.com"));
        assert!(!has_version_segment("https://x.example.com/"));
        // 相邻版本形态不算命中：再补一次版本段属已记录边缘取舍（ADR 0008）。
        assert!(!has_version_segment("https://x.example.com/v1beta1"));
        assert!(!has_version_segment("https://x.example.com/v1alpha2"));
        assert!(!has_version_segment("https://x.example.com/v10"));
        // 版本段只认 path，不认 host / query 字面量。
        assert!(!has_version_segment("https://v1.example.com/anthropic"));
    }

    #[test]
    fn query_drops_credentials_only() {
        assert_eq!(
            clean_query("alt=sse&key=sw-abc&api_key=k2").as_deref(),
            Some("alt=sse")
        );
        assert_eq!(clean_query("key=sw-abc").as_deref(), None);
        assert_eq!(
            clean_query("KEY=sw-abc&alt=sse").as_deref(),
            Some("alt=sse")
        );
        assert_eq!(
            clean_query("alt=sse&monkey=1").as_deref(),
            Some("alt=sse&monkey=1")
        );
        assert_eq!(clean_query("").as_deref(), None);
    }

    #[test]
    fn non_generation_endpoints() {
        assert!(!Inbound::MessagesCountTokens.is_generation());
        assert!(Inbound::Messages.is_generation());
        assert!(Inbound::ChatCompletions.is_generation());
        assert!(Inbound::Responses.is_generation());
    }
}
