//! 任意字节输入不 panic（无 proptest 依赖的等价健壮性门槛）。

use solidway_transcode::protocol::ProtocolId;
use solidway_transcode::registry::{InFrame, Plan, direction, stream_translator};
use solidway_transcode::report::TranscodePolicy;

fn protocols() -> [ProtocolId; 5] {
    [
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::PRIVATE_CODEX,
        ProtocolId::PRIVATE_CLAUDE_CODE,
    ]
}

fn inputs() -> Vec<Vec<u8>> {
    vec![
        Vec::new(),
        b"null".to_vec(),
        b"[]".to_vec(),
        b"{}".to_vec(),
        br#"{"model":1}"#.to_vec(),
        br#"{"messages":"x"}"#.to_vec(),
        br#"{"type":"error","error":{}}"#.to_vec(),
        br#"{"content":[{"type":"text","text":null}]}"#.to_vec(),
        vec![0xff, 0xfe, 0xfd],
        b"data: not-json\n\n".to_vec(),
    ]
}

#[test]
fn overflowing_usage_does_not_panic() {
    let max = u64::MAX;
    let bodies = vec![
        format!(
            r#"{{"id":"chatcmpl-1","object":"chat.completion","created":0,"model":"m","choices":[{{"index":0,"message":{{"role":"assistant","content":"hi"}},"finish_reason":"stop"}}],"usage":{{"prompt_tokens":{max},"completion_tokens":{max},"prompt_tokens_details":{{"cached_tokens":{max},"cache_creation_tokens":{max}}}}}}}"#
        )
        .into_bytes(),
        format!(
            r#"{{"id":"resp-1","object":"response","created_at":0,"model":"m","status":"completed","output":[],"usage":{{"input_tokens":{max},"output_tokens":{max},"cache_creation_input_tokens":{max},"input_tokens_details":{{"cached_tokens":{max}}},"output_tokens_details":{{"reasoning_tokens":{max}}}}}}}"#
        )
        .into_bytes(),
        format!(
            r#"{{"id":"msg-1","type":"message","role":"assistant","model":"m","content":[{{"type":"text","text":"hi"}}],"stop_reason":"end_turn","usage":{{"input_tokens":{max},"output_tokens":{max},"cache_read_input_tokens":{max},"cache_creation_input_tokens":{max}}}}}"#
        )
        .into_bytes(),
    ];

    for src in protocols() {
        for dst in protocols() {
            if src == dst {
                continue;
            }
            let direction = direction(src, dst).unwrap();
            let plan = Plan {
                src,
                dst,
                policy: TranscodePolicy::BestEffort,
            };
            for body in &bodies {
                let _ = direction.request(body, &plan);
                let _ = direction.response(body, &plan);
                let _ = direction.stream(&plan).and_then(|mut bridge| {
                    let _ = bridge.push(InFrame::Sse {
                        event: None,
                        id: None,
                        data: body.clone(),
                    });
                    bridge.finish()
                });
            }
        }
    }
}

/// `u64::MAX + u64::MAX` 曾以 `+` 计算（debug 溢出 panic）；现在必须饱和。
#[test]
fn synthesized_totals_saturate_instead_of_overflowing() {
    let max = u64::MAX;

    // Responses -> Chat：`total_tokens` 缺失时由 input + output 合成。
    let body = format!(
        r#"{{"id":"resp-1","object":"response","created_at":0,"model":"m","status":"completed","output":[],"usage":{{"input_tokens":{max},"output_tokens":{max}}}}}"#
    );
    let plan = Plan {
        src: ProtocolId::OPENAI_CHAT,
        dst: ProtocolId::OPENAI_RESPONSES,
        policy: TranscodePolicy::BestEffort,
    };
    let out = direction(plan.src, plan.dst)
        .unwrap()
        .response(body.as_bytes(), &plan)
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.body).unwrap();
    assert_eq!(value["usage"]["total_tokens"], serde_json::json!(max));

    // Anthropic -> Chat：`prompt_tokens` 由 input + cache_read + cache_creation 合成。
    let body = format!(
        r#"{{"id":"msg-1","type":"message","role":"assistant","model":"m","content":[{{"type":"text","text":"hi"}}],"stop_reason":"end_turn","usage":{{"input_tokens":{max},"output_tokens":{max},"cache_read_input_tokens":{max},"cache_creation_input_tokens":{max}}}}}"#
    );
    let out = direction(ProtocolId::OPENAI_CHAT, ProtocolId::ANTHROPIC_MESSAGES)
        .unwrap()
        .response(body.as_bytes(), &plan)
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.body).unwrap();
    assert_eq!(value["usage"]["prompt_tokens"], serde_json::json!(max));
    assert_eq!(value["usage"]["total_tokens"], serde_json::json!(max));

    // Chat -> Anthropic：派生 input 饱和到 0，不下溢。
    let body = format!(
        r#"{{"id":"chatcmpl-1","object":"chat.completion","created":0,"model":"m","choices":[{{"index":0,"message":{{"role":"assistant","content":"hi"}},"finish_reason":"stop"}}],"usage":{{"prompt_tokens":1,"completion_tokens":{max},"prompt_tokens_details":{{"cached_tokens":{max},"cache_creation_tokens":{max}}}}}}}"#
    );
    let plan = Plan {
        src: ProtocolId::ANTHROPIC_MESSAGES,
        dst: ProtocolId::OPENAI_CHAT,
        policy: TranscodePolicy::BestEffort,
    };
    let out = direction(plan.src, plan.dst)
        .unwrap()
        .response(body.as_bytes(), &plan)
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.body).unwrap();
    assert_eq!(value["usage"]["input_tokens"], serde_json::json!(0));
    assert_eq!(value["usage"]["output_tokens"], serde_json::json!(max));
}

#[test]
fn malformed_inputs_do_not_panic() {
    let inputs = inputs();
    for src in protocols() {
        for dst in protocols() {
            if src == dst {
                continue;
            }
            let direction = direction(src, dst).unwrap();
            let plan = Plan {
                src,
                dst,
                policy: TranscodePolicy::BestEffort,
            };
            for input in &inputs {
                let _ = direction.request(input, &plan);
                let _ = direction.response(input, &plan);
                let _ = direction.stream(&plan).and_then(|mut bridge| {
                    let _ = bridge.push(InFrame::Sse {
                        event: None,
                        id: None,
                        data: input.clone(),
                    });
                    bridge.finish()
                });
                if let Ok(mut translator) = stream_translator(src, dst, &plan) {
                    let _ = translator.feed(input);
                    let _ = translator.finish();
                }
            }
        }
    }
}
