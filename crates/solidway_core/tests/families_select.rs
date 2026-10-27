//! Anthropic 的换号与粘性行为。

mod common;

use common::*;

#[tokio::test]
async fn anthropic_affinity_reuses_same_provider_for_session() {
    let a = spawn_mock(Behavior::Ok).await;
    let b = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![
            anthropic_provider("a", a.base_url(), &["claude"]),
            anthropic_provider("b", b.base_url(), &["claude"]),
        ],
        fast_options(),
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
