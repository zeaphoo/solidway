//! 性能预算（默认忽略；用 release 手动跑）。
//!
//! ```txt
//! cargo test -p solidway-transcode --test perf -- --ignored --release
//! ```

use std::time::{Duration, Instant};

use solidway_transcode::protocol::ProtocolId;
use solidway_transcode::registry::{Plan, direction};
use solidway_transcode::report::TranscodePolicy;

#[test]
#[ignore = "performance budget; run with --release"]
fn ten_kib_request_conversion_budget() {
    let body = serde_json::json!({
        "model": "gpt-5.1",
        "messages": [{
            "role": "user",
            "content": "x".repeat(10 * 1024)
        }]
    })
    .to_string();

    let plan = Plan {
        src: ProtocolId::OPENAI_CHAT,
        dst: ProtocolId::ANTHROPIC_MESSAGES,
        policy: TranscodePolicy::BestEffort,
    };
    let direction = direction(ProtocolId::OPENAI_CHAT, ProtocolId::ANTHROPIC_MESSAGES).unwrap();

    for _ in 0..10 {
        direction.request(body.as_bytes(), &plan).unwrap();
    }

    let iterations = 100;
    let start = Instant::now();
    for _ in 0..iterations {
        direction.request(body.as_bytes(), &plan).unwrap();
    }
    let per_request = start.elapsed() / iterations;
    let limit = if cfg!(debug_assertions) {
        Duration::from_millis(50)
    } else {
        Duration::from_millis(1)
    };
    assert!(
        per_request < limit,
        "10 KiB request conversion took {per_request:?}, limit {limit:?}"
    );
}
