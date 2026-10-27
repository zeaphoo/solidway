//! Anthropic Messages -> Chat Completions。

mod request;
mod response;
mod stream;

use crate::protocol::{DirectionCaps, ProtocolId, RequestCaps, ResponseCaps, StreamCaps, Support};
use crate::registry::{Direction, Plan, StreamBridge, Transcoded};
use crate::report::Error;

pub(crate) struct AnthropicToChat;

pub(crate) static DIRECTION: AnthropicToChat = AnthropicToChat;

pub(crate) const CAPS: DirectionCaps = DirectionCaps {
    request: RequestCaps {
        documents: Support::Emulated,
        audio_input: Support::Unsupported("Chat has no Anthropic audio input"),
        custom_tools: Support::Unsupported("Anthropic has no custom tools"),
        hosted_tools: Support::Unsupported("Chat has no server tools"),
        top_k: Support::Unsupported("Chat has no top_k"),
        cache_control: Support::Lossy("Anthropic cache_control is dropped"),
        thinking: Support::Lossy("Anthropic thinking is mapped to reasoning_effort"),
        structured_output: Support::Lossy("Anthropic output_config.format is not mapped"),
        ..RequestCaps::DEFAULT
    },
    response: ResponseCaps {
        refusal: Support::Lossy("refusal is mapped to a text block"),
        thinking_signature: Support::Lossy("Chat has no thinking signature field"),
        citations: Support::Lossy("Chat has no citations field"),
        server_tools: Support::Unsupported("Chat has no server tool blocks"),
        usage_reasoning_tokens: Support::Lossy("Anthropic usage has no separate reasoning tokens"),
        ..ResponseCaps::DEFAULT
    },
    stream: StreamCaps {
        thinking: Support::Lossy("Chat reasoning_content is streamed as Anthropic thinking"),
        parallel_tool_calls: Support::Lossy("Anthropic content blocks are sequential"),
        keepalive: Support::Unsupported("Anthropic stream has no ping event"),
        ..StreamCaps::DEFAULT
    },
};

impl Direction for AnthropicToChat {
    fn src(&self) -> ProtocolId {
        ProtocolId::ANTHROPIC_MESSAGES
    }

    fn dst(&self) -> ProtocolId {
        ProtocolId::OPENAI_CHAT
    }

    fn caps(&self) -> &'static DirectionCaps {
        &CAPS
    }

    fn request(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
        let req: crate::wire::anthropic::AnthropicRequest =
            serde_json::from_slice(body).map_err(|err| Error::Invalid(err.to_string()))?;
        let (out, report) = request::convert(req, plan)?;
        Ok(Transcoded {
            body: serde_json::to_vec(&out)?,
            report,
        })
    }

    fn response(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
        response::convert(body, plan)
    }

    fn stream(&self, plan: &Plan) -> Result<Box<dyn StreamBridge>, Error> {
        Ok(Box::new(stream::Bridge::new(plan)))
    }
}
