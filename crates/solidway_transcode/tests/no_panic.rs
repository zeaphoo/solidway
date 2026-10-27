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
