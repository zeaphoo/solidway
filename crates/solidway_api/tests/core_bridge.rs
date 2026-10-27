//! 控制面 → 数据面配置映射的集成测试（DB → `CoreConfig`，含解密与健康回灌）。

use std::path::PathBuf;

use solidway_api::db::{api_keys, provider_health, provider_keys, routes};
use solidway_api::state::{new_id, now_unix};
use solidway_api::{Api, ApiConfig};

async fn temp_api() -> (Api, PathBuf) {
    let dir = std::env::temp_dir().join(format!("solidway-api-test-{}", new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let api = Api::init(ApiConfig {
        data_root: dir.clone(),
        master_key: None,
        mode: solidway_api::RunMode::Standalone,
        web_assets: None,
    })
    .await
    .expect("init api");
    (api, dir)
}

#[tokio::test]
async fn builds_core_config_with_decrypted_credentials_and_health_seed() {
    let (api, dir) = temp_api().await;
    let state = api.state();
    let now = now_unix();

    // 一条路由
    let route_id = new_id();
    routes::create(
        &state.pool,
        &routes::Route {
            id: route_id.clone(),
            owner_id: None,
            name: "main".to_string(),
            description: None,
            enabled: 1,
            created_at: now,
            updated_at: now,
            selection_strategy: "round_robin".to_string(),
            selection_config: "{}".to_string(),
            archived_at: None,
            archived_by: None,
        },
    )
    .await
    .unwrap();

    // 一条 provider 凭据（明文经 cipher 加密后落库）
    let provider_id = new_id();
    let aad = format!("provider_key|{provider_id}");
    let token = "sk-upstream-secret";
    provider_keys::insert(
        &state.pool,
        &provider_keys::ProviderKeyRow {
            id: provider_id.clone(),
            route_id: route_id.clone(),
            provider: "openai".to_string(),
            name: Some("p1".to_string()),
            base_url: Some("https://up.example.com/v1".to_string()),
            api_formats: r#"["chat_completions","responses"]"#.to_string(),
            auth_scheme: "api-key".to_string(),
            credential_kind: "api_key".to_string(),
            credential_ciphertext: state.cipher.encrypt(token, &aad).unwrap(),
            credential_masked: "****cret".to_string(),
            credential_hash: state.cipher.fingerprint(token).unwrap(),
            options: None,
            models: r#"[{"name":"gpt-5","upstream":"gpt-5-2025"},{"name":"gpt-5-mini","upstream":"gpt-5-mini"}]"#.to_string(),
            priority: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .unwrap();

    // 一把客户端 API Key
    let api_key_id = new_id();
    api_keys::insert(
        &state.pool,
        &api_keys::ApiKeyRow {
            id: api_key_id.clone(),
            route_id: route_id.clone(),
            name: None,
            key_prefix: "sw-abcd".to_string(),
            key_masked: "****wxyz".to_string(),
            key_hash: solidway_core::hash_api_key("sw-client"),
            status: "active".to_string(),
            expires_at: None,
            last_used_at: None,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .unwrap();

    // 一条健康标记（冷却）
    provider_health::upsert(
        &state.pool,
        &provider_id,
        "quarantined",
        "http_429",
        Some(now + 300),
        now,
    )
    .await
    .unwrap();

    let config = solidway_api::core_bridge::build_core_config(state)
        .await
        .expect("build core config");

    // 路由映射
    assert_eq!(config.routes.len(), 1);
    assert!(config.routes[0].enabled);
    // provider 映射：api_key 已解密、models 已解析、启停映射
    assert_eq!(config.providers.len(), 1);
    let provider = &config.providers[0];
    assert_eq!(provider.api_key, token, "provider api_key 必须解密后下发");
    assert_eq!(
        provider.models,
        vec![
            solidway_core::ModelMapping {
                name: "gpt-5".to_string(),
                upstream: "gpt-5-2025".to_string(),
            },
            solidway_core::ModelMapping {
                name: "gpt-5-mini".to_string(),
                upstream: "gpt-5-mini".to_string(),
            },
        ]
    );
    assert!(provider.enabled);
    assert_eq!(provider.auth_scheme, "api-key");
    // API Key 映射（只下发哈希）
    assert_eq!(config.api_keys.len(), 1);
    assert_eq!(
        config.api_keys[0].key_hash,
        solidway_core::hash_api_key("sw-client")
    );
    assert!(config.api_keys[0].enabled);
    // 健康回灌（含冷却截止）
    assert_eq!(config.health.len(), 1);
    assert_eq!(config.health[0].provider_id, provider_id);
    match &config.health[0].state {
        solidway_core::HealthState::Quarantined { until, reason } => {
            assert_eq!(*until, now + 300);
            assert_eq!(reason, "http_429");
        }
        other => panic!("期望 Quarantined，实际 {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn disabled_route_and_key_are_mapped_as_disabled() {
    let (api, dir) = temp_api().await;
    let state = api.state();
    let now = now_unix();

    let route_id = new_id();
    routes::create(
        &state.pool,
        &routes::Route {
            id: route_id.clone(),
            owner_id: None,
            name: "off".to_string(),
            description: None,
            enabled: 0,
            created_at: now,
            updated_at: now,
            selection_strategy: "round_robin".to_string(),
            selection_config: "{}".to_string(),
            archived_at: None,
            archived_by: None,
        },
    )
    .await
    .unwrap();
    api_keys::insert(
        &state.pool,
        &api_keys::ApiKeyRow {
            id: new_id(),
            route_id: route_id.clone(),
            name: None,
            key_prefix: "sw-off".to_string(),
            key_masked: "****off0".to_string(),
            key_hash: solidway_core::hash_api_key("sw-off"),
            status: "inactive".to_string(),
            expires_at: None,
            last_used_at: None,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .unwrap();

    let config = solidway_api::core_bridge::build_core_config(state)
        .await
        .unwrap();
    assert!(!config.routes[0].enabled);
    assert!(!config.api_keys[0].enabled);

    let _ = std::fs::remove_dir_all(&dir);
}
