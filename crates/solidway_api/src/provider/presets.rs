use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use solidway_core::ApiFormat;

/// 路径模板里 `{action}` 的两个取值。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathActions {
    pub unary: String,
    pub stream: String,
}

/// 端点模板中的变量声明。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseUrlVariable {
    /// 变量名，必须与模板中 `{name}` / `${name}` 的原文一致。
    pub name: String,
    /// 展示用 i18n key（缺省回退到 `name`）。
    #[serde(default)]
    pub label: Option<String>,
    /// 取值种类：host-label / path-segment / host-only / full-url / gcp-location。
    pub kind: String,
    #[serde(default)]
    pub example: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// provider 目录条目。数据由 `scripts/gen-providers.mjs` 构建期生成。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPreset {
    pub key: String,
    pub name: String,
    /// 中文显示名；`name` 仍是规范名与英文回退值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_zh: Option<String>,
    /// 端点模板（含 0..n 个变量）；写入时展开为 `base_url`。
    pub base_url_template: String,
    /// 鉴权方式：bearer / x-api-key / api-key / google_service_account / sigv4 / none。
    pub auth_scheme: String,
    /// 是否常用（前端优先展示）。
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub doc: Option<String>,
    /// 预设类型：api / coding_plan / token_plan / subscription / local / enterprise / gateway。
    #[serde(default = "default_kind")]
    pub kind: String,
    /// 一句话说明（i18n key）；缺省时前端回退到 kind 通用提示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// 特殊注意事项（i18n key）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// 预设显式声明的默认可服务入站格式（目录为每条预设声明）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_api_formats: Option<Vec<String>>,
    /// 模板变量声明。
    #[serde(default)]
    pub base_url_variables: Vec<BaseUrlVariable>,
    /// 上游路径模板：可含 `{param}` / `{model}` / `{action}` / `{endpoint}`，用户不可编辑。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_template: Option<String>,
    /// 路径模板中需要用户填写的参数声明（不含 model / action / endpoint）。
    #[serde(default)]
    pub path_variables: Vec<BaseUrlVariable>,
    /// `{action}` 的取值（Anthropic 族）：`unary` / `stream`；缺省按 rawPredict 系列。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_actions: Option<PathActions>,
    /// SigV4 的服务名（缺省 `bedrock`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws_service: Option<String>,
    /// Anthropic 系端点的 `anthropic_version` 缺省值（Vertex / Bedrock 不同）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_version: Option<String>,
    /// 同一上游族的展示分组。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// 族内变体标签（i18n key）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_label: Option<String>,
}

#[derive(Deserialize)]
struct Catalogue {
    #[allow(dead_code)]
    source: Option<String>,
    #[allow(dead_code)]
    generated_at: Option<String>,
    /// 允许使用 Service Account 的端点模式（模板形式，如 `{prefix}.googleapis.com`）。
    #[serde(default)]
    service_account_host_patterns: Vec<String>,
    providers: Vec<ProviderPreset>,
}

fn default_kind() -> String {
    "api".to_string()
}

/// 解析后的目录：条目与 Google 端点模式。
struct CatalogueData {
    providers: Vec<ProviderPreset>,
    service_account_host_patterns: Vec<String>,
}

static CATALOGUE: OnceLock<CatalogueData> = OnceLock::new();

fn catalogue() -> &'static CatalogueData {
    CATALOGUE.get_or_init(|| match serde_json::from_str::<Catalogue>(include_str!("providers.json"))
    {
        Ok(catalogue) => {
            // fail-closed：无法映射到三种入站格式的条目不出现在运行时目录。
            let providers = catalogue
                .providers
                .into_iter()
                .filter(|preset| !default_api_formats(preset).is_empty())
                .collect();
            CatalogueData {
                providers,
                service_account_host_patterns: catalogue.service_account_host_patterns,
            }
        }
        Err(error) => {
            tracing::error!(%error, "failed to parse provider catalogue; falling back to empty");
            CatalogueData {
                providers: Vec::new(),
                service_account_host_patterns: Vec::new(),
            }
        }
    })
}

/// 完整内置目录（featured 在前）。
pub fn all() -> &'static [ProviderPreset] {
    &catalogue().providers
}

pub fn find(key: &str) -> Option<&'static ProviderPreset> {
    catalogue().providers.iter().find(|preset| preset.key == key)
}

/// 反查条目所属预设：先精确匹配；否则剥离**一次** `-custom` 后缀再匹配。
///
/// 不递归：`a-custom-custom` 剥离后为 `a-custom`，仍不命中即视为无关联。
pub fn preset_of(provider: &str) -> Option<&'static ProviderPreset> {
    let key = provider.trim();
    if let Some(preset) = find(key) {
        return Some(preset);
    }
    key.strip_suffix("-custom").and_then(find)
}

/// 是否为预设条目（字段锁定）。
pub fn is_locked(provider: &str) -> bool {
    find(provider.trim()).is_some()
}

/// 端点是否命中允许 Service Account 的 Google 端点模式。
pub fn is_google_endpoint(url: &str) -> bool {
    let host = crate::provider::template::host_of(url);
    catalogue()
        .service_account_host_patterns
        .iter()
        .any(|pattern| crate::provider::template::match_template(pattern, host).is_some())
}

/// 预设默认可无转换服务的入站 API 格式；缺省为空表示不兼容，由 `all()` 过滤。
pub fn default_api_formats(preset: &ProviderPreset) -> Vec<ApiFormat> {
    match &preset.default_api_formats {
        Some(formats) => formats
            .iter()
            .filter_map(|raw| ApiFormat::parse(raw))
            .collect(),
        None => Vec::new(),
    }
}

/// `GET /api/providers` 的响应条目：预设 + 默认入站格式 + 端点的可编辑性事实。
#[derive(Debug, Clone, Serialize)]
pub struct ProviderPresetView {
    pub key: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_zh: Option<String>,
    pub base_url_template: String,
    pub base_url_variables: Vec<BaseUrlVariable>,
    pub auth_scheme: String,
    pub featured: bool,
    pub doc: Option<String>,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub api_formats: Vec<ApiFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_template: Option<String>,
    #[serde(default)]
    pub path_variables: Vec<BaseUrlVariable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_actions: Option<PathActions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws_service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_label: Option<String>,
    /// 预设条目的端点与格式由目录决定，控制台据此禁用编辑。
    pub locked_fields: Vec<&'static str>,
}

impl From<&ProviderPreset> for ProviderPresetView {
    fn from(preset: &ProviderPreset) -> Self {
        Self {
            key: preset.key.clone(),
            name: preset.name.clone(),
            name_zh: preset.name_zh.clone(),
            base_url_template: preset.base_url_template.clone(),
            base_url_variables: preset.base_url_variables.clone(),
            auth_scheme: preset.auth_scheme.clone(),
            featured: preset.featured,
            doc: preset.doc.clone(),
            kind: preset.kind.clone(),
            summary: preset.summary.clone(),
            note: preset.note.clone(),
            api_formats: default_api_formats(preset),
            path_template: preset.path_template.clone(),
            path_variables: preset.path_variables.clone(),
            path_actions: preset.path_actions.clone(),
            aws_service: preset.aws_service.clone(),
            anthropic_version: preset.anthropic_version.clone(),
            group: preset.group.clone(),
            variant_label: preset.variant_label.clone(),
            locked_fields: vec!["base_url", "api_formats", "auth_scheme", "provider"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模板占位符必须可解析：路径模板只允许声明过的路径参数 + 请求级 model/action/endpoint；
    /// URL 模板只允许声明过的 URL 变量。
    #[test]
    fn catalogue_templates_only_use_declared_placeholders() {
        let reserved = ["model", "action", "endpoint"];
        for preset in all() {
            let url_names: Vec<String> = preset
                .base_url_variables
                .iter()
                .map(|var| var.name.clone())
                .collect();
            for name in crate::provider::template::placeholders(&preset.base_url_template) {
                assert!(
                    url_names.contains(&name),
                    "{}: base_url 模板占位符 {name} 未声明",
                    preset.key
                );
            }
            let Some(path_template) = &preset.path_template else {
                continue;
            };
            let path_names: Vec<String> = preset
                .path_variables
                .iter()
                .map(|var| var.name.clone())
                .collect();
            for name in crate::provider::template::placeholders(path_template) {
                assert!(
                    path_names.contains(&name) || reserved.contains(&name.as_str()),
                    "{}: 路径模板占位符 {name} 既未声明也不是请求级占位符",
                    preset.key
                );
            }
        }
    }

    #[test]
    fn catalogue_entries_are_compatible_and_have_metadata() {
        let allowed_kinds = [
            "api",
            "coding_plan",
            "token_plan",
            "subscription",
            "local",
            "enterprise",
            "gateway",
        ];
        for preset in all() {
            assert!(
                !default_api_formats(preset).is_empty(),
                "{}: api_formats 不能为空",
                preset.key
            );
            assert!(
                allowed_kinds.contains(&preset.kind.as_str()),
                "{}: 未知 kind {}",
                preset.key,
                preset.kind
            );
            assert!(
                preset.name_zh.as_deref().is_some_and(|name| !name.trim().is_empty()),
                "{}: name_zh 不能为空",
                preset.key
            );
            if let Some(summary) = &preset.summary {
                assert!(
                    summary.starts_with("providers."),
                    "{}: summary 必须是 i18n key",
                    preset.key
                );
            }
            if let Some(note) = &preset.note {
                assert!(
                    note.starts_with("providers."),
                    "{}: note 必须是 i18n key",
                    preset.key
                );
            }
        }
        for key in ["google", "google-vertex", "amazon-bedrock", "gitlab"] {
            assert!(find(key).is_none(), "不兼容预设 {key} 不应出现");
        }
        let minimax = find("minimax").expect("minimax");
        assert_eq!(
            default_api_formats(minimax),
            vec![ApiFormat::AnthropicMessages]
        );
    }

    #[test]
    fn vertex_presets_are_split_and_declare_paths() {
        // Vertex 的判定依据是"有路径模板 + Service Account + Anthropic 格式"。
        let vertex: Vec<&ProviderPreset> = all()
            .iter()
            .filter(|preset| preset.key.starts_with("google-vertex"))
            .collect();
        assert_eq!(vertex.len(), 2);
        for preset in vertex {
            assert!(preset.path_template.is_some(), "{} 缺少路径模板", preset.key);
            assert!(preset.auth_scheme == "google_service_account");
        }
    }
}
