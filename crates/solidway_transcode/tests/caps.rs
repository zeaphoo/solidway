//! caps 矩阵：每个方向的能力声明必须自洽，组合方向不得比 base 更宽。

use solidway_transcode::protocol::{DirectionCaps, ProtocolId, Support};
use solidway_transcode::registry::{all_directions, direction};

fn rank(support: Support) -> u8 {
    match support {
        Support::Native => 0,
        Support::Emulated => 1,
        Support::Lossy(_) => 2,
        Support::Unsupported(_) => 3,
    }
}

fn assert_reason(name: &str, support: Support) {
    match support {
        Support::Lossy(reason) | Support::Unsupported(reason) => {
            assert!(!reason.trim().is_empty(), "{name} has an empty reason");
        }
        Support::Native | Support::Emulated => {}
    }
}

fn request_fields(caps: &DirectionCaps) -> [(&'static str, Support); 19] {
    [
        ("system_prompt", caps.request.system_prompt),
        ("developer_role", caps.request.developer_role),
        ("images", caps.request.images),
        ("documents", caps.request.documents),
        ("audio_input", caps.request.audio_input),
        ("tools", caps.request.tools),
        ("parallel_tool_calls", caps.request.parallel_tool_calls),
        ("custom_tools", caps.request.custom_tools),
        ("hosted_tools", caps.request.hosted_tools),
        ("tool_choice", caps.request.tool_choice),
        ("thinking", caps.request.thinking),
        ("cache_control", caps.request.cache_control),
        ("structured_output", caps.request.structured_output),
        ("stop", caps.request.stop),
        ("top_k", caps.request.top_k),
        ("seed", caps.request.seed),
        ("penalties", caps.request.penalties),
        ("n_choices", caps.request.n_choices),
        ("logprobs", caps.request.logprobs),
    ]
}

fn response_fields(caps: &DirectionCaps) -> [(&'static str, Support); 7] {
    [
        ("thinking", caps.response.thinking),
        ("thinking_signature", caps.response.thinking_signature),
        ("citations", caps.response.citations),
        ("refusal", caps.response.refusal),
        ("server_tools", caps.response.server_tools),
        ("usage_cache_tokens", caps.response.usage_cache_tokens),
        (
            "usage_reasoning_tokens",
            caps.response.usage_reasoning_tokens,
        ),
    ]
}

fn stream_fields(caps: &DirectionCaps) -> [(&'static str, Support); 7] {
    [
        ("text", caps.stream.text),
        ("thinking", caps.stream.thinking),
        ("tool_calls", caps.stream.tool_calls),
        ("parallel_tool_calls", caps.stream.parallel_tool_calls),
        ("usage", caps.stream.usage),
        ("errors", caps.stream.errors),
        ("keepalive", caps.stream.keepalive),
    ]
}

fn base_protocol(protocol: ProtocolId) -> ProtocolId {
    match protocol {
        ProtocolId::PRIVATE_CODEX => ProtocolId::OPENAI_RESPONSES,
        ProtocolId::PRIVATE_CLAUDE_CODE => ProtocolId::ANTHROPIC_MESSAGES,
        other => other,
    }
}

fn assert_not_wider(
    label: &str,
    ours: [(&'static str, Support); 19],
    base: [(&'static str, Support); 19],
) {
    for ((name, ours), (_, base)) in ours.into_iter().zip(base) {
        assert!(
            rank(ours) >= rank(base),
            "{label}/{name} is wider than its base: {ours:?} vs {base:?}"
        );
    }
}

#[test]
fn caps_are_well_formed_and_composed_directions_are_narrower() {
    for current in all_directions() {
        let src = current.src();
        let dst = current.dst();
        let caps = current.caps();
        let label = format!("{src} -> {dst}");

        for (name, support) in request_fields(caps) {
            assert_reason(&format!("{label}/request/{name}"), support);
        }
        for (name, support) in response_fields(caps) {
            assert_reason(&format!("{label}/response/{name}"), support);
        }
        for (name, support) in stream_fields(caps) {
            assert_reason(&format!("{label}/stream/{name}"), support);
        }

        let base_src = base_protocol(src);
        let base_dst = base_protocol(dst);
        if base_src == base_dst {
            continue;
        }
        let Some(base_direction) = direction(base_src, base_dst) else {
            continue;
        };
        assert_not_wider(
            &format!("{label} request"),
            request_fields(caps),
            request_fields(base_direction.caps()),
        );
        for ((name, ours), (_, base)) in response_fields(caps)
            .into_iter()
            .zip(response_fields(base_direction.caps()))
        {
            assert!(
                rank(ours) >= rank(base),
                "{label}/response/{name} is wider than its base: {ours:?} vs {base:?}"
            );
        }
        for ((name, ours), (_, base)) in stream_fields(caps)
            .into_iter()
            .zip(stream_fields(base_direction.caps()))
        {
            assert!(
                rank(ours) >= rank(base),
                "{label}/stream/{name} is wider than its base: {ours:?} vs {base:?}"
            );
        }
    }
}
