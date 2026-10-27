//! 纯 egress 计划：把账号身份整理成上游私有协议需要的 header / metadata。
//!
//! 本 crate 不发起 HTTP、不获取 token、不读环境变量。OAuth token 的刷新与
//! 账号选择由调用方完成；这里只负责把已确定的身份编码成 wire 形状。

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidway_transcode::protocol::ProtocolId;

pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const CLAUDE_CODE_VERSION: &str = "2.1.258";
pub const CODEX_VERSION: &str = "0.146.0";
pub const CLAUDE_CODE_USER_AGENT: &str = "claude-cli/2.1.258 (external, cli)";

const CLAUDE_CODE_BETAS: &[&str] = &[
    "claude-code-20250219",
    "oauth-2025-04-20",
    "interleaved-thinking-2025-05-14",
    "fine-grained-tool-streaming-2025-05-14",
];
const CLAUDE_API_KEY_BETAS: &[&str] = &[
    "claude-code-20250219",
    "interleaved-thinking-2025-05-14",
    "fine-grained-tool-streaming-2025-05-14",
];
const CODEX_UA_SUFFIX: &str = " (Ubuntu 22.4.0; x86_64) xterm-256color";

/// 调用方已经解析好的账号 / 会话身份。
#[derive(Debug, Clone, Default)]
pub struct EgressIdentity {
    pub account_id: Option<String>,
    pub account_uuid: Option<String>,
    pub device_id: Option<String>,
    pub session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub oauth_access_token: Option<String>,
    pub api_key: Option<String>,
    pub cli_version: Option<String>,
    pub user_agent: Option<String>,
    pub betas: Vec<String>,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressPlan {
    pub headers: BTreeMap<String, String>,
    pub metadata_user_id: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EgressError {
    #[error("{0} is required for egress")]
    MissingIdentity(&'static str),
    #[error("no OAuth token or API key available for {0}")]
    MissingCredential(ProtocolId),
    #[error("egress planning is only defined for private dialects, got {0}")]
    UnsupportedProtocol(ProtocolId),
}

/// 为私有方言生成 header / body metadata 计划。
pub fn plan(protocol: ProtocolId, identity: &EgressIdentity) -> Result<EgressPlan, EgressError> {
    match protocol {
        ProtocolId::PRIVATE_CLAUDE_CODE => claude_code_plan(identity),
        ProtocolId::PRIVATE_CODEX => codex_plan(identity),
        other => Err(EgressError::UnsupportedProtocol(other)),
    }
}

fn claude_code_plan(identity: &EgressIdentity) -> Result<EgressPlan, EgressError> {
    let oauth = identity
        .oauth_access_token
        .as_deref()
        .filter(|token| !token.is_empty());
    let api_key = identity.api_key.as_deref().filter(|key| !key.is_empty());
    if oauth.is_none() && api_key.is_none() {
        return Err(EgressError::MissingCredential(
            ProtocolId::PRIVATE_CLAUDE_CODE,
        ));
    }

    let version = identity
        .cli_version
        .as_deref()
        .filter(|version| !version.is_empty())
        .unwrap_or(CLAUDE_CODE_VERSION);
    let mut headers = BTreeMap::new();
    headers.insert(
        "anthropic-version".to_string(),
        ANTHROPIC_VERSION.to_string(),
    );
    if let Some(token) = oauth {
        headers.insert("authorization".to_string(), format!("Bearer {token}"));
    } else if let Some(key) = api_key {
        headers.insert("x-api-key".to_string(), key.to_string());
    }
    let required = if oauth.is_some() {
        CLAUDE_CODE_BETAS
    } else {
        CLAUDE_API_KEY_BETAS
    };
    headers.insert(
        "anthropic-beta".to_string(),
        merge_betas(required.iter().copied(), &identity.betas),
    );
    headers.insert(
        "user-agent".to_string(),
        identity
            .user_agent
            .clone()
            .filter(|ua| !ua.is_empty())
            .unwrap_or_else(|| format!("claude-cli/{version} (external, cli)")),
    );
    headers.insert("x-app".to_string(), "cli".to_string());
    headers.insert(
        "anthropic-dangerous-direct-browser-access".to_string(),
        "true".to_string(),
    );
    for (name, value) in claude_stainless_defaults() {
        headers.insert(name.to_string(), value.to_string());
    }
    headers.extend(identity.headers.clone());

    Ok(EgressPlan {
        headers,
        metadata_user_id: Some(claude_metadata_user_id(identity, version)?),
    })
}

fn codex_plan(identity: &EgressIdentity) -> Result<EgressPlan, EgressError> {
    let bearer = identity
        .oauth_access_token
        .as_deref()
        .or(identity.api_key.as_deref())
        .filter(|token| !token.is_empty())
        .ok_or(EgressError::MissingCredential(ProtocolId::PRIVATE_CODEX))?;
    let version = identity
        .cli_version
        .as_deref()
        .filter(|version| !version.is_empty())
        .unwrap_or(CODEX_VERSION);
    let user_agent = identity
        .user_agent
        .clone()
        .filter(|ua| !ua.is_empty())
        .unwrap_or_else(|| format!("codex-tui/{version}{CODEX_UA_SUFFIX}"));

    let mut headers = BTreeMap::new();
    headers.insert("authorization".to_string(), format!("Bearer {bearer}"));
    headers.insert("originator".to_string(), "codex-tui".to_string());
    headers.insert("version".to_string(), version.to_string());
    headers.insert(
        "openai-beta".to_string(),
        "responses=experimental".to_string(),
    );
    headers.insert("user-agent".to_string(), user_agent);
    if let Some(account) = identity.account_id.as_deref().filter(|id| !id.is_empty()) {
        headers.insert("chatgpt-account-id".to_string(), account.to_string());
    }
    if let Some(session) = identity.session_id.as_deref().filter(|id| !id.is_empty()) {
        headers.insert("session_id".to_string(), session.to_string());
    }
    if let Some(conversation) = identity
        .conversation_id
        .as_deref()
        .filter(|id| !id.is_empty())
    {
        headers.insert("conversation_id".to_string(), conversation.to_string());
    }
    headers.extend(identity.headers.clone());

    Ok(EgressPlan {
        headers,
        metadata_user_id: None,
    })
}

/// 按 Claude Code 版本选择 legacy / JSON metadata.user_id 编码。
pub fn claude_metadata_user_id(
    identity: &EgressIdentity,
    version: &str,
) -> Result<String, EgressError> {
    let device = identity
        .device_id
        .as_deref()
        .or(identity.account_id.as_deref())
        .filter(|value| !value.is_empty())
        .ok_or(EgressError::MissingIdentity("device_id or account_id"))?;
    let session = identity
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(EgressError::MissingIdentity("session_id"))?;
    let account_uuid = identity.account_uuid.as_deref().unwrap_or("");
    if version_at_least(version, "2.1.78") {
        let value = json!({
            "device_id": device,
            "account_uuid": account_uuid,
            "session_id": session,
        });
        Ok(value.to_string())
    } else {
        Ok(format!(
            "user_{device}_account_{account_uuid}_session_{session}"
        ))
    }
}

/// required beta 在前，incoming 去重后追加，保持稳定顺序。
pub fn merge_betas(
    required: impl IntoIterator<Item = impl AsRef<str>>,
    incoming: &[String],
) -> String {
    let mut seen = BTreeMap::<String, ()>::new();
    let mut out = Vec::new();
    for beta in required
        .into_iter()
        .map(|value| value.as_ref().to_string())
        .chain(incoming.iter().cloned())
    {
        let beta = beta.trim().to_string();
        if beta.is_empty() || seen.insert(beta.clone(), ()).is_some() {
            continue;
        }
        out.push(beta);
    }
    out.join(",")
}

/// 提供给调用方写入 body 的 metadata patch（纯数据，不修改 body）。
pub fn metadata_patch(plan: &EgressPlan) -> Option<Value> {
    plan.metadata_user_id
        .as_ref()
        .map(|user_id| json!({"metadata": {"user_id": user_id}}))
}

/// 把计划中的 metadata.user_id 写进 JSON body，保留其他字段。
pub fn apply_metadata(body: &[u8], plan: &EgressPlan) -> Result<Vec<u8>, serde_json::Error> {
    let Some(user_id) = plan.metadata_user_id.as_ref() else {
        return Ok(body.to_vec());
    };
    let mut value: Value = serde_json::from_slice(body)?;
    let Some(object) = value.as_object_mut() else {
        return Ok(body.to_vec());
    };
    let metadata = object
        .entry("metadata".to_string())
        .or_insert_with(|| json!({}));
    if !metadata.is_object() {
        *metadata = json!({});
    }
    metadata
        .as_object_mut()
        .expect("metadata object")
        .insert("user_id".to_string(), Value::String(user_id.clone()));
    serde_json::to_vec(&value)
}

fn claude_stainless_defaults() -> &'static [(&'static str, &'static str)] {
    &[
        ("x-stainless-lang", "js"),
        ("x-stainless-package-version", "0.94.0"),
        ("x-stainless-os", "Linux"),
        ("x-stainless-arch", "arm64"),
        ("x-stainless-runtime", "node"),
        ("x-stainless-runtime-version", "v24.3.0"),
        ("x-stainless-retry-count", "0"),
        ("x-stainless-timeout", "600"),
    ]
}

fn version_at_least(version: &str, minimum: &str) -> bool {
    let parse = |value: &str| {
        value
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let mut left = parse(version);
    let mut right = parse(minimum);
    let len = left.len().max(right.len());
    left.resize(len, 0);
    right.resize(len, 0);
    left >= right
}
