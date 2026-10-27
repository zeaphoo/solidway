//! Chat Completions -> Anthropic Messages。

mod request;
mod response;
mod stream;

use crate::protocol::{DirectionCaps, ProtocolId, RequestCaps, ResponseCaps, StreamCaps, Support};
use crate::registry::{Direction, Plan, StreamBridge, Transcoded};
use crate::report::Error;

pub(crate) struct ChatToAnthropic;

pub(crate) static DIRECTION: ChatToAnthropic = ChatToAnthropic;

pub(crate) const CAPS: DirectionCaps = DirectionCaps {
    request: RequestCaps {
        developer_role: Support::Emulated,
        documents: Support::Emulated,
        audio_input: Support::Unsupported("Anthropic has no audio input"),
        custom_tools: Support::Unsupported("Chat has no custom tools"),
        hosted_tools: Support::Unsupported("Chat has no hosted tools"),
        thinking: Support::Lossy("Chat reasoning_effort is mapped to a thinking budget"),
        top_k: Support::Lossy("Chat has no top_k; Anthropic top_k is left unset"),
        seed: Support::Unsupported("Anthropic has no seed"),
        penalties: Support::Unsupported("Anthropic has no presence/frequency penalty"),
        n_choices: Support::Unsupported("Anthropic has no n"),
        logprobs: Support::Unsupported("Anthropic has no logprobs"),
        parallel_tool_calls: Support::Unsupported("Anthropic has no parallel_tool_calls"),
        cache_control: Support::Lossy(
            "Chat cache breakpoints are dropped or mapped to cache_control",
        ),
        structured_output: Support::Lossy("Chat response_format is mapped to output_config.format"),
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
        thinking: Support::Lossy("thinking is streamed as reasoning_content"),
        parallel_tool_calls: Support::Lossy(
            "Chat stream supports parallel tool calls; Anthropic blocks are sequential",
        ),
        keepalive: Support::Unsupported("Chat stream has no ping/keepalive event"),
        ..StreamCaps::DEFAULT
    },
};

impl Direction for ChatToAnthropic {
    fn src(&self) -> ProtocolId {
        ProtocolId::OPENAI_CHAT
    }

    fn dst(&self) -> ProtocolId {
        ProtocolId::ANTHROPIC_MESSAGES
    }

    fn caps(&self) -> &'static DirectionCaps {
        &CAPS
    }

    fn request(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
        let req: crate::wire::openai_chat::ChatRequest =
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
