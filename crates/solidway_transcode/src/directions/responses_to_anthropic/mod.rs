//! Responses -> Anthropic Messages。

mod request;
mod response;
mod stream;

use crate::protocol::{DirectionCaps, ProtocolId, RequestCaps, ResponseCaps, StreamCaps, Support};
use crate::registry::{Direction, Plan, StreamBridge, Transcoded};
use crate::report::Error;

pub(crate) struct ResponsesToAnthropic;

pub(crate) static DIRECTION: ResponsesToAnthropic = ResponsesToAnthropic;

pub(crate) const CAPS: DirectionCaps = DirectionCaps {
    request: RequestCaps {
        documents: Support::Emulated,
        structured_output: Support::Lossy(
            "Responses text.format is mapped to Anthropic output_config.format",
        ),
        audio_input: Support::Unsupported("Anthropic has no audio input"),
        custom_tools: Support::Unsupported("Anthropic has no custom/grammar tools"),
        hosted_tools: Support::Unsupported("Anthropic has no hosted/server tools"),
        parallel_tool_calls: Support::Unsupported("Anthropic has no parallel_tool_calls"),
        cache_control: Support::Lossy("Responses prompt_cache_key is dropped"),
        thinking: Support::Lossy("Responses reasoning is mapped to an Anthropic thinking budget"),
        ..RequestCaps::DEFAULT
    },
    response: ResponseCaps {
        refusal: Support::Lossy("refusal is mapped to a text block"),
        thinking_signature: Support::Emulated,
        citations: Support::Lossy("Anthropic citations are mapped to text annotations"),
        server_tools: Support::Lossy("server_tool_use + tool_result are mapped to web_search_call"),
        ..ResponseCaps::DEFAULT
    },
    stream: StreamCaps {
        thinking: Support::Lossy("Anthropic thinking is carried as reasoning summary"),
        parallel_tool_calls: Support::Lossy("Anthropic content blocks are sequential"),
        keepalive: Support::Unsupported("Anthropic ping has no Responses equivalent"),
        ..StreamCaps::DEFAULT
    },
};

impl Direction for ResponsesToAnthropic {
    fn src(&self) -> ProtocolId {
        ProtocolId::OPENAI_RESPONSES
    }

    fn dst(&self) -> ProtocolId {
        ProtocolId::ANTHROPIC_MESSAGES
    }

    fn caps(&self) -> &'static DirectionCaps {
        &CAPS
    }

    fn request(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
        let req: crate::wire::openai_responses::ResponsesRequest =
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
