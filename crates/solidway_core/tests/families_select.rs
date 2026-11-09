//! Anthropic 的换号与会话记录（`last_used`）行为。

mod common;

use common::*;
use solidway_core::{RouteSelection, SelectionStrategy};

fn with_strategy(mut cfg: solidway_core::CoreConfig, strategy: SelectionStrategy) -> solidway_core::CoreConfig {
    cfg.routes[0].selection = RouteSelection::for_strategy(strategy);
    cfg
}

#[tokio::test]
async fn anthropic_last_used_reuses_same_provider_for_session() {
    let a = spawn_mock(Behavior::Ok).await;
    let b = spawn_mock(Behavior::Ok).await;
    let cfg = with_strategy(
        base_config(
            vec![
                anthropic_provider("a", a.base_url(), &["claude"]),
                anthropic_provider("b", b.base_url(), &["claude"]),
            ],
            fast_options(),
        ),
        SelectionStrategy::LastUsed,
    );
    let (base, _core) = spawn_core(cfg).await;

    let body = br#"{"model":"claude","max_tokens":8,"metadata":{"user_id":"u-42"}}"#.to_vec();
    for _ in 0..3 {
        let resp = post_raw(
            &base,
            "/v1/messages",
            &[("x-api-key", CLIENT_KEY)],
            body.clone(),
        )
        .await;
        assert_eq!(resp.status(), 200);
    }

    let (first, second) = if a.hit_count() > 0 { (a, b) } else { (b, a) };
    assert_eq!(first.hit_count(), 3, "同会话复用同一凭据");
    assert_eq!(second.hit_count(), 0);
}

#[tokio::test]
async fn anthropic_round_robin_ignores_session_record() {
    let a = spawn_mock(Behavior::Ok).await;
    let b = spawn_mock(Behavior::Ok).await;
    // 默认策略（round_robin）不读会话记录：同会话应分散到两个凭据。
    let cfg = base_config(
        vec![
            anthropic_provider("a", a.base_url(), &["claude"]),
            anthropic_provider("b", b.base_url(), &["claude"]),
        ],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let body = br#"{"model":"claude","max_tokens":8,"metadata":{"user_id":"u-42"}}"#.to_vec();
    for _ in 0..4 {
        let resp = post_raw(
            &base,
            "/v1/messages",
            &[("x-api-key", CLIENT_KEY)],
            body.clone(),
        )
        .await;
        assert_eq!(resp.status(), 200);
    }

    assert_eq!(a.hit_count(), 2, "轮询不使用会话记录");
    assert_eq!(b.hit_count(), 2, "轮询不使用会话记录");
}
