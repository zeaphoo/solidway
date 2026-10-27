//! Anthropic Messages -> Responses。

mod request;
mod response;
mod stream;

use crate::protocol::{DirectionCaps, ProtocolId, RequestCaps, ResponseCaps, StreamCaps, Support};
use crate::registry::{Direction, Plan, StreamBridge, Transcoded};
use crate::report::Error;

pub(crate) struct AnthropicToResponses;

pub(crate) static DIRECTION: AnthropicToResponses = AnthropicToResponses;

pub(crate) const CAPS: DirectionCaps = DirectionCaps {
    request: RequestCaps {
        documents: Support::Emulated,
        thinking: Support::Lossy("Anthropic thinking is mapped to Responses reasoning"),
        structured_output: Support::Lossy(
            "Anthropic output_config.format is mapped to Responses text.format",
        ),
        audio_input: Support::Unsupported("Responses has no Anthropic audio input"),
        custom_tools: Support::Unsupported("Anthropic has no custom tools"),
        hosted_tools: Support::Unsupported(
            "Anthropic server tools are not mapped to Responses hosted tools",
        ),
        top_k: Support::Unsupported("Responses has no top_k"),
        cache_control: Support::Lossy("Anthropic cache_control has no Responses field"),
        stop: Support::Unsupported("Responses has no stop"),
        ..RequestCaps::DEFAULT
    },
    response: ResponseCaps {
        refusal: Support::Lossy("refusal is mapped to a text block"),
        thinking_signature: Support::Emulated,
        citations: Support::Lossy("Responses text annotations are mapped to Anthropic citations"),
        server_tools: Support::Lossy("web_search_call is mapped to server_tool_use + tool_result"),
        ..ResponseCaps::DEFAULT
    },
    stream: StreamCaps {
        thinking: Support::Lossy("Anthropic thinking is carried as reasoning summary"),
        parallel_tool_calls: Support::Lossy("Responses output items are per index"),
        keepalive: Support::Unsupported("Anthropic ping has no Responses equivalent"),
        ..StreamCaps::DEFAULT
    },
};

impl Direction for AnthropicToResponses {
    fn src(&self) -> ProtocolId {
        ProtocolId::ANTHROPIC_MESSAGES
    }

    fn dst(&self) -> ProtocolId {
        ProtocolId::OPENAI_RESPONSES
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
