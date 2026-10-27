//! 协议标识与方向级能力矩阵。

use serde::{Deserialize, Serialize};

/// 协议标识。用 `&'static str` 而非封闭 enum，新增私有协议不需要改核心枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProtocolId(pub &'static str);

impl ProtocolId {
    pub const OPENAI_CHAT: Self = Self("openai.chat");
    pub const OPENAI_RESPONSES: Self = Self("openai.responses");
    pub const ANTHROPIC_MESSAGES: Self = Self("anthropic.messages");
    pub const PRIVATE_CODEX: Self = Self("private.codex");
    pub const PRIVATE_CLAUDE_CODE: Self = Self("private.claude-code");

    pub const PUBLIC: [Self; 3] = [
        Self::OPENAI_CHAT,
        Self::OPENAI_RESPONSES,
        Self::ANTHROPIC_MESSAGES,
    ];
}

impl std::fmt::Display for ProtocolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireKind {
    Json,
    Sse,
    AwsEventStream,
}

/// 某个方向对一类语义的支持级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Support {
    #[default]
    Native,
    Emulated,
    Lossy(&'static str),
    Unsupported(&'static str),
}

impl Support {
    fn rank(self) -> u8 {
        match self {
            Self::Native => 0,
            Self::Emulated => 1,
            Self::Lossy(_) => 2,
            Self::Unsupported(_) => 3,
        }
    }

    /// 组合方向取更差的支持级别。
    pub fn narrow(self, other: Self) -> Self {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestCaps {
    pub system_prompt: Support,
    pub developer_role: Support,
    pub images: Support,
    pub documents: Support,
    pub audio_input: Support,
    pub tools: Support,
    pub parallel_tool_calls: Support,
    pub custom_tools: Support,
    pub hosted_tools: Support,
    pub tool_choice: Support,
    pub thinking: Support,
    pub cache_control: Support,
    pub structured_output: Support,
    pub stop: Support,
    pub top_k: Support,
    pub seed: Support,
    pub penalties: Support,
    pub n_choices: Support,
    pub logprobs: Support,
}

impl RequestCaps {
    pub const DEFAULT: Self = Self {
        system_prompt: Support::Native,
        developer_role: Support::Native,
        images: Support::Native,
        documents: Support::Native,
        audio_input: Support::Native,
        tools: Support::Native,
        parallel_tool_calls: Support::Native,
        custom_tools: Support::Native,
        hosted_tools: Support::Native,
        tool_choice: Support::Native,
        thinking: Support::Native,
        cache_control: Support::Native,
        structured_output: Support::Native,
        stop: Support::Native,
        top_k: Support::Native,
        seed: Support::Native,
        penalties: Support::Native,
        n_choices: Support::Native,
        logprobs: Support::Native,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseCaps {
    pub thinking: Support,
    pub thinking_signature: Support,
    pub citations: Support,
    pub refusal: Support,
    pub server_tools: Support,
    pub usage_cache_tokens: Support,
    pub usage_reasoning_tokens: Support,
}

impl ResponseCaps {
    pub const DEFAULT: Self = Self {
        thinking: Support::Native,
        thinking_signature: Support::Native,
        citations: Support::Native,
        refusal: Support::Native,
        server_tools: Support::Native,
        usage_cache_tokens: Support::Native,
        usage_reasoning_tokens: Support::Native,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamCaps {
    pub text: Support,
    pub thinking: Support,
    pub tool_calls: Support,
    pub parallel_tool_calls: Support,
    pub usage: Support,
    pub errors: Support,
    pub keepalive: Support,
}

impl StreamCaps {
    pub const DEFAULT: Self = Self {
        text: Support::Native,
        thinking: Support::Native,
        tool_calls: Support::Native,
        parallel_tool_calls: Support::Native,
        usage: Support::Native,
        errors: Support::Native,
        keepalive: Support::Native,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectionCaps {
    pub request: RequestCaps,
    pub response: ResponseCaps,
    pub stream: StreamCaps,
}

impl DirectionCaps {
    pub const DEFAULT: Self = Self {
        request: RequestCaps::DEFAULT,
        response: ResponseCaps::DEFAULT,
        stream: StreamCaps::DEFAULT,
    };
}

macro_rules! narrow_caps {
    ($name:ident { $($field:ident),+ $(,)? }) => {
        impl $name {
            pub fn narrow(self, other: Self) -> Self {
                Self {
                    $($field: self.$field.narrow(other.$field),)+
                }
            }
        }
    };
}

narrow_caps!(RequestCaps {
    system_prompt,
    developer_role,
    images,
    documents,
    audio_input,
    tools,
    parallel_tool_calls,
    custom_tools,
    hosted_tools,
    tool_choice,
    thinking,
    cache_control,
    structured_output,
    stop,
    top_k,
    seed,
    penalties,
    n_choices,
    logprobs,
});

narrow_caps!(ResponseCaps {
    thinking,
    thinking_signature,
    citations,
    refusal,
    server_tools,
    usage_cache_tokens,
    usage_reasoning_tokens,
});

narrow_caps!(StreamCaps {
    text,
    thinking,
    tool_calls,
    parallel_tool_calls,
    usage,
    errors,
    keepalive,
});

impl DirectionCaps {
    pub fn narrow(self, other: Self) -> Self {
        Self {
            request: self.request.narrow(other.request),
            response: self.response.narrow(other.response),
            stream: self.stream.narrow(other.stream),
        }
    }
}
