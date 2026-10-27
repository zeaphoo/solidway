use std::collections::BTreeMap;

use serde_json::Value;
use solidway_egress::{
    EgressError, EgressIdentity, claude_metadata_user_id, merge_betas, metadata_patch, plan,
};
use solidway_transcode::protocol::ProtocolId;

fn claude_identity() -> EgressIdentity {
    EgressIdentity {
        account_id: Some("acc_1".to_string()),
        account_uuid: Some("uuid_1".to_string()),
        device_id: Some("a".repeat(64)),
        session_id: Some("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string()),
        oauth_access_token: Some("oauth-token".to_string()),
        cli_version: Some("2.1.77".to_string()),
        ..EgressIdentity::default()
    }
}

#[test]
fn claude_legacy_metadata_and_oauth_headers() {
    let identity = claude_identity();
    let plan = plan(ProtocolId::PRIVATE_CLAUDE_CODE, &identity).unwrap();
    assert_eq!(
        plan.metadata_user_id.as_deref(),
        Some(
            "user_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa_account_uuid_1_session_bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
        )
    );
    assert_eq!(
        plan.headers.get("authorization").unwrap(),
        "Bearer oauth-token"
    );
    assert!(!plan.headers.contains_key("x-api-key"));
    assert_eq!(plan.headers.get("anthropic-version").unwrap(), "2023-06-01");
    assert_eq!(plan.headers.get("x-app").unwrap(), "cli");
    assert_eq!(
        plan.headers.get("user-agent").unwrap(),
        "claude-cli/2.1.77 (external, cli)"
    );
    let betas = plan.headers.get("anthropic-beta").unwrap();
    assert!(betas.contains("oauth-2025-04-20"));
    assert!(betas.contains("claude-code-20250219"));
    assert!(betas.contains("fine-grained-tool-streaming-2025-05-14"));
}

#[test]
fn claude_new_metadata_is_json() {
    let mut identity = claude_identity();
    identity.cli_version = Some("2.1.78".to_string());
    let value = claude_metadata_user_id(&identity, "2.1.78").unwrap();
    let parsed: Value = serde_json::from_str(&value).unwrap();
    assert_eq!(parsed["device_id"], "a".repeat(64));
    assert_eq!(parsed["account_uuid"], "uuid_1");
    assert_eq!(parsed["session_id"], "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
}

#[test]
fn api_key_betas_omit_oauth() {
    let mut identity = claude_identity();
    identity.oauth_access_token = None;
    identity.api_key = Some("sk-ant".to_string());
    let plan = plan(ProtocolId::PRIVATE_CLAUDE_CODE, &identity).unwrap();
    assert_eq!(plan.headers.get("x-api-key").unwrap(), "sk-ant");
    assert!(!plan.headers.contains_key("authorization"));
    let betas = plan.headers.get("anthropic-beta").unwrap();
    assert!(!betas.contains("oauth-2025-04-20"));
    assert!(betas.contains("claude-code-20250219"));
}

#[test]
fn beta_merge_preserves_order_and_deduplicates() {
    let incoming = vec![
        "oauth-2025-04-20".to_string(),
        "custom-beta".to_string(),
        " custom-beta ".to_string(),
    ];
    let merged = merge_betas(
        [
            "claude-code-20250219",
            "oauth-2025-04-20",
            "interleaved-thinking-2025-05-14",
        ],
        &incoming,
    );
    assert_eq!(
        merged,
        "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,custom-beta"
    );
}

#[test]
fn codex_headers_include_account_and_session() {
    let mut identity = EgressIdentity {
        account_id: Some("chatgpt-acc".to_string()),
        session_id: Some("sess-1".to_string()),
        conversation_id: Some("conv-1".to_string()),
        oauth_access_token: Some("codex-token".to_string()),
        ..EgressIdentity::default()
    };
    identity
        .headers
        .insert("x-codex-window-id".to_string(), "win-1".to_string());
    let plan = plan(ProtocolId::PRIVATE_CODEX, &identity).unwrap();
    assert_eq!(plan.metadata_user_id, None);
    assert_eq!(
        plan.headers.get("authorization").unwrap(),
        "Bearer codex-token"
    );
    assert_eq!(
        plan.headers.get("chatgpt-account-id").unwrap(),
        "chatgpt-acc"
    );
    assert_eq!(plan.headers.get("originator").unwrap(), "codex-tui");
    assert_eq!(plan.headers.get("version").unwrap(), "0.146.0");
    assert_eq!(
        plan.headers.get("openai-beta").unwrap(),
        "responses=experimental"
    );
    assert_eq!(plan.headers.get("session_id").unwrap(), "sess-1");
    assert_eq!(plan.headers.get("conversation_id").unwrap(), "conv-1");
    assert_eq!(plan.headers.get("x-codex-window-id").unwrap(), "win-1");
}

#[test]
fn missing_credentials_and_unsupported_protocols_are_errors() {
    assert_eq!(
        plan(ProtocolId::PRIVATE_CODEX, &EgressIdentity::default()),
        Err(EgressError::MissingCredential(ProtocolId::PRIVATE_CODEX))
    );
    assert_eq!(
        plan(ProtocolId::OPENAI_CHAT, &EgressIdentity::default()),
        Err(EgressError::UnsupportedProtocol(ProtocolId::OPENAI_CHAT))
    );
}

#[test]
fn metadata_patch_is_a_plain_value() {
    let plan = plan(ProtocolId::PRIVATE_CLAUDE_CODE, &claude_identity()).unwrap();
    let patch = metadata_patch(&plan).unwrap();
    assert_eq!(patch["metadata"]["user_id"], plan.metadata_user_id.unwrap());
}

#[test]
fn extra_headers_override_defaults() {
    let mut identity = claude_identity();
    identity
        .headers
        .insert("x-app".to_string(), "custom".to_string());
    identity
        .headers
        .insert("anthropic-version".to_string(), "2099-01-01".to_string());
    let plan = plan(ProtocolId::PRIVATE_CLAUDE_CODE, &identity).unwrap();
    assert_eq!(plan.headers.get("x-app").unwrap(), "custom");
    assert_eq!(plan.headers.get("anthropic-version").unwrap(), "2099-01-01");
}

#[test]
fn metadata_requires_device_and_session() {
    let identity = EgressIdentity::default();
    assert_eq!(
        claude_metadata_user_id(&identity, "2.1.78"),
        Err(EgressError::MissingIdentity("device_id or account_id"))
    );
    let identity = EgressIdentity {
        account_id: Some("acc".to_string()),
        ..EgressIdentity::default()
    };
    assert_eq!(
        claude_metadata_user_id(&identity, "2.1.78"),
        Err(EgressError::MissingIdentity("session_id"))
    );
}

#[test]
fn stainless_defaults_are_present() {
    let plan = plan(ProtocolId::PRIVATE_CLAUDE_CODE, &claude_identity()).unwrap();
    let expected: BTreeMap<&str, &str> = BTreeMap::from([
        ("x-stainless-lang", "js"),
        ("x-stainless-package-version", "0.94.0"),
        ("x-stainless-runtime", "node"),
    ]);
    for (name, value) in expected {
        assert_eq!(plan.headers.get(name).unwrap(), value);
    }
}

#[test]
fn apply_metadata_preserves_other_body_fields() {
    let plan = plan(ProtocolId::PRIVATE_CLAUDE_CODE, &claude_identity()).unwrap();
    let body = br#"{"model":"m","metadata":{"trace":"keep"},"messages":[]}"#;
    let patched: Value =
        serde_json::from_slice(&solidway_egress::apply_metadata(body, &plan).unwrap()).unwrap();
    assert_eq!(patched["metadata"]["trace"], "keep");
    assert_eq!(
        patched["metadata"]["user_id"],
        plan.metadata_user_id.unwrap()
    );
    assert_eq!(patched["model"], "m");
}

#[test]
fn apply_metadata_is_noop_for_codex() {
    let identity = EgressIdentity {
        oauth_access_token: Some("token".to_string()),
        ..EgressIdentity::default()
    };
    let plan = plan(ProtocolId::PRIVATE_CODEX, &identity).unwrap();
    let body = br#"{"model":"m"}"#;
    assert_eq!(
        solidway_egress::apply_metadata(body, &plan).unwrap(),
        body.to_vec()
    );
}
